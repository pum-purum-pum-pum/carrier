use std::collections::HashMap;

/// Queue which returns only sequenced events
/// If event is missing next will return None forever
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
}

impl<T> Iterator for SequencedQueue<T> {
    type Item = (u32, T);

    /// If there is next sequenced event then return it
    fn next(&mut self) -> Option<Self::Item> {
        if self.current > self.total_items {
            return None;
        }
        let res = self
            .queue
            .remove(&self.current)
            .map(|event| (self.current, event));
        if res.is_some() {
            self.current += 1
        }
        res
    }
}
