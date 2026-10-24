-- Repository intelligence: evidence-backed facts and explicit conflicts per
-- scan, plus the cache key that lets an unchanged repository skip a rescan.

ALTER TABLE repository_scans ADD COLUMN scanner_version TEXT;
ALTER TABLE repository_scans ADD COLUMN cache_key TEXT;
CREATE INDEX repository_scans_cache_idx ON repository_scans (repository_id, cache_key);

CREATE TABLE repository_facts (
    id TEXT PRIMARY KEY NOT NULL,
    repository_id TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    scan_id TEXT NOT NULL REFERENCES repository_scans (id) ON DELETE CASCADE,
    category TEXT NOT NULL CHECK (category IN ('language', 'framework', 'runtime', 'package_manager', 'workspace',
        'manifest', 'lockfile', 'docker', 'ci', 'script', 'dependency', 'metadata')),
    fact_key TEXT NOT NULL,
    value TEXT NOT NULL,
    evidence_file TEXT,
    evidence_line INTEGER CHECK (evidence_line IS NULL OR evidence_line >= 1),
    evidence_pointer TEXT,
    detection_method TEXT NOT NULL,
    confidence REAL NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
    created_at TEXT NOT NULL
);
CREATE INDEX repository_facts_scan_idx ON repository_facts (scan_id, category, fact_key);
CREATE INDEX repository_facts_repository_idx ON repository_facts (repository_id, category, fact_key);

CREATE TABLE repository_conflicts (
    id TEXT PRIMARY KEY NOT NULL,
    repository_id TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    scan_id TEXT NOT NULL REFERENCES repository_scans (id) ON DELETE CASCADE,
    subject TEXT NOT NULL,
    message TEXT NOT NULL,
    values_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL
);
CREATE INDEX repository_conflicts_scan_idx ON repository_conflicts (scan_id);
