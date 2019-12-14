/*!
System which acts as a socket listener that
reads events from an _event source_ and forwards them to the relevant _user
clients_.
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

use misc::SequencedQueue;
use server::{ChatState, Peer};

// TODO dependecnies update.
use chat_app::event::Event;

/// General functions and data structures
pub mod misc;
/// Core server logic
pub mod server;
pub mod user;
#[cfg(test)]
mod tests;

/// Shared chat state
pub type State = Arc<Mutex<ChatState>>;
/// Shared sequenced queue with messages(events) from _event source_
pub type Queue = Arc<Mutex<SequencedQueue<(Event, String)>>>;

/// Send messages from event source into queue and asynchronously process them
pub async fn process_event_source(
    state: State,
    sequenced_queue: Queue,
    stream: TcpStream,
) -> Result<(), Error> {
    {
        // do work with socket here
        let mut lines = Framed::new(stream, LinesCodec::new());
        let _user_num: u32 = if let Some(Ok(line)) = lines.next().await {
            u32::from_str_radix(&line, 10)?
        } else {
            bail!("failed to parse number of users in event source message");
        };
        while let Some(Ok(line)) = lines.next().await {
            let event = Event::parse(&line)?;
            let sq = Arc::clone(&sequenced_queue);
            tokio::spawn(async move {
                sq.lock().await.insert(event.0, (event.1, line.clone()));
            });
            // processing events asap
            let sq = Arc::clone(&sequenced_queue);
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                if let Err(error) = process_queue(sq, state).await {
                    log::error!("Error during processing events queue {}", error);
                }
            });
        }
    }
    // drain queue
    while !sequenced_queue.lock().await.finished() {
        process_queue(Arc::clone(&sequenced_queue), Arc::clone(&state)).await?;
    }
    Ok(())
}

/// Check new messages in queue and process them.
/// Queue is locked until we drain all events from it
pub async fn process_queue(sequenced_queue: Queue, state: State) -> Result<(), Error> {
    let mut sequenced_queue = sequenced_queue.lock().await;
    while let Some((item, msg)) = sequenced_queue.next() {
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

/// Either message from receiver or state of the message queue
#[derive(Debug)]
enum GracefulMessage {
    QueueEmptiness(bool),
    Message(Option<String>),
}

/// Perform retranslating messages from queue and check timeout afterwards
pub async fn process_client(
    state: State,
    queue: Queue,
    stream: TcpStream,
    timeout_millis: u64,
) -> Result<(), Error> {
    let mut peer = Peer::new(state, stream).await?;
    loop {
        let mut queue = Box::pin(queue.lock().fuse());
        let mut msg = peer.rx.next().fuse();
        let message = select!(
            queue = queue => {
                GracefulMessage::QueueEmptiness(queue.finished())
            }
            msg = msg => {
                GracefulMessage::Message(msg)
            }
        );
        match message {
            GracefulMessage::QueueEmptiness(empty) => {
                if empty {
                    // make sure there is no other messages in receiver before closing socket
                    match timeout(Duration::from_millis(timeout_millis), peer.rx.next()).await {
                        Err(_) => {
                            log::info!("done");
                            break;
                        }
                        Ok(Some(msg)) => {
                            peer.lines.send(msg).await?;
                        }
                        Ok(None) => (),
                    }
                }
            }
            GracefulMessage::Message(Some(msg)) => {
                peer.lines.send(msg).await?;
            }
            GracefulMessage::Message(None) => break,
        }
    }
    Ok(())
}
