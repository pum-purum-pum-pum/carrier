use std::collections::{HashMap, HashSet};

use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::codec::{Framed, LinesCodec};

use futures::StreamExt;

use failure::{bail, Error};

use crate::user::User;

pub type Tx = mpsc::UnboundedSender<String>;

pub type Rx = mpsc::UnboundedReceiver<String>;

/// Client socket and messages receiver
pub struct Peer {
    pub id: u32,
    pub lines: Framed<TcpStream, LinesCodec>,
    pub rx: Rx,
}

/// Users state and connections
#[derive(Debug, Default)]
pub struct ServerState {
    pub peers: HashMap<u32, Tx>,
    pub users: HashMap<u32, User>,
}

impl ServerState {
    /// create new peer. Also send all awaiting messages to client peer
    pub async fn new_peer(&mut self, stream: TcpStream) -> Result<Peer, Error> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut lines = Framed::new(stream, LinesCodec::new());
        let id = if let Some(Ok(line)) = lines.next().await {
            u32::from_str_radix(&line, 10)?
        } else {
            bail!("Client have not send it's id");
        };
        self.peers.insert(id, tx);
        self.send_await(id)?;
        Ok(Peer { id, lines, rx })
    }

    /// send all events that occured before the user connected 
    pub fn send_await(&mut self, id: u32) -> Result<(), Error> {
        if let Some(user) = self.users.get_mut(&id) {
            while let Some(msg) = user.await_messages.pop_front() {
                if let Some(target_peer) = self.peers.get(&id) {
                    target_peer.send(msg)?;
                }
            }
        }
        Ok(())
    }

    pub fn generate_users(&mut self, users_num: u32) {
        for i in 1..=users_num {
            self.users.insert(i, User::default());
        }
    }

    pub fn contains(&self, user: u32) -> bool {
        self.peers.contains_key(&user)
    }

    pub fn is_not_blocked(&self, from: u32, to: u32) -> bool {
        self.blocked(to)
            .map(|list| !list.contains(&from))
            .unwrap_or(true)
    }

    pub fn followers(&self, user: u32) -> Option<&HashSet<u32>> {
        self.users.get(&user).map(|u| &u.followers)
    }

    pub fn mut_followers(&mut self, user: u32) -> Option<&mut HashSet<u32>> {
        self.users.get_mut(&user).map(|u| &mut u.followers)
    }

    pub fn blocked(&self, user: u32) -> Option<&HashSet<u32>> {
        self.users.get(&user).map(|u| &u.blocked)
    }

    pub fn mut_blocked(&mut self, user: u32) -> Option<&mut HashSet<u32>> {
        self.users.get_mut(&user).map(|u| &mut u.blocked)
    }
}
