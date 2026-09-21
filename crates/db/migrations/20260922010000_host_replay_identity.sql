-- Existing hosts retain Attach/v1 behavior; never infer v2 from a PID/path.
ALTER TABLE agent_process_registry ADD COLUMN host_protocol_version INTEGER;
ALTER TABLE agent_process_registry ADD COLUMN host_start_identity TEXT;
