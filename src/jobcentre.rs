use std::cmp::Ordering;
use std::collections::BinaryHeap;

mod msg;

#[derive(Eq, Ord, PartialEq, Debug)]
pub struct Job {
    id: u32,
    prio: u32,
    queue: String,
    worked_by: Option<u32>,
    deleted: bool,
}

impl PartialOrd for Job {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.prio.partial_cmp(&other.prio)
    }
}

impl Job {
    pub fn new(id: u32, prio: u32, queue: &str) -> Self {
        Self {
            id,
            prio,
            queue: queue.to_string(),
            deleted: false,
            worked_by: None,
        }
    }

    pub fn delete(&mut self) {
        self.deleted = true;
    }

    pub fn worked_by(&mut self, client_id: u32) {
        self.worked_by = Some(client_id)
    }

    pub fn clear_worked_by(&mut self) {
        self.worked_by = None
    }
}

#[derive(Default)]
pub struct Queue {
    heap: BinaryHeap<Job>,
}

impl Queue {
    pub fn peek(&mut self) -> Option<&Job> {
        self.skip_deleted();
        self.heap.peek()
    }

    pub fn pop(&mut self) -> Option<Job> {
        self.skip_deleted();
        self.heap.pop()
    }

    fn skip_deleted(&mut self) {
        while let Some(Job { deleted: true, .. }) = self.heap.peek() {
            self.heap.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::jobcentre::{Job, Queue};

    #[test]
    fn peek_with_no_deleted_jobs() {
        let mut queue = Queue::default();

        queue.heap.push(Job::new(1, 10, "foo"));
        queue.heap.push(Job::new(2, 100, "foo"));

        assert!(queue.peek().is_some_and(|j| j.id == 2));
    }

    #[test]
    fn pop_with_no_deleted_jobs() {
        let mut queue = Queue::default();

        queue.heap.push(Job::new(1, 10, "foo"));
        queue.heap.push(Job::new(2, 100, "foo"));

        assert!(queue.pop().is_some_and(|j| j.id == 2));
        assert!(queue.pop().is_some_and(|j| j.id == 1));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn peek_with_deleted_jobs() {
        let mut queue = Queue::default();

        queue.heap.push(Job::new(1, 10, "foo"));
        queue.heap.push(Job::new(2, 100, "foo"));
        let mut job = Job::new(3, 200, "foo");
        job.delete();
        queue.heap.push(job);
        let mut job = Job::new(4, 300, "foo");
        job.delete();
        queue.heap.push(job);

        assert!(queue.peek().is_some_and(|j| j.id == 2));
    }

    #[test]
    fn pop_with_deleted_jobs() {
        let mut queue = Queue::default();

        queue.heap.push(Job::new(1, 10, "foo"));
        queue.heap.push(Job::new(2, 100, "foo"));
        let mut job = Job::new(3, 200, "foo");
        job.delete();
        queue.heap.push(job);
        let mut job = Job::new(4, 300, "foo");
        job.delete();
        queue.heap.push(job);

        assert!(queue.pop().is_some_and(|j| j.id == 2));
        assert!(queue.pop().is_some_and(|j| j.id == 1));
        assert_eq!(queue.pop(), None);
    }
}
