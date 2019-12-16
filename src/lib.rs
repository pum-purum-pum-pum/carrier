/*!
System which acts as a socket listener that reads events from an _event source_ and forwards them to the relevant _user clients_.
*/

use std::sync::Arc;
use std::collections::VecDeque;

use failure::{bail, Error};

use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};
use tokio_util::codec::{Framed, LinesCodec};

use futures::future::FutureExt;
use futures::{select, StreamExt};
use futures_util::sink::SinkExt;

use sequenced_queue::SequencedQueue;
use server::{Peer, ServerState};

// TODO dependecnies update.
use chat_app::event::Event;

const LOG_EVERY: u32 = 1000;

/// A queue with a guarantee of the return of sequential elements.
pub mod sequenced_queue;
/// Users and functions to execute events on them
pub mod server;
#[cfg(test)]
mod tests;
pub mod user;

/// Shared chat state
pub type State = Arc<Mutex<ServerState>>;
/// Shared sequenced queue with messages(events) from _event source_
pub type Queue = Arc<Mutex<SequencedQueue<Event>>>;

/// Connect event source and accept(and return) number of users and initialize them in state
pub async fn init_event_source(
    event_source: &mut Framed<TcpStream, LinesCodec>,
    state: State,
) -> Result<u32, Error> {
    let user_num: u32 = if let Some(Ok(line)) = event_source.next().await {
        u32::from_str_radix(&line, 10)?
    } else {
        bail!("failed to parse number of users in event source message");
    };
    state.lock().await.generate_users(user_num);
    Ok(user_num)
}

/// Send messages from event source into queue and asynchronously process them
pub async fn listen_events(
    event_source: &mut Framed<TcpStream, LinesCodec>,
    state: State,
    incomming_events: Queue,
) -> Result<(), Error> {
    let mut last_timestamp = 0;
    let mut current_id = 0;
    while let Some(Ok(line)) = event_source.next().await {
        current_id += 1;
        if current_id - last_timestamp >= LOG_EVERY {
            last_timestamp = current_id;
            log::info!("{} events processed", current_id);
        }
        let event = Event::parse(&line)?;
        let sq = Arc::clone(&incomming_events);
        tokio::spawn(async move {
            sq.lock().await.insert(event.0, event.1);
        });
        // processing events asap
        let sq = Arc::clone(&incomming_events);
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(error) = process_and_forward(sq, state).await {
                log::error!("Error during processing events queue {}", error);
            }
        });
    }
    // drain events from queue
    while !incomming_events.lock().await.finished() {
        process_and_forward(Arc::clone(&incomming_events), Arc::clone(&state)).await?;
    }
    log::info!("all events processed. close event listner");
    Ok(())
}

/// Send messages from event source into queue and asynchronously process them
pub async fn process_event_source(
    state: State,
    incomming_events: Queue,
    stream: TcpStream,
) -> Result<(), Error> {
    let mut lines = Framed::new(stream, LinesCodec::new());
    init_event_source(&mut lines, Arc::clone(&state)).await?;
    listen_events(&mut lines, state, incomming_events).await?;
    Ok(())
}

pub async fn update_state(state: State, id: u32, event: Event) -> Result<(), Error> {
    let mut state = state.lock().await;
    let msg = format!("{}/{}", id, event);
    // if message is not possible to send we store it in state in order to send later
    // (it's not possible to do implicit because we are borrowing state)
    let mut await_messages: VecDeque<(u32, String)> = VecDeque::new(); 
    match event {
        // Actor should now be following Target. Target is expected to receive this event.
        Event::Follow { from, to } => {
            if let Some(user) = state.users.get_mut(&to) {
                if user.is_not_blocked(from) {
                    user.followers.insert(from);
                    if let Some(target_peer) = state.peers.get(&to) {
                        target_peer.send(msg.clone())?;
                    } else {
                        await_messages.push_back((to, msg.clone()));
                    };
                }
            };
        }
        // Target should be removed from Actor's followers. No one is expected to receive this event.
        // In test code turns out that `to` is expected to recive the message, so we send it
        Event::Unfollow { from, to } => {
            if let Some(user) = state.users.get_mut(&to) {
                if user.is_not_blocked(from) {
                    user.followers.remove(&from);
                    if let Some(target_peer) = state.peers.get(&to) {
                        target_peer.send(msg.clone())?
                    } else {
                        await_messages.push_back((to, msg.clone()))
                    }
                }
            }
        }
        // All followers of Actor are expected to receive this event.
        Event::StatusUpdate { from, .. } => {
            if let Some(user) = state.users.get(&from) {
                let recipients = &user.followers - &user.blocked;
                for recipient in recipients.iter() {
                    if let Some(peer) = state.peers.get(recipient) {
                        peer.send(msg.clone())?;
                    } else {
                        await_messages.push_back((*recipient, msg.clone()));
                    }
                }
            }
        }
        // Target expects to receive this event.
        Event::PrivateMessage {
            from,
            to,
            ..
        } => {
            if let Some(user) = state.users.get(&to) {
                if user.is_not_blocked(from) {
                    if let Some(target_peer) = state.peers.get(&to) {
                        target_peer.send(msg.clone())?;
                    } else {
                        await_messages.push_back((to, msg.clone()));
                    }
                }
            }
        }
        // Prevents Actor from receiving a Follow, Status Update, or Private Message from Target.
        Event::Block { from, to } => {
            if let Some(blocked) = state.mut_blocked(from) {
                blocked.insert(to);
            }
        }
    }
    // for each user assign messages
    for (to, msg) in await_messages.drain(..) {
        if let Some(user) = state.users.get_mut(&to) {
            user.await_messages.push_back(msg)
        }
    }
    Ok(())
}

/// Check new messages in queue then apply events to state and forward events to connected clients if possible.
/// Queue is locked until we drain all events from it
pub async fn process_and_forward(incomming_events: Queue, state: State) -> Result<(), Error> {
    let mut incomming_events = incomming_events.lock().await;
    while let Some((id, item)) = incomming_events.next() {
        update_state(Arc::clone(&state), id, item).await?;
    }
    Ok(())
}

/// Perform retranslating messages from queue and check timeout afterwards
pub async fn process_client(
    queue: Queue,
    mut peer: Peer,
    timeout_millis: u64,
) -> Result<(), Error> {
    loop {
        let mut queue = Box::pin(queue.lock().fuse());
        let mut msg = peer.rx.next().fuse();
        select!(
            queue = queue => {
                if queue.finished() {
                    // make sure there is no other messages in receiver before closing socket
                    match timeout(Duration::from_millis(timeout_millis), peer.rx.next()).await {
                        Err(_) => {
                            break;
                        }
                        Ok(Some(msg)) => {
                            log::info!("{} send: {}", peer.id, msg);
                            peer.lines.send(msg).await?;
                        }
                        Ok(None) => (),
                    }
                }
            }
            msg = msg => {
                if let Some(msg) = msg {
                    log::info!("{} send: {}", peer.id, msg);
                    peer.lines.send(msg).await?;
                } else {
                    break
                }
            }
        );
    }
    log::info!("Client with id={} connection closed", peer.id);
    Ok(())
}
