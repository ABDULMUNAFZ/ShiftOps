-- Task 5 and 6: independent verification plans, bounded Devin repair tasks,
-- protection against duplicate long-running operations, and user-editable
-- repository metadata.

-- A repair task is drafted from a failed verification run and carries the
-- hash of the verification plan the user approved, so ShiftOps re-runs those
-- exact checks after the repair only if the plan is unchanged.
ALTER TABLE agent_tasks ADD COLUMN task_kind TEXT NOT NULL DEFAULT 'migration'
    CHECK (task_kind IN ('migration', 'repair'));
ALTER TABLE agent_tasks ADD COLUMN source_verification_run_id TEXT
    REFERENCES verification_runs (id) ON DELETE SET NULL;
ALTER TABLE agent_tasks ADD COLUMN verification_plan_hash TEXT;

-- The hash of the plan a run executed, and what started it.
ALTER TABLE verification_runs ADD COLUMN plan_hash TEXT;
ALTER TABLE verification_runs ADD COLUMN trigger TEXT NOT NULL DEFAULT 'manual'
    CHECK (trigger IN ('manual', 'after_repair'));

-- At most one live Devin session and one live verification per campaign member.
CREATE UNIQUE INDEX agent_tasks_one_live_idx ON agent_tasks (campaign_repository_id)
    WHERE status IN ('queued', 'running');
CREATE UNIQUE INDEX verification_runs_one_live_idx ON verification_runs (campaign_repository_id)
    WHERE status IN ('queued', 'running');

-- User-edited display name and notes. Discovery never overwrites them.
ALTER TABLE repositories ADD COLUMN display_name TEXT
    CHECK (display_name IS NULL OR length(display_name) BETWEEN 1 AND 255);
ALTER TABLE repositories ADD COLUMN notes TEXT
    CHECK (notes IS NULL OR length(notes) <= 4000);
