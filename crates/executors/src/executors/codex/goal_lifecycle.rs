//! A provider snapshot is not evidence that this run started a Goal.
//! Activation is fenced by its RPC response, processed on the ordered reader
//! before waking the caller. Never arm completion from replayed notifications.
use codex_app_server_protocol::RequestId;

#[derive(Debug, Default)]
pub(super) enum GoalLifecycle {
    #[default]
    Observing,
    Activating {
        request_id: RequestId,
        thread_id: String,
    },
    Managed {
        thread_id: String,
        keep_alive: bool,
    },
    Failed,
}

impl GoalLifecycle {
    pub fn is_observing(&self) -> bool {
        matches!(self, Self::Observing)
    }
    pub fn begin(&mut self, request_id: RequestId, thread_id: String) {
        *self = Self::Activating {
            request_id,
            thread_id,
        };
    }

    pub fn awaiting(&self, id: &RequestId) -> bool {
        matches!(self, Self::Activating { request_id, .. } if request_id == id)
    }

    pub fn acknowledge(
        &mut self,
        id: &RequestId,
        thread: &str,
        status: &str,
    ) -> Result<(), &'static str> {
        if let Self::Activating {
            request_id,
            thread_id,
        } = self
        {
            if request_id != id {
                return Ok(());
            }
            if thread_id != thread || status != "active" {
                *self = Self::Failed;
                return Err("Goal activation response does not confirm the requested active Goal");
            }
            *self = Self::Managed {
                thread_id: thread.to_owned(),
                keep_alive: true,
            };
        }
        Ok(())
    }

    pub fn fail(&mut self, id: &RequestId) {
        if self.awaiting(id) {
            *self = Self::Failed;
        }
    }

    pub fn keep_alive(&self) -> bool {
        matches!(
            self,
            Self::Activating { .. }
                | Self::Managed {
                    keep_alive: true,
                    ..
                }
        )
    }

    /// Returns true only for a terminal transition of this run's managed Goal.
    pub fn observe(&mut self, thread: &str, status: &str) -> bool {
        let Self::Managed {
            thread_id,
            keep_alive,
        } = self
        else {
            return false;
        };
        if thread_id != thread {
            return false;
        }
        match status {
            "active" | "paused" | "blocked" | "usageLimited" | "usage_limited"
            | "budgetLimited" | "budget_limited" => {
                *keep_alive = true;
                false
            }
            "complete" | "cleared" => {
                let was_alive = *keep_alive;
                *keep_alive = false;
                was_alive
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replay_cannot_finish_activation_and_response_arms_before_caller_wakes() {
        let mut state = GoalLifecycle::default();
        assert!(!state.observe("parent", "active"));
        state.begin(RequestId::Integer(4), "parent".into());
        for status in ["cleared", "complete", "active"] {
            assert!(!state.observe("parent", status));
        }
        assert!(state.keep_alive());
        state
            .acknowledge(&RequestId::Integer(3), "parent", "active")
            .unwrap();
        assert!(state.awaiting(&RequestId::Integer(4)));
        state
            .acknowledge(&RequestId::Integer(4), "parent", "active")
            .unwrap();
        assert!(!state.observe("child", "complete"));
        assert!(state.observe("parent", "complete"));
        assert!(!state.observe("parent", "complete"));
    }
    #[test]
    fn failed_or_wrong_thread_activation_never_becomes_success() {
        let mut state = GoalLifecycle::default();
        state.begin(RequestId::Integer(1), "parent".into());
        state.fail(&RequestId::Integer(1));
        assert!(!state.observe("parent", "complete"));
        state.begin(RequestId::Integer(2), "parent".into());
        assert!(
            state
                .acknowledge(&RequestId::Integer(2), "child", "active")
                .is_err()
        );
        assert!(!state.keep_alive());
    }
}
