-- Runner credentials and transport state are separate from resource ownership.
CREATE TABLE bridge_runners (
    id BLOB PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    token_hash TEXT NOT NULL,
    installation_id BLOB UNIQUE,
    execution_resource_id BLOB NOT NULL UNIQUE REFERENCES shared_resources(id),
    capabilities TEXT NOT NULL DEFAULT '{}',
    last_seen TEXT,
    disabled INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE bridge_sources (
    id BLOB PRIMARY KEY NOT NULL,
    workspace_id BLOB NOT NULL,
    digest TEXT NOT NULL,
    manifest TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now','subsec'))
);
CREATE TABLE bridge_commands (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    id BLOB NOT NULL UNIQUE,
    operation_id BLOB NOT NULL REFERENCES resource_operations(id),
    kind TEXT NOT NULL CHECK(kind IN ('start','step','finish')),
    script TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','sent','done')),
    result TEXT,
    UNIQUE(operation_id,id)
);
CREATE INDEX bridge_commands_operation ON bridge_commands(operation_id,sequence);
CREATE TABLE bridge_settlements (
    operation_id BLOB PRIMARY KEY NOT NULL REFERENCES resource_operations(id),
    runtime_id BLOB NOT NULL,
    evidence TEXT NOT NULL
);
