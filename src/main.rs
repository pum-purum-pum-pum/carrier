use std::sync::Arc;

use log::LevelFilter;

use failure::Error;

use tokio::net::TcpListener;
use tokio::sync::Mutex;

use carrier::misc::SequencedQueue;
use carrier::server::ChatState;
use carrier::{process_client, process_event_source};

const TOTAL_EVENTS: u32 = 1_000;
const CLIENT_RECEIVER_TIMEOUT: u64 = 10;

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::builder().filter_level(LevelFilter::Info).init();
    // information about users
    let shared_state = Arc::new(Mutex::new(ChatState::default()));
    // sequenced messages from event source
    let sequenced_queue = Arc::new(Mutex::new(SequencedQueue::new(TOTAL_EVENTS)));
    let mut event_source_listener = TcpListener::bind("127.0.0.1:9999").await?;
    let mut clients_listner = TcpListener::bind("127.0.0.1:9990").await?;
    // we spawn event source listener in order to pass chat-app test
    // while it seems to be more correct to wait until it's connected
    let state = Arc::clone(&shared_state);
    let queue = Arc::clone(&sequenced_queue);
    tokio::spawn(async move {
        match event_source_listener.accept().await {
            Ok((stream, _addr)) => {
                if let Err(error) = process_event_source(state, queue, stream).await {
                    log::error!("Error during processing event source: {}", error);
                }
            }
            Err(error) => {
                log::error!("Failed to accept event source connection: {}", error);
            }
        }
    });
    loop {
        let state = Arc::clone(&shared_state);
        let queue = Arc::clone(&sequenced_queue);
        match clients_listner.accept().await {
            Ok((stream, _addr)) => {
                tokio::spawn(async move {
                    let peer = state.lock().await.new_peer(stream).await;
                    match peer {
                        Ok(peer) => {
                            if let Err(error) = process_client(queue, peer, CLIENT_RECEIVER_TIMEOUT).await
                            {
                                log::error!("Error during processing client: {}", error)
                            }
                        },
                        Err(error) => {
                            log::error!("Failed to connect client: {}", error);
                        }
                    }
                });
            }
            Err(error) => {
                log::error!("Failed to accept client connection: {}", error);
            }
        }
    }
}
