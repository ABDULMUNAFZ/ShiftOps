//! Persistent activity and audit events. Events summarise state changes;
//! command output lines are never stored as individual events.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    json_col, json_text, new_id, paginate, string_enum, Filter, JsonShape, ListParams, Page,
};
use crate::error::AppResult;
use crate::redact::redact;
use crate::time_util;

string_enum!(Severity {
    Info => "info",
    Notice => "notice",
    Warning => "warning",
    Error => "error",
});

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEvent {
    pub id: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub event_type: String,
    pub severity: Severity,
    pub summary: String,
    pub metadata: Value,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewEvent {
    pub entity_type: &'static str,
    pub entity_id: Option<String>,
    pub event_type: &'static str,
    pub severity: Severity,
    pub summary: String,
    pub metadata: Value,
}

impl NewEvent {
    pub fn new(
        entity_type: &'static str,
        event_type: &'static str,
        severity: Severity,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            entity_type,
            entity_id: None,
            event_type,
            severity,
            summary: summary.into(),
            metadata: Value::Object(Default::default()),
        }
    }

    pub fn entity(mut self, id: impl Into<String>) -> Self {
        self.entity_id = Some(id.into());
        self
    }

    pub fn metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }
}

/// Recursively redact string values inside metadata.
fn redact_value(value: Value) -> Value {
    match value {
        Value::String(s) => Value::String(redact(&s)),
        Value::Array(items) => Value::Array(items.into_iter().map(redact_value).collect()),
        Value::Object(map) => {
            Value::Object(map.into_iter().map(|(k, v)| (k, redact_value(v))).collect())
        }
        other => other,
    }
}

pub fn record(conn: &Connection, event: NewEvent) -> AppResult<ActivityEvent> {
    let metadata = redact_value(event.metadata);
    let stored = ActivityEvent {
        id: new_id(),
        entity_type: event.entity_type.to_string(),
        entity_id: event.entity_id,
        event_type: event.event_type.to_string(),
        severity: event.severity,
        summary: redact(&event.summary),
        metadata: metadata.clone(),
        created_at: time_util::now(),
    };
    conn.execute(
        "INSERT INTO activity_events (id, entity_type, entity_id, event_type, severity, summary, metadata_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            stored.id,
            stored.entity_type,
            stored.entity_id,
            stored.event_type,
            stored.severity,
            stored.summary,
            json_text(&metadata, "metadata", JsonShape::Object)?,
            stored.created_at,
        ],
    )?;
    Ok(stored)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ActivityQuery {
    #[serde(flatten)]
    pub page: ListParams,
    pub entity_type: Option<String>,
    pub entity_id: Option<String>,
}

const COLUMNS: &str =
    "SELECT id, entity_type, entity_id, event_type, severity, summary, metadata_json, created_at";

pub fn list(conn: &Connection, query: &ActivityQuery) -> AppResult<Page<ActivityEvent>> {
    let severities = query.page.statuses_as(Severity::parse)?;
    let severity_strs: Vec<&str> = severities.iter().map(|s| s.as_str()).collect();
    let mut filter = Filter::default();
    filter.any_of("severity", &severity_strs);
    if let Some(t) = query.entity_type.as_deref().filter(|t| !t.is_empty()) {
        filter.eq("entity_type", t.to_string());
    }
    if let Some(id) = query.entity_id.as_deref().filter(|t| !t.is_empty()) {
        filter.eq("entity_id", id.to_string());
    }
    filter.search(&["summary", "event_type"], query.page.search_pattern());
    paginate(
        conn,
        COLUMNS,
        "activity_events",
        &filter,
        "created_at DESC, rowid DESC",
        &query.page,
        |r| {
            Ok(ActivityEvent {
                id: r.get("id")?,
                entity_type: r.get("entity_type")?,
                entity_id: r.get("entity_id")?,
                event_type: r.get("event_type")?,
                severity: r.get("severity")?,
                summary: r.get("summary")?,
                metadata: json_col(r, "metadata_json")?,
                created_at: r.get("created_at")?,
            })
        },
    )
}

pub fn count(conn: &Connection) -> AppResult<u64> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))?;
    Ok(u64::try_from(n).unwrap_or(0))
}

pub fn entity_types(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT DISTINCT entity_type FROM activity_events ORDER BY entity_type")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use serde_json::json;

    #[test]
    fn records_redacted_events_and_lists_newest_first_with_pagination() {
        let db = Database::open_in_memory();
        for i in 0..7 {
            db.write(|c| {
                record(
                    c,
                    NewEvent::new(
                        "process",
                        "process.completed",
                        if i % 2 == 0 {
                            Severity::Info
                        } else {
                            Severity::Error
                        },
                        format!("run {i}"),
                    )
                    .entity(format!("run-{i}"))
                    .metadata(json!({ "i": i, "header": "Authorization: Bearer abcdefgh12345" })),
                )
            })
            .unwrap();
        }
        let first = db
            .read(|c| {
                list(
                    c,
                    &ActivityQuery {
                        page: ListParams {
                            limit: Some(3),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(first.total, 7);
        assert_eq!(
            first
                .items
                .iter()
                .map(|e| e.summary.as_str())
                .collect::<Vec<_>>(),
            ["run 6", "run 5", "run 4"]
        );
        assert!(!first.items[0]
            .metadata
            .to_string()
            .contains("abcdefgh12345"));

        let last = db
            .read(|c| {
                list(
                    c,
                    &ActivityQuery {
                        page: ListParams {
                            limit: Some(3),
                            offset: Some(6),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(last.items.len(), 1);
        assert_eq!(last.items[0].summary, "run 0");

        let errors = db
            .read(|c| {
                list(
                    c,
                    &ActivityQuery {
                        page: ListParams {
                            statuses: vec!["error".into()],
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(errors.total, 3);

        let searched = db
            .read(|c| {
                list(
                    c,
                    &ActivityQuery {
                        page: ListParams {
                            search: Some("run 3".into()),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(searched.total, 1);

        let by_entity = db
            .read(|c| {
                list(
                    c,
                    &ActivityQuery {
                        entity_id: Some("run-2".into()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(by_entity.total, 1);

        assert!(db
            .read(|c| list(
                c,
                &ActivityQuery {
                    page: ListParams {
                        statuses: vec!["bogus".into()],
                        ..Default::default()
                    },
                    ..Default::default()
                }
            ))
            .is_err());
        assert_eq!(db.read(count).unwrap(), 7);
    }

    #[test]
    fn search_escapes_like_wildcards() {
        let db = Database::open_in_memory();
        db.write(|c| record(c, NewEvent::new("app", "x", Severity::Info, "100% done")))
            .unwrap();
        db.write(|c| record(c, NewEvent::new("app", "x", Severity::Info, "1000 done")))
            .unwrap();
        let page = db
            .read(|c| {
                list(
                    c,
                    &ActivityQuery {
                        page: ListParams {
                            search: Some("100%".into()),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(page.total, 1);
    }
}
