use serde_json::Value;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
pub mod msg;

#[derive(Eq, PartialEq, Debug, Clone)]
pub struct Job {
    pub id: u32,
    pub prio: u32,
    pub queue: String,
    pub job: Value,
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
    pub fn new(id: u32, prio: u32, queue: &str, job: Value) -> Self {
        Self {
            id,
            prio,
            queue: queue.to_string(),
            job,
        }
    }
}

#[derive(Clone, PartialEq, Copy)]
pub enum JobState {
    Unassigned,
    Assigned(u32),
    Deleted,
}

#[derive(Default)]
pub struct Queue {
    heap: BinaryHeap<Job>,
    pub waiting_clients: HashMap<u32, Vec<String>>,
}

impl Queue {
    pub fn push(&mut self, job: Job) {
        self.heap.push(job)
    }

    pub fn peek(&mut self, deleted: &HashMap<u32, JobState>) -> Option<Job> {
        self.skip_deleted(deleted);
        self.heap.peek().cloned()
    }

    pub fn pop(&mut self, deleted: &HashMap<u32, JobState>) -> Option<Job> {
        self.skip_deleted(deleted);
        self.heap.pop()
    }

    fn skip_deleted(&mut self, deleted: &HashMap<u32, JobState>) {
        while let Some(Job { id, .. }) = self.heap.peek() {
            if deleted.get(id).cloned() == Some(JobState::Deleted) {
                self.heap.pop();
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::jobcentre::{Job, JobState, Queue};
    use serde_json::{json, Value};
    use std::collections::{HashMap, HashSet};

    #[test]
    fn peek() {
        let mut queue = Queue::default();

        queue.push(Job::new(1, 10, "foo", json!("foo")));
        queue.push(Job::new(2, 100, "foo", json!("foo")));
        queue.push(Job::new(3, 200, "foo", json!("foo")));
        queue.push(Job::new(4, 300, "foo", json!("foo")));

        let mut deleted = HashMap::new();
        deleted.insert(3, JobState::Deleted);
        deleted.insert(4, JobState::Deleted);

        assert!(queue.peek(&deleted).is_some_and(|j| j.id == 2));
    }

    #[test]
    fn pop() {
        let mut queue = Queue::default();

        queue.push(Job::new(1, 10, "foo", json!("foo")));
        queue.push(Job::new(2, 100, "foo", json!("foo")));
        queue.push(Job::new(3, 200, "foo", json!("foo")));
        queue.push(Job::new(4, 300, "foo", json!("foo")));

        let mut deleted = HashMap::new();
        deleted.insert(3, JobState::Deleted);
        deleted.insert(4, JobState::Deleted);

        assert!(queue.pop(&deleted).is_some_and(|j| j.id == 2));
        assert!(queue.pop(&deleted).is_some_and(|j| j.id == 1));
        assert_eq!(queue.pop(&deleted), None);
    }
}
