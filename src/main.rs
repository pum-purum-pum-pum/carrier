#![recursion_limit = "1024"]
use std::sync::Arc;

use log::LevelFilter;

use failure::Error;

use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};
use tokio_util::codec::{Framed, LinesCodec};

use crate::clients_processing::{init_event_source, listen_events, process_client};
use crate::sequenced_queue::SequencedQueue;
use crate::server::Users;

// dependecnies update needed
pub use chat_app::event::Event;

pub const TOTAL_EVENTS: u32 = 1_000_000;
pub const CLIENT_RECEIVER_TIMEOUT_MILLIS: u64 = 10;
pub const LOG_EVERY_MILLIS: u32 = TOTAL_EVENTS / 10;
// Timeout is a debugging hack.
// It will drop the accepting of a connection(even in progress) every 5 seconds and log the server state
pub const ACCEPTING_LOG_INTERVAL_SEC: u64 = 5;

/// clients and event source processors
pub mod clients_processing;
/// A queue with a guarantee of the return of sequential elements.
pub mod sequenced_queue;
/// Users and functions to execute events on them
pub mod server;
#[cfg(test)]
mod tests;
pub mod user;

/// Shared chat state
pub type State = Arc<Mutex<Users>>;
/// Shared sequenced queue with messages(events) from _event source_
pub type Queue = Arc<Mutex<SequencedQueue<Event>>>;

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::builder().filter_level(LevelFilter::Info).init();
    // sequenced messages from event source
    let incomming_events = Arc::new(Mutex::new(SequencedQueue::new(TOTAL_EVENTS)));

    // wait until events source is connected
    let mut event_source_listener = TcpListener::bind("127.0.0.1:9999").await?;
    let (stream, _addr) = event_source_listener.accept().await?;
    log::info!("Event source connected");
    let mut event_source_stream = Framed::new(stream, LinesCodec::new());
    let users_number = init_event_source(&mut event_source_stream).await?;
    let users = Arc::new(Mutex::new(Users::new(users_number)));

    log::info!("waiting users to connect");
    let mut clients_listner = TcpListener::bind("127.0.0.1:9990").await?;
    let queue = Arc::clone(&incomming_events);
    let state = Arc::clone(&users);
    // spawn task to process events from event source
    tokio::spawn(async move {
        if let Err(error) = listen_events(&mut event_source_stream, state, queue).await {
            log::error!("error while listen events: {}", error);
        }
        log::info!("all events processed. close event listner");
    });
    // connect new clients
    loop {
        let state = Arc::clone(&users);
        let queue = Arc::clone(&incomming_events);
        let accept = clients_listner.accept();
        match timeout(Duration::from_secs(ACCEPTING_LOG_INTERVAL_SEC), accept).await {
            Ok(Ok((stream, _addr))) => {
                // asynchronously process clients
                tokio::spawn(async move {
                    process_client(stream, state, queue).await;
                });
            }
            Ok(Err(error)) => {
                log::error!("Failed to accept client connection: {}", error);
            }
            // time elapsed
            Err(_) => {
                log::info!(
                    "waiting for new clients(this message will appear every {} sec)",
                    ACCEPTING_LOG_INTERVAL_SEC
                );
            }
        }
    }
}
