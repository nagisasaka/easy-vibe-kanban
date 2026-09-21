-- Additive optimistic concurrency for authoring. Existing graphs/runs are retained.
ALTER TABLE workflows ADD COLUMN revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0);
