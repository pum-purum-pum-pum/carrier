/*!
System which acts as a socket listener that reads events from an _event source_ and forwards them to the relevant _user clients_.
*/

use std::sync::Arc;

use failure::{bail, Error};

use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};
use tokio_util::codec::{Framed, LinesCodec};

use futures::future::FutureExt;
use futures::{select, StreamExt};
use futures_util::sink::SinkExt;

use sequenced_queue::SequencedQueue;
use server::{ServerState, Peer};

// TODO dependecnies update.
use chat_app::event::Event;

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
pub type Queue = Arc<Mutex<SequencedQueue<(Event, String)>>>;

/// Send messages from event source into queue and asynchronously process them
pub async fn process_event_source(
    state: State,
    incomming_events: Queue,
    stream: TcpStream,
) -> Result<(), Error> {
    {
        // do work with socket here
        let mut lines = Framed::new(stream, LinesCodec::new());
        let user_num: u32 = if let Some(Ok(line)) = lines.next().await {
            u32::from_str_radix(&line, 10)?
        } else {
            bail!("failed to parse number of users in event source message");
        };
        state.lock().await.generate_users(user_num);
        while let Some(Ok(line)) = lines.next().await {
            let event = Event::parse(&line)?;
            let sq = Arc::clone(&incomming_events);
            tokio::spawn(async move {
                sq.lock().await.insert(event.0, (event.1, line.clone()));
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
    }
    // drain events from queue
    while !incomming_events.lock().await.finished() {
        process_and_forward(Arc::clone(&incomming_events), Arc::clone(&state)).await?;
    }
    Ok(())
}

/// Check new messages in queue then process and forward them to connected clients.
/// Queue is locked until we drain all events from it
pub async fn process_and_forward(incomming_events: Queue, state: State) -> Result<(), Error> {
    let mut incomming_events = incomming_events.lock().await;
    while let Some((item, msg)) = incomming_events.next() {
        match item {
            Event::Follow { from, to } => {
                state.lock().await.follow(from, to, &msg)?;
            }
            Event::Unfollow { from, to } => {
                state.lock().await.unfollow(from, to);
            }
            Event::StatusUpdate { from, message: _ } => {
                state.lock().await.status_update(from, &msg).await?;
            }
            Event::PrivateMessage {
                from,
                to,
                message: _,
            } => state.lock().await.private_message(from, to, &msg).await?,
            Event::Block { from, to } => {
                state.lock().await.block(from, to).await;
            }
        }
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
                            peer.lines.send(msg).await?;
                        }
                        Ok(None) => (),
                    }
                }
            }
            msg = msg => {
                if let Some(msg) = msg {
                    peer.lines.send(msg).await?;
                } else {
                    break
                }
            }
        );
    }
    Ok(())
}
