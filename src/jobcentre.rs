use std::cmp::Ordering;
use std::collections::BinaryHeap;

pub mod msg;

#[derive(Eq, PartialEq, Debug, Clone)]
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

impl Ord for Job {
    fn cmp(&self, other: &Self) -> Ordering {
        self.prio.cmp(&other.prio)
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
    pub fn push(&mut self, job: Job) {
        self.heap.push(job)
    }

    pub fn peek(&mut self) -> Option<Job> {
        self.skip_deleted();
        self.heap.peek().cloned()
    }

    pub fn pop(&mut self) -> Option<Job> {
        self.skip_deleted();
        self.heap.pop()
    }

    pub fn pop_multi(queues: &mut [Self]) -> Option<Job> {
        queues.iter_mut().for_each(Queue::skip_deleted);

        queues
            .iter_mut()
            .filter(|q| q.heap.peek().is_some())
            .max_by(|q1, q2| q1.heap.peek().unwrap().cmp(q2.heap.peek().unwrap()))
            .and_then(|q| q.heap.pop())
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

        queue.push(Job::new(1, 10, "foo"));
        queue.push(Job::new(2, 100, "foo"));

        assert!(queue.peek().is_some_and(|j| j.id == 2));
    }

    #[test]
    fn pop_with_no_deleted_jobs() {
        let mut queue = Queue::default();

        queue.push(Job::new(1, 10, "foo"));
        queue.push(Job::new(2, 100, "foo"));

        assert!(queue.pop().is_some_and(|j| j.id == 2));
        assert!(queue.pop().is_some_and(|j| j.id == 1));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn peek_with_deleted_jobs() {
        let mut queue = Queue::default();

        queue.push(Job::new(1, 10, "foo"));
        queue.push(Job::new(2, 100, "foo"));
        let mut job = Job::new(3, 200, "foo");
        job.delete();
        queue.push(job);
        let mut job = Job::new(4, 300, "foo");
        job.delete();
        queue.push(job);

        assert!(queue.peek().is_some_and(|j| j.id == 2));
    }

    #[test]
    fn pop_with_deleted_jobs() {
        let mut queue = Queue::default();

        queue.push(Job::new(1, 10, "foo"));
        queue.push(Job::new(2, 100, "foo"));
        let mut job = Job::new(3, 200, "foo");
        job.delete();
        queue.push(job);
        let mut job = Job::new(4, 300, "foo");
        job.delete();
        queue.push(job);

        assert!(queue.pop().is_some_and(|j| j.id == 2));
        assert!(queue.pop().is_some_and(|j| j.id == 1));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn pop_multi() {
        let mut queue1 = Queue::default();

        queue1.push(Job::new(1, 10, "foo"));
        queue1.push(Job::new(2, 100, "foo"));
        let mut job = Job::new(3, 200, "foo");
        job.delete();
        queue1.push(job);
        let mut job = Job::new(4, 300, "foo");
        job.delete();
        queue1.push(job);

        let mut queue2 = Queue::default();

        queue2.push(Job::new(5, 10, "foo"));
        queue2.push(Job::new(6, 101, "foo"));
        let mut job = Job::new(7, 200, "foo");
        job.delete();
        queue2.push(job);
        let mut job = Job::new(8, 300, "foo");
        job.delete();
        queue2.push(job);

        assert!(Queue::pop_multi(&mut [queue1, queue2]).is_some_and(|j| j.id == 6));
    }
}
