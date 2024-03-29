use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio::sync::Mutex;
use tokio_util::codec::{Framed, LinesCodec};

use futures::StreamExt;

use chat_app::event::Event;

use crate::clients_processing::{forward_messages, process_event_source, update_users};
use crate::sequenced_queue::SequencedQueue;
use crate::server::{Peer, Users};
use crate::SequencedQueueShared;

const TEST_ADDRESS: &str = "127.0.0.1:9938";
const TEST_ADDRESS2: &str = "127.0.0.1:9939";
const TEST_ADDRESS3: &str = "127.0.0.1:9940";
const TEST_ADDRESS4: &str = "127.0.0.1:9941";
const FOLLOW1_FROM1_TO2: &str = "1/F/1/2\n";

async fn fake_users(num: usize, adress: String) -> Vec<TcpStream> {
    let mut streams = vec![];
    for i in 1..=num {
        let mut stream = TcpStream::connect(adress.clone()).await.unwrap();
        let data = format!("{}\n", i);
        stream.write_all(data.as_bytes()).await.unwrap();
        streams.push(stream)
    }
    streams
}

async fn connect_clients(
    num: u32,
    chat_users: &mut Users,
    clients_listner: &mut TcpListener,
) -> Vec<Peer> {
    let mut clients = vec![];
    for _ in 1..=num {
        let (stream, _) = clients_listner.accept().await.unwrap();
        let peer = chat_users.new_peer(stream).await.unwrap();
        clients.push(peer);
    }
    clients
}

#[tokio::test]
async fn follow() {
    let users_num = 2;
    let mut clients_listner = TcpListener::bind(TEST_ADDRESS).await.unwrap();
    let mut chat_users = Users::new(users_num);
    let test_adress = TEST_ADDRESS.clone();
    fake_users(users_num as usize, test_adress.to_string()).await;
    let mut clients = connect_clients(users_num, &mut chat_users, &mut clients_listner).await;
    update_users(&mut chat_users, 1, Event::Follow { from: 1, to: 2 })
        .await
        .unwrap();
    update_users(&mut chat_users, 2, Event::Follow { from: 2, to: 1 })
        .await
        .unwrap();
    let b = clients[0].rx.next().await.unwrap();
    let a = clients[1].rx.next().await.unwrap();
    assert_eq!(a, format!("1/{}", Event::Follow { from: 1, to: 2 }));
    assert_eq!(b, format!("2/{}", Event::Follow { from: 2, to: 1 }));
    assert!(chat_users.followers(1).unwrap().contains(&2));
}

#[test]
fn test_queue() {
    let mut objects = vec![(1, "a"), (2, "b"), (4, "c")];
    let mut queue = SequencedQueue::new(objects.len() as u32);
    for (id, msg) in objects.drain(..) {
        queue.insert(id, msg);
    }
    assert_eq!(queue.next().unwrap(), (1, "a"));
    assert_eq!(queue.next().unwrap(), (2, "b"));
    assert_eq!(queue.next(), None);
}

#[tokio::test(threaded_scheduler)]
async fn client() {
    let users_num = 2;
    let queue_size = 0; // queue is empty
    let chat_users = Arc::new(Mutex::new(Users::new(users_num)));
    let incomming_events: SequencedQueueShared =
        Arc::new(Mutex::new(SequencedQueue::new(queue_size)));

    // create fake users and conncet to loca peers i.e."clients"
    let mut clients_listner = TcpListener::bind(TEST_ADDRESS4).await.unwrap();
    let mut fake_users_streams = fake_users(users_num as usize, TEST_ADDRESS4.to_string()).await;
    let mut clients = connect_clients(
        users_num,
        &mut *chat_users.lock().await,
        &mut clients_listner,
    )
    .await;

    let users = Arc::clone(&chat_users);
    tokio::spawn(async move {
        for peer in clients.drain(..) {
            let queue = Arc::clone(&incomming_events);
            let users = Arc::clone(&users);
            forward_messages(queue, peer, users, 1).await.unwrap();
        }
    });
    update_users(
        &mut *chat_users.lock().await,
        1,
        Event::Follow { from: 1, to: 2 },
    )
    .await
    .unwrap();
    let mut lines = Framed::new(fake_users_streams.swap_remove(1), LinesCodec::new());
    assert_eq!(
        lines.next().await.unwrap().unwrap(),
        FOLLOW1_FROM1_TO2.split("\n").next().unwrap()
    );
}

#[tokio::test(threaded_scheduler)]
async fn event_source() {
    let queue_size = 1;
    let users_num = 2;
    let chat_users = Arc::new(Mutex::new(Users::new(users_num)));
    let incomming_events: SequencedQueueShared =
        Arc::new(Mutex::new(SequencedQueue::new(queue_size)));

    // first we need to generate clients
    let mut clients_listner = TcpListener::bind(TEST_ADDRESS3).await.unwrap();
    fake_users(users_num as usize, TEST_ADDRESS3.to_string()).await;
    let mut clients = connect_clients(
        users_num,
        &mut *chat_users.lock().await,
        &mut clients_listner,
    )
    .await;

    // then we generate event source client and write our message
    let mut event_source_listener = TcpListener::bind(TEST_ADDRESS2).await.unwrap();
    tokio::spawn(async move {
        let mut stream = TcpStream::connect(TEST_ADDRESS2).await.unwrap();
        // first send number of users
        let users_num_msg = format!("{}\n", users_num);
        stream.write_all(users_num_msg.as_bytes()).await.unwrap();
        // then send a message
        stream
            .write_all(FOLLOW1_FROM1_TO2.as_bytes())
            .await
            .unwrap();
    });
    let (stream, _addr) = event_source_listener.accept().await.unwrap();
    process_event_source(Arc::clone(&chat_users), incomming_events, stream)
        .await
        .unwrap();

    let msg = clients[1].rx.next().await.unwrap();
    assert_eq!(msg, FOLLOW1_FROM1_TO2.split("\n").next().unwrap());
}
