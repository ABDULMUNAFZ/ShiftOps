-- Devin task records: a content hash of the generated instructions and the
-- moment a human approved the task for dispatch. Dispatch is only allowed
-- after `approved_at` is set; the hash pins exactly what was approved.
ALTER TABLE agent_tasks ADD COLUMN instructions_hash TEXT;
ALTER TABLE agent_tasks ADD COLUMN approved_at TEXT;

-- Campaign members flagged by planning as needing human review must be
-- explicitly cleared before any Devin task can be dispatched for them.
ALTER TABLE campaign_repositories ADD COLUMN review_required INTEGER NOT NULL DEFAULT 0;
