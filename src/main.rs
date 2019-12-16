#![recursion_limit="1024"]
use std::sync::Arc;

use log::LevelFilter;

use failure::Error;

use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_util::codec::{Framed, LinesCodec};
use tokio::time::{timeout, Duration, delay_for};

use futures::future::FutureExt;
use futures::{select, StreamExt};
use futures::future::join;

use carrier::sequenced_queue::SequencedQueue;
use carrier::server::ServerState;
use carrier::{init_event_source, listen_events, process_client, TOTAL_EVENTS, CLIENT_RECEIVER_TIMEOUT_MILLIS};

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
    });
    let state = Arc::clone(&shared_state);
    // connect new clients
    loop {
        // log::info!("[[");
        let s = Arc::clone(&state);
        let q = Arc::clone(&incomming_events);
        let state = Arc::clone(&state);
        let queue = Arc::clone(&incomming_events);
        let mut accept = Box::pin(clients_listner.accept()).fuse();
        let mut state_select = Box::pin(s.lock()).fuse();
        match accept.await {
            Ok((stream, _addr)) => {
                // asynchronously process clients
                tokio::spawn(async move {
                    let peer = state.lock().await.new_peer(stream).await;
                    match peer {
                        Ok(peer) => {
                            log::info!("connected new client with id {}", peer.id);
                            if let Err(error) =
                                process_client(queue, peer, Arc::clone(&state), CLIENT_RECEIVER_TIMEOUT_MILLIS).await
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
        // select!(
        //     accept = accept => {
        //     }
        //     state_select = state_select => {
        //         // let (state, queue) = state_queue;
        //         if state_select.peers.len() == 0 && q.lock().await.finished() {
        //             break;
        //         }
        //     }
        // );
        // log::info!("]]");
    }
    // Ok(())
}
