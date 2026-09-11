-- Durable evidence for a single host observation that could not be reduced
-- into the canonical projection. Existing events are intentionally untouched.
CREATE TABLE agent_projection_failures (
    id                  BLOB PRIMARY KEY,
    agent_run_id        BLOB NOT NULL,
    run_attempt_id      BLOB NOT NULL,
    host_event_sequence INTEGER NOT NULL CHECK (host_event_sequence > 0),
    error_kind          TEXT NOT NULL CHECK (length(trim(error_kind)) > 0),
    reason              TEXT NOT NULL CHECK (length(trim(reason)) > 0),
    created_at          TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    updated_at          TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (agent_run_id) REFERENCES agent_runs(id) ON DELETE CASCADE,
    FOREIGN KEY (run_attempt_id) REFERENCES agent_run_attempts(id) ON DELETE CASCADE,
    UNIQUE (run_attempt_id, host_event_sequence)
);

CREATE INDEX idx_agent_projection_failures_run
    ON agent_projection_failures(agent_run_id, created_at);

-- Cumulative provider usage is latest-value state, not event history. One row
-- per attempt lets run-level stats add retries without summing repeated
-- cumulative notifications from the same attempt.
CREATE TABLE agent_run_usage_snapshots (
    run_attempt_id      BLOB PRIMARY KEY,
    agent_run_id        BLOB NOT NULL,
    session_id          BLOB NOT NULL,
    run_attempt_number  INTEGER NOT NULL CHECK (run_attempt_number > 0),
    native_sequence     INTEGER NOT NULL CHECK (native_sequence > 0),
    input_tokens        INTEGER NOT NULL CHECK (input_tokens >= 0),
    output_tokens       INTEGER NOT NULL CHECK (output_tokens >= 0),
    cached_input_tokens INTEGER CHECK (cached_input_tokens >= 0),
    event_json          TEXT NOT NULL CHECK (json_valid(event_json)),
    updated_at          TEXT NOT NULL,
    FOREIGN KEY (agent_run_id) REFERENCES agent_runs(id) ON DELETE CASCADE,
    FOREIGN KEY (run_attempt_id) REFERENCES agent_run_attempts(id) ON DELETE CASCADE
);

CREATE INDEX idx_agent_run_usage_snapshots_run
    ON agent_run_usage_snapshots(agent_run_id, run_attempt_number);
