use std::collections::HashMap;

// TODO timeout for missing events

/// Queue which returns only _sequenced events_
/// If event is missing it will wait forever
#[derive(Debug)]
pub struct SequencedQueue<T> {
    pub current: u32,
    queue: HashMap<u32, T>,
    total_items: u32,
}

impl<T> SequencedQueue<T> {
    pub fn new(total_items: u32) -> Self {
        Self {
            current: 1,
            queue: HashMap::new(),
            total_items,
        }
    }

    /// Have we iterated over all _total_items_ events
    pub fn finished(&self) -> bool {
        self.current > self.total_items
    }

    /// Insert event into queue (not necessarily a sequential event)
    pub fn insert(&mut self, id: u32, value: T) {
        self.queue.insert(id, value);
    }

    /// If there is next sequenced event then return it
    pub fn next(&mut self) -> Option<T> {
        if self.current > self.total_items {
            return None;
        }
        let res = self.queue.remove(&self.current);
        if res.is_some() {
            self.current += 1
        }
        res
    }
}
