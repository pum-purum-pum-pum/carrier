use std::collections::VecDeque;
use std::sync::Arc;

use failure::{bail, Error};

use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};
use tokio_util::codec::{Framed, LinesCodec};

use futures::future::FutureExt;
use futures::{select, StreamExt};
use futures_util::sink::SinkExt;

use crate::server::Peer;
use crate::{Event, Queue, State, CLIENT_RECEIVER_TIMEOUT_MILLIS, LOG_EVERY};

/// Connect event source and accept(and return) number of users and initialize them in state
pub async fn init_event_source(
    event_source: &mut Framed<TcpStream, LinesCodec>,
) -> Result<u32, Error> {
    let user_num: u32 = if let Some(Ok(line)) = event_source.next().await {
        u32::from_str_radix(&line, 10)?
    } else {
        bail!("failed to get number of users from event source");
    };
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
        incomming_events.lock().await.insert(event.0, event.1);
        // processing events asap
        while let Some((id, item)) = incomming_events.lock().await.next() {
            update_state(Arc::clone(&state), id, item).await?;
        }
    }
    Ok(())
}

/// Send messages from event source into queue and process them
pub async fn process_event_source(
    state: State,
    incomming_events: Queue,
    stream: TcpStream,
) -> Result<(), Error> {
    let mut lines = Framed::new(stream, LinesCodec::new());
    init_event_source(&mut lines).await?;
    listen_events(&mut lines, state, incomming_events).await?;
    Ok(())
}

/// Lock state and update it by applying event
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
                    state.send_or_keep(to, msg.clone(), &mut await_messages)?;
                }
            };
        }
        // Target should be removed from Actor's followers. No one is expected to receive this event.
        // In test code turns out that `to` is expected to recive the message, so we send it
        Event::Unfollow { from, to } => {
            if let Some(user) = state.users.get_mut(&to) {
                if user.is_not_blocked(from) {
                    user.followers.remove(&from);
                    state.send_or_keep(to, msg.clone(), &mut await_messages)?;
                }
            }
        }
        // All followers of Actor are expected to receive this event.
        Event::StatusUpdate { from, .. } => {
            if let Some(user) = state.users.get(&from) {
                for follower in user.followers.iter() {
                    if state.is_not_blocked(from, *follower) {
                        state.send_or_keep(*follower, msg.clone(), &mut await_messages)?;
                    }
                }
            }
        }
        // Target expects to receive this event.
        Event::PrivateMessage { from, to, .. } => {
            if let Some(user) = state.users.get(&to) {
                if user.is_not_blocked(from) {
                    state.send_or_keep(to, msg.clone(), &mut await_messages)?;
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

/// connect new client and forward events into it
pub async fn process_client(stream: TcpStream, state: State, incomming_events: Queue) {
    let peer = state.lock().await.new_peer(stream).await;
    match peer {
        Ok(peer) => {
            log::info!("connected new client with id {}", peer.id);
            if let Err(error) = forward_messages(
                incomming_events,
                peer,
                Arc::clone(&state),
                CLIENT_RECEIVER_TIMEOUT_MILLIS,
            )
            .await
            {
                log::error!("Error during processing client: {}", error)
            }
        }
        Err(error) => {
            log::error!("Failed to connect client: {}", error);
        }
    }
}

/// Perform retranslating messages from queue to client socket and check timeout afterwards
pub async fn forward_messages(
    queue: Queue,
    mut peer: Peer,
    state: State,
    timeout_millis: u64,
) -> Result<(), Error> {
    loop {
        let mut finished = Box::pin(queue.lock().map(|queue| queue.finished()).fuse());
        let mut msg = peer.rx.next().fuse();
        #[deny(clippy::unnecessary_mut_passed)]
        select!(
            // no more events in queue
            finished = finished => {
                if finished {
                    // make sure there is no other messages in receiver before closing socket
                    match timeout(Duration::from_millis(timeout_millis), msg).await {
                        Ok(Some(msg)) => {
                            peer.lines.send(msg).await?;
                        }
                        Ok(None) => {
                            break
                        },
                        Err(error) => {
                            // deadline ha elapsed and rc is empty
                            break;
                        }
                    }
                }
            }
            msg = msg => {
                if let Some(msg) = msg {
                    // log::info!("sending");
                    // let msg = format!("{}\n", msg);
                    peer.lines.send(msg).await?;
                } else {
                    break
                }
            }
        );
    }
    state.lock().await.peers.remove(&peer.id);
    log::info!("Client with id={} connection closed", peer.id);
    Ok(())
}
