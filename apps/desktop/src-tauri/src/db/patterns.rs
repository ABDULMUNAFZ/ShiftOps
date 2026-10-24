//! Migration patterns: reusable knowledge learnt from earlier campaigns.

// Service operations beyond the read paths are used by later steps
// (discovery, planning, delegation, verification) and are covered by
// this module's tests; only test builds exercise them today.
#![cfg_attr(not(test), allow(dead_code))]

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    expect_changed, json_col, json_text, new_id, paginate, require_text, require_unit_interval,
    string_enum, Filter, JsonShape, ListParams, Page,
};
use crate::error::{AppError, AppResult};
use crate::time_util;

string_enum!(ApprovalStatus {
    Candidate => "candidate",
    Approved => "approved",
    Rejected => "rejected",
    Disabled => "disabled",
});

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPattern {
    pub id: String,
    pub name: String,
    pub applicability: Value,
    pub problem_description: String,
    pub solution_markdown: String,
    pub verification_rules: Value,
    pub confidence: f64,
    pub approval_status: ApprovalStatus,
    pub source_campaign_id: Option<String>,
    pub source_repository_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternDefinition {
    pub name: String,
    pub applicability: Value,
    pub problem_description: String,
    pub solution_markdown: String,
    pub verification_rules: Value,
    pub confidence: f64,
    pub source_campaign_id: Option<String>,
    pub source_repository_id: Option<String>,
}

impl PatternDefinition {
    fn validate(&self) -> AppResult<(String, String)> {
        require_text(&self.name, "Pattern name", 200)?;
        require_text(&self.problem_description, "Problem description", 20_000)?;
        require_text(&self.solution_markdown, "Solution", 200_000)?;
        require_unit_interval(self.confidence, "Confidence")?;
        Ok((
            json_text(&self.applicability, "applicability", JsonShape::Object)?,
            json_text(
                &self.verification_rules,
                "verificationRules",
                JsonShape::Array,
            )?,
        ))
    }
}

const COLUMNS: &str = "SELECT id, name, applicability_json, problem_description, solution_markdown, \
    verification_rules_json, confidence, approval_status, source_campaign_id, source_repository_id, created_at, updated_at";

fn map(r: &rusqlite::Row<'_>) -> rusqlite::Result<MigrationPattern> {
    Ok(MigrationPattern {
        id: r.get("id")?,
        name: r.get("name")?,
        applicability: json_col(r, "applicability_json")?,
        problem_description: r.get("problem_description")?,
        solution_markdown: r.get("solution_markdown")?,
        verification_rules: json_col(r, "verification_rules_json")?,
        confidence: r.get("confidence")?,
        approval_status: r.get("approval_status")?,
        source_campaign_id: r.get("source_campaign_id")?,
        source_repository_id: r.get("source_repository_id")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

pub fn create(conn: &Connection, def: &PatternDefinition) -> AppResult<MigrationPattern> {
    let (applicability, rules) = def.validate()?;
    let id = new_id();
    conn.execute(
        "INSERT INTO migration_patterns (id, name, applicability_json, problem_description, solution_markdown,
             verification_rules_json, confidence, approval_status, source_campaign_id, source_repository_id,
             created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'candidate', ?8, ?9, ?10, ?10)",
        params![
            id,
            def.name.trim(),
            applicability,
            def.problem_description,
            def.solution_markdown,
            rules,
            def.confidence,
            def.source_campaign_id,
            def.source_repository_id,
            time_util::now()
        ],
    )?;
    get(conn, &id)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<MigrationPattern> {
    conn.query_row(
        &format!("{COLUMNS} FROM migration_patterns WHERE id = ?1"),
        [id],
        map,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Migration pattern", id))
}

pub fn list(conn: &Connection, params: &ListParams) -> AppResult<Page<MigrationPattern>> {
    let statuses = params.statuses_as(ApprovalStatus::parse)?;
    let values: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
    let mut filter = Filter::default();
    filter.any_of("approval_status", &values);
    filter.search(&["name", "problem_description"], params.search_pattern());
    paginate(
        conn,
        COLUMNS,
        "migration_patterns",
        &filter,
        "updated_at DESC, id",
        params,
        map,
    )
}

/// Editing a pattern's content returns it to `candidate` for re-approval.
pub fn update(conn: &Connection, id: &str, def: &PatternDefinition) -> AppResult<MigrationPattern> {
    let (applicability, rules) = def.validate()?;
    let changed = conn.execute(
        "UPDATE migration_patterns SET name = ?2, applicability_json = ?3, problem_description = ?4,
             solution_markdown = ?5, verification_rules_json = ?6, confidence = ?7, approval_status = 'candidate',
             updated_at = ?8 WHERE id = ?1",
        params![id, def.name.trim(), applicability, def.problem_description, def.solution_markdown, rules, def.confidence, time_util::now()],
    )?;
    expect_changed(changed, "Migration pattern", id)?;
    get(conn, id)
}

pub fn set_approval(
    conn: &Connection,
    id: &str,
    status: ApprovalStatus,
) -> AppResult<MigrationPattern> {
    let changed = conn.execute(
        "UPDATE migration_patterns SET approval_status = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, status, time_util::now()],
    )?;
    expect_changed(changed, "Migration pattern", id)?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let changed = conn.execute("DELETE FROM migration_patterns WHERE id = ?1", [id])?;
    expect_changed(changed, "Migration pattern", id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::campaigns::{self, tests::definition};
    use crate::db::Database;
    use serde_json::json;

    fn def(campaign_id: Option<String>) -> PatternDefinition {
        PatternDefinition {
            name: "request to fetch".into(),
            applicability: json!({ "dependency": "request" }),
            problem_description: "request is deprecated".into(),
            solution_markdown: "Use `fetch`.".into(),
            verification_rules: json!(["pnpm test"]),
            confidence: 0.7,
            source_campaign_id: campaign_id,
            source_repository_id: None,
        }
    }

    #[test]
    fn crud_approval_and_source_set_null() {
        let db = Database::open_in_memory();
        let campaign = db
            .write(|c| campaigns::create_with_repositories(c, &definition("x"), &[]))
            .unwrap();
        let p = db
            .write(|c| create(c, &def(Some(campaign.id.clone()))))
            .unwrap();
        assert_eq!(p.approval_status, ApprovalStatus::Candidate);
        db.write(|c| set_approval(c, &p.id, ApprovalStatus::Approved))
            .unwrap();
        let edited = db
            .write(|c| {
                update(
                    c,
                    &p.id,
                    &PatternDefinition {
                        name: "renamed".into(),
                        ..def(None)
                    },
                )
            })
            .unwrap();
        assert_eq!(
            edited.approval_status,
            ApprovalStatus::Candidate,
            "edits require re-approval"
        );

        let approved = db
            .read(|c| {
                list(
                    c,
                    &ListParams {
                        statuses: vec!["approved".into()],
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(approved.total, 0);

        db.write(|c| campaigns::delete(c, &campaign.id)).unwrap();
        assert_eq!(db.read(|c| get(c, &p.id)).unwrap().source_campaign_id, None);

        assert!(db
            .write(|c| create(
                c,
                &PatternDefinition {
                    confidence: -0.1,
                    ..def(None)
                }
            ))
            .is_err());
        db.write(|c| delete(c, &p.id)).unwrap();
        assert!(db.write(|c| delete(c, &p.id)).is_err());
    }
}
