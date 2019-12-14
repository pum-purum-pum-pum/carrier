use std::collections::{HashMap, HashSet};

use log::info;

use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::codec::{Framed, LinesCodec};

use futures::StreamExt;

use failure::{bail, Error};

use crate::user::User;
use crate::State;

pub type Tx = mpsc::UnboundedSender<String>;

pub type Rx = mpsc::UnboundedReceiver<String>;

/// Client socket and messages receiver
pub struct Peer {
    pub lines: Framed<TcpStream, LinesCodec>,
    pub rx: Rx,
}

impl Peer {
    pub async fn new(state: State, stream: TcpStream) -> Result<Self, Error> {
        let mut lines = Framed::new(stream, LinesCodec::new());
        let id = if let Some(Ok(line)) = lines.next().await {
            u32::from_str_radix(&line, 10)?
        } else {
            bail!("Client have not send it's id");
        };
        let (tx, rx) = mpsc::unbounded_channel();
        state.lock().await.peers.insert(id, tx);
        Ok(Self { lines, rx })
    }
}

/// Struct which contains information about connected users and allows to execute events on them
#[derive(Debug, Default)]
pub struct ChatState {
    peers: HashMap<u32, Tx>,
    users: HashMap<u32, User>,
}

impl ChatState {
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

    pub fn new_peer(&mut self, id: u32, tx: Tx) {
        self.peers.insert(id, tx);
        self.users.insert(id, User::default());
    }

    /// Actor should now be following Target. Target is expected to receive this event.
    pub fn follow(&mut self, from: u32, to: u32, message: &str) -> Result<(), Error> {
        if let Some(user) = self.users.get_mut(&to) {
            if user.is_not_blocked(from) {
                user.followers.insert(from);
                let target_peer = self.peers.get(&to).unwrap();
                target_peer.send(message.into())?;
                info!("follow message send {:?}", &message);
            }
        };
        Ok(())
    }

    /// Target should be removed from Actor's followers. No one is expected to receive this event.
    pub fn unfollow(&mut self, from: u32, to: u32) {
        if let Some(user) = self.users.get_mut(&to) {
            if user.is_not_blocked(from) {
                user.followers.remove(&from);
            }
        }
    }

    /// All followers of Actor are expected to receive this event.
    pub async fn status_update(&mut self, from: u32, message: &str) -> Result<(), Error> {
        if let Some(user) = self.users.get(&from) {
            let recipients = &user.followers - &user.blocked;
            for recipient in recipients.iter() {
                if let Some(peer) = self.peers.get(recipient) {
                    peer.send(message.into())?;
                }
            }
        }
        Ok(())
    }

    /// Target expects to receive this event.
    pub async fn private_message(
        &mut self,
        from: u32,
        to: u32,
        message: &str,
    ) -> Result<(), Error> {
        if let Some(user) = self.users.get(&to) {
            if user.is_not_blocked(from) {
                let target_peer = self.peers.get(&to).unwrap();
                target_peer.send(message.into())?;
            }
        }
        Ok(())
    }

    /// Prevents Actor from receiving a Follow, Status Update, or Private Message from Target.
    pub async fn block(&mut self, from: u32, to: u32) {
        if let Some(blocked) = self.mut_blocked(from) {
            blocked.insert(to);
        }
    }
}
