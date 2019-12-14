// use std::time::Duration;
// use std::sync::Arc;
// use std::collections::{HashMap};

// use tokio::prelude::*;
// use tokio::sync::Mutex;
// use tokio::sync::mpsc;
// use tokio::net::TcpStream;
// use tokio::net::TcpListener;
// use tokio::time::delay_for;
// use tokio_util::codec::{Framed, LinesCodec};

// use futures::StreamExt;

// use crate::server::{Tx, Rx, ChatState, Peer};
// use crate::misc::SequencedQueue;
// use crate::{Queue, process_event_source, process_client};

// const TEST_ADDRESS: &str = "127.0.0.1:9938";
// const TEST_ADDRESS2: &str = "127.0.0.1:9939";
// const FOLLOW1_FROM1_TO2: &str = "1/F/1/2\n";

// #[derive(Debug)]
// struct Client {
// 	pub id: u32,
// 	pub rx: Rx,
// }

// struct ClientProvider {
// 	pub current_id: u32,
// }

// impl Default for ClientProvider {
// 	fn default() -> Self {
// 		Self {
// 			current_id: 1,
// 		}
// 	}
// }

// impl ClientProvider {
// 	fn next(&mut self) -> (Client, Tx) {
// 		let (tx, rx): (Tx, Rx) = mpsc::unbounded_channel();
// 		let res = Client {
// 			id: self.current_id,
// 			rx
// 		};
// 		self.current_id += 1;
// 		(res, tx)
// 	}
// }

// #[tokio::test]
// async fn follow() {
// 	let mut chat_state = ChatState::default();
// 	let mut client_provider = ClientProvider::default();
// 	let mut clients = HashMap::new();
// 	for _ in 1..=2 {
// 		let (client, tx) = client_provider.next();
// 		let stream = TcpStream::connect(TEST_ADDRESS).await.unwrap();
// 		chat_state.new_peer(client.id, stream);
// 		clients.insert(client.id, client);
// 	}
// 	chat_state.follow(1, 2, "a").unwrap();
// 	chat_state.follow(2, 1, "b").unwrap();
// 	let b = clients.get_mut(&1).unwrap().rx.next().await.unwrap();
// 	let a = clients.get_mut(&2).unwrap().rx.next().await.unwrap();
// 	assert_eq!(a, "a");
// 	assert_eq!(b, "b");
// }

// #[test]
// fn test_queue() {
// 	let mut objects = vec![(1, "a"), (2, "b"), (4, "c")];
// 	let mut queue = SequencedQueue::new(objects.len() as u32);
// 	for (id, msg) in objects.drain(..) {
// 		queue.insert(id, msg);
// 	}
// 	assert_eq!(queue.next().unwrap(), "a");
// 	assert_eq!(queue.next().unwrap(), "b");
// 	assert_eq!(queue.next(), None);
// }

// #[tokio::test(threaded_scheduler)]
// async fn event_source() {
// 	let queue_size = 1;
//     let chat_state = Arc::new(Mutex::new(ChatState::default()));
//     let sequenced_queue: Queue = Arc::new(Mutex::new(SequencedQueue::new(queue_size)));
// 	let mut client_provider = ClientProvider::default();
// 	let mut clients = HashMap::new();
// 	for _ in 1..=2 {
// 		let (client, tx) = client_provider.next();
// 		let stream = TcpStream::connect(TEST_ADDRESS).await.unwrap();
// 		chat_state.lock().await.new_peer(client.id, stream);
// 		clients.insert(client.id, client);
// 	}
//     let mut event_source_listener = TcpListener::bind(TEST_ADDRESS).await.unwrap();
//     tokio::spawn(async move {
// 		let mut stream = TcpStream::connect(TEST_ADDRESS).await.unwrap();
// 		// first send id
// 		stream.write_all("1\n".as_bytes()).await.unwrap();
// 		// second send message
// 		stream.write_all(FOLLOW1_FROM1_TO2.as_bytes()).await.unwrap();
//     });
// 	let (stream, _addr) = event_source_listener.accept().await.unwrap();
// 	process_event_source(chat_state, sequenced_queue, stream).await.unwrap();
// 	let msg = clients.get_mut(&2).unwrap().rx.next().await.unwrap();
// 	assert_eq!(msg, FOLLOW1_FROM1_TO2.split("\n").next().unwrap());
// }

// #[tokio::test(threaded_scheduler)]
// async fn client() {
// 	// let queue_size = 0; // queue is empty
//  //    let chat_state = Arc::new(Mutex::new(ChatState::default()));
//  //    let sequenced_queue: Queue = Arc::new(Mutex::new(SequencedQueue::new(queue_size)));

// 	// let mut client_provider = ClientProvider::default();
// 	// let mut clients = HashMap::new();
// 	// for _ in 1..=2 {
// 	// 	let (client, tx) = client_provider.next();
// 	// 	chat_state.lock().await.new_peer(client.id, tx);
// 	// 	clients.insert(client.id, client);
// 	// }
// 	// let mut client_listner = TcpListener::bind(TEST_ADDRESS2).await.unwrap();
// 	// let client_stream = TcpStream::connect(TEST_ADDRESS2).await.unwrap();
// 	// let (stream, _addr) = client_listner.accept().await.unwrap();
//  //    let state = Arc::clone(&chat_state);
//  //    let queue = Arc::clone(&sequenced_queue);
//  //    tokio::spawn(async move {
//  //    	delay_for(Duration::from_millis(10)).await;
//  //    	let peer = Peer::new(Arc::clone(&state), stream).await.unwrap();
// 	// 	process_client(state, queue, peer, 1).await.unwrap();
// 	// });
//  //    chat_state.lock().await.follow(1, 2, FOLLOW1_FROM1_TO2).unwrap();
//  //    let mut lines = Framed::new(client_stream, LinesCodec::new());
//  //    assert_eq!(lines.next().await.unwrap().unwrap(), FOLLOW1_FROM1_TO2);
// }