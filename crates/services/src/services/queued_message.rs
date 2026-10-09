use std::{collections::VecDeque, sync::Arc};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use db::models::scratch::DraftFollowUpData;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, OwnedMutexGuard};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct QueuedMessage {
    pub id: Uuid,
    pub session_id: Uuid,
    pub data: DraftFollowUpData,
    pub queued_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum QueueStatus {
    Empty,
    Queued {
        // Keep the head for older clients and maintenance tooling.
        message: Box<QueuedMessage>,
        messages: Vec<QueuedMessage>,
        paused: bool,
    },
}

#[derive(Default)]
struct SessionQueue {
    messages: VecDeque<QueuedMessage>,
    paused: bool,
}

/// Session-local FIFO. Drafts are independent of submitted messages.
/// Like the previous queue, this is memory-only and does not survive a restart.
#[derive(Clone, Default)]
pub struct QueuedMessageService {
    queue: Arc<DashMap<Uuid, SessionQueue>>,
    locks: Arc<DashMap<Uuid, Arc<Mutex<()>>>>,
}

impl QueuedMessageService {
    pub fn new() -> Self {
        Self::default()
    }

    /// Serialize admission, editing and dispatch across await points.
    pub async fn lock_session(&self, session_id: Uuid) -> OwnedMutexGuard<()> {
        let lock = self.locks.entry(session_id).or_default().clone();
        lock.lock_owned().await
    }

    pub fn queue_message(&self, session_id: Uuid, data: DraftFollowUpData) -> QueuedMessage {
        let queued = QueuedMessage {
            id: Uuid::new_v4(),
            session_id,
            data,
            queued_at: Utc::now(),
        };
        let mut queue = self.queue.entry(session_id).or_default();
        if queue.messages.is_empty() {
            queue.paused = false;
        }
        queue.messages.push_back(queued.clone());
        queued
    }

    pub fn cancel_queued(&self, session_id: Uuid) -> Option<QueuedMessage> {
        self.queue
            .remove(&session_id)
            .and_then(|(_, mut q)| q.messages.pop_front())
    }

    pub fn get_queued(&self, session_id: Uuid) -> Option<QueuedMessage> {
        self.queue.get(&session_id)?.messages.front().cloned()
    }

    pub fn pause(&self, session_id: Uuid) {
        if let Some(mut queue) = self.queue.get_mut(&session_id) {
            queue.paused = true;
        }
    }

    pub fn resume(&self, session_id: Uuid) {
        if let Some(mut queue) = self.queue.get_mut(&session_id) {
            queue.paused = false;
        }
    }

    pub fn take_queued(&self, session_id: Uuid) -> Option<QueuedMessage> {
        let mut queue = self.queue.get_mut(&session_id)?;
        if queue.paused {
            return None;
        }
        queue.messages.pop_front()
    }

    pub fn restore_and_pause(&self, message: QueuedMessage) {
        let mut queue = self.queue.entry(message.session_id).or_default();
        queue.messages.push_front(message);
        queue.paused = true;
    }

    pub fn edit(&self, session_id: Uuid, id: Uuid, message: String) -> bool {
        let Some(mut queue) = self.queue.get_mut(&session_id) else {
            return false;
        };
        let Some(item) = queue.messages.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        item.data.message = message;
        true
    }

    pub fn remove(&self, session_id: Uuid, id: Uuid) -> bool {
        let Some(mut queue) = self.queue.get_mut(&session_id) else {
            return false;
        };
        let Some(index) = queue.messages.iter().position(|item| item.id == id) else {
            return false;
        };
        queue.messages.remove(index);
        true
    }

    /// Require an exact permutation: stale clients cannot lose newly queued work.
    pub fn reorder(&self, session_id: Uuid, ids: &[Uuid]) -> bool {
        let Some(mut queue) = self.queue.get_mut(&session_id) else {
            return false;
        };
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        if ids.len() != queue.messages.len()
            || unique.len() != ids.len()
            || !queue.messages.iter().all(|item| unique.contains(&item.id))
        {
            return false;
        }
        queue
            .messages
            .make_contiguous()
            .sort_by_key(|item| ids.iter().position(|id| *id == item.id).unwrap());
        true
    }

    pub fn has_queued(&self, session_id: Uuid) -> bool {
        self.get_queued(session_id).is_some()
    }

    pub fn get_status(&self, session_id: Uuid) -> QueueStatus {
        let Some(queue) = self.queue.get(&session_id) else {
            return QueueStatus::Empty;
        };
        match queue.messages.front() {
            Some(message) => QueueStatus::Queued {
                message: Box::new(message.clone()),
                messages: queue.messages.iter().cloned().collect(),
                paused: queue.paused,
            },
            None => QueueStatus::Empty,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(message: &str) -> DraftFollowUpData {
        serde_json::from_value(serde_json::json!({
            "message": message, "executor_config": { "executor": "CODEX" }
        }))
        .unwrap()
    }

    #[test]
    fn fifo_isolated_sessions_and_pause_preserve_work() {
        let queue = QueuedMessageService::new();
        let session = Uuid::new_v4();
        let other = Uuid::new_v4();
        let first = queue.queue_message(session, data("first"));
        let second = queue.queue_message(session, data("second"));
        queue.queue_message(other, data("other"));
        queue.pause(session);
        assert!(queue.take_queued(session).is_none());
        assert_eq!(queue.take_queued(other).unwrap().data.message, "other");
        queue.resume(session);
        let dispatched = queue.take_queued(session).unwrap();
        assert_eq!(dispatched.id, first.id);
        // A launch failure returns the same message to the front and stops.
        queue.restore_and_pause(dispatched);
        assert!(queue.take_queued(session).is_none());
        queue.resume(session);
        assert_eq!(queue.take_queued(session).unwrap().id, first.id);
        assert_eq!(queue.take_queued(session).unwrap().id, second.id);
        assert!(!queue.has_queued(session));
    }

    #[test]
    fn edit_remove_and_reorder_reject_stale_or_duplicate_ids() {
        let queue = QueuedMessageService::new();
        let session = Uuid::new_v4();
        let a = queue.queue_message(session, data("a"));
        let b = queue.queue_message(session, data("b"));
        assert!(!queue.reorder(session, &[a.id, a.id]));
        assert!(!queue.reorder(session, &[b.id]));
        assert!(queue.reorder(session, &[b.id, a.id]));
        assert!(queue.edit(session, b.id, "edited".into()));
        assert_eq!(queue.take_queued(session).unwrap().data.message, "edited");
        assert!(!queue.edit(session, b.id, "too late".into()));
        assert!(!queue.remove(session, b.id));
        assert!(queue.remove(session, a.id));
        assert!(matches!(queue.get_status(session), QueueStatus::Empty));
    }
}
