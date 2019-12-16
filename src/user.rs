use std::collections::HashSet;
use std::collections::VecDeque;

/// User with followers, and black list
#[derive(Debug, Default)]
pub struct User {
    pub followers: HashSet<u32>,
    pub blocked: HashSet<u32>,
    pub await_messages: VecDeque<String>,
}

impl User {
    pub fn is_not_blocked(&self, id: u32) -> bool {
        !self.blocked.contains(&id)
    }
}
