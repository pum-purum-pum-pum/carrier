use std::sync::Arc;

use log::LevelFilter;

use failure::Error;

use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_util::codec::{Framed, LinesCodec};
use tokio::time::{timeout, Duration};

use carrier::sequenced_queue::SequencedQueue;
use carrier::server::ServerState;
use carrier::{init_event_source, listen_events, process_client};

const TOTAL_EVENTS: u32 = 1_000_0;
const CLIENT_RECEIVER_TIMEOUT_MILLIS: u64 = 1;
const ACCEPT_CLIENT_TIMEOUT: u64 = 10;

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
    let users_number = init_event_source(&mut event_source_stream, state).await?;
    
    // spawn task to process events from event source
    let queue = Arc::clone(&incomming_events);
    let state = Arc::clone(&shared_state);
    tokio::spawn(async move {
        if let Err(error) = listen_events(&mut event_source_stream, state, queue).await {
            log::error!("error while listen events: {}", error);
        }
    });

    log::info!("waiting {} users to connect", users_number);
    let mut clients_listner = TcpListener::bind("127.0.0.1:9990").await?;
    // connect new clients
    loop {
        let state = Arc::clone(&shared_state);
        let queue = Arc::clone(&incomming_events);
        match clients_listner.accept().await {
            Ok((stream, _addr)) => {
                tokio::spawn(async move {
                    let peer = state.lock().await.new_peer(stream).await;
                    match peer {
                        Ok(peer) => {
                            log::info!("connected new client with id {}", peer.id);
                            if let Err(error) =
                                process_client(queue, peer, CLIENT_RECEIVER_TIMEOUT_MILLIS).await
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
            Err(error) => {
                log::error!("Failed to accept client connection: {}", error);
            }
        }
    }
}
