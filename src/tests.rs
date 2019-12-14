use std::collections::{HashMap};

use tokio::sync::mpsc;

use crate::server::{Tx, Rx, ChatState};

struct Client {
	pub id: u32,
	pub rx: Rx,
}

struct ClientProvider {
	current_id: u32,
	peers: HashMap<u32, Tx>,
}

impl Default for ClientProvider {
	fn default() -> Self {
		Self {
			current_id: 1,
			..Default::default()
		}
	}
}

impl ClientProvider {
	fn next(&mut self) -> Client {
		let (tx, rx): (Tx, Rx) = mpsc::unbounded_channel();
		let res = Client {
			id: self.current_id,
			rx
		};
		self.peers.insert(self.current_id, tx);
		self.current_id += 1;
		res
	}

	fn peer(&self, id: u32) -> Option<&Tx> {
		self.peers.get(&id)
	}
}

#[test]
fn chat() {
	let mut chat_state = ChatState::default();
	let mut clients = ClientProvider::default();
	let client
	chat_state.new_peer(1, tx);
}