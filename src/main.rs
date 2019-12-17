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
use crate::server::ServerState;

// TODO dependecnies update.
pub use chat_app::event::Event;

pub const TOTAL_EVENTS: u32 = 1_000_000;
pub const CLIENT_RECEIVER_TIMEOUT_MILLIS: u64 = 10;
pub const LOG_EVERY: u32 = TOTAL_EVENTS / 10;
pub const ACCEPT_TIMEOUT: u64 = 5;

pub mod clients_processing;
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

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::builder().filter_level(LevelFilter::Info).init();
    // information about users
    let shared_state = Arc::new(Mutex::new(ServerState::default()));
    // sequenced messages from event source
    let incomming_events = Arc::new(Mutex::new(SequencedQueue::new(TOTAL_EVENTS)));

    // wait until events source is connected
    let mut event_source_listener = TcpListener::bind("127.0.0.1:9999").await?;
    let state = Arc::clone(&shared_state);
    let (stream, _addr) = event_source_listener.accept().await?;
    log::info!("Event source connected");
    let mut event_source_stream = Framed::new(stream, LinesCodec::new());
    let _users_number = init_event_source(&mut event_source_stream, state).await?;

    // spawn task to process events from event source
    let queue = Arc::clone(&incomming_events);

    log::info!("waiting users to connect");
    let mut clients_listner = TcpListener::bind("127.0.0.1:9990").await?;
    let state = Arc::clone(&shared_state);
    tokio::spawn(async move {
        if let Err(error) = listen_events(&mut event_source_stream, state, queue).await {
            log::error!("error while listen events: {}", error);
        }
        log::info!("all events processed. close event listner");
    });
    let state = Arc::clone(&shared_state);
    // connect new clients
    loop {
        let state = Arc::clone(&state);
        let queue = Arc::clone(&incomming_events);
        let accept = clients_listner.accept();
        match timeout(Duration::from_secs(ACCEPT_TIMEOUT), accept).await {
            Ok(Ok((stream, _addr))) => {
                // asynchronously process clients
                tokio::spawn(async move {
                    let peer = state.lock().await.new_peer(stream).await;
                    match peer {
                        Ok(peer) => {
                            log::info!("connected new client with id {}", peer.id);
                            if let Err(error) = process_client(
                                queue,
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
                });
            }
            Ok(Err(error)) => {
                log::error!("Failed to accept client connection: {}", error);
            }
            // time elapsed
            Err(_) => {
                log::info!(
                    "waiting for new clients(this message will appear every {} sec)",
                    ACCEPT_TIMEOUT
                );
            }
        }
    }
}
