//! Campaigns and campaign repository membership.

// Service operations beyond the read paths are used by later steps
// (discovery, planning, delegation, verification) and are covered by
// this module's tests; only test builds exercise them today.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    expect_changed, json_col, json_text, new_id, paginate, require_text, require_unit_interval,
    string_enum, Filter, JsonShape, ListParams, Page,
};
use crate::error::{AppError, AppResult};
use crate::time_util;

string_enum!(CampaignStatus {
    Draft => "draft",
    Planning => "planning",
    Ready => "ready",
    Running => "running",
    Paused => "paused",
    Completed => "completed",
    Failed => "failed",
    Cancelled => "cancelled",
});

string_enum!(Classification {
    Unclassified => "unclassified",
    Affected => "affected",
    NotAffected => "not_affected",
    NeedsReview => "needs_review",
});

string_enum!(MembershipStatus {
    Pending => "pending",
    Excluded => "excluded",
    Queued => "queued",
    InProgress => "in_progress",
    AwaitingVerification => "awaiting_verification",
    Verified => "verified",
    Failed => "failed",
    Blocked => "blocked",
});

impl CampaignStatus {
    pub fn can_transition_to(self, to: CampaignStatus) -> bool {
        use CampaignStatus::*;
        matches!(
            (self, to),
            (Draft, Planning | Cancelled)
                | (Planning, Ready | Draft | Cancelled)
                | (Ready, Running | Planning | Cancelled)
                | (Running, Paused | Completed | Failed | Cancelled)
                | (Paused, Running | Cancelled)
        )
    }

    /// Definition and membership can change only before execution starts.
    pub fn is_editable(self) -> bool {
        matches!(self, CampaignStatus::Draft | CampaignStatus::Planning)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Campaign {
    pub id: String,
    pub name: String,
    pub objective: String,
    pub migration_type: String,
    pub source_condition: Value,
    pub target_condition: Value,
    pub constraints: Value,
    pub verification_profile: String,
    pub maximum_parallel_tasks: u32,
    pub status: CampaignStatus,
    pub repository_count: u32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignDefinition {
    pub name: String,
    pub objective: String,
    pub migration_type: String,
    pub source_condition: Value,
    pub target_condition: Value,
    pub constraints: Value,
    pub verification_profile: String,
    pub maximum_parallel_tasks: u32,
}

impl CampaignDefinition {
    fn validate(&self) -> AppResult<[String; 3]> {
        require_text(&self.name, "Campaign name", 200)?;
        require_text(&self.objective, "Objective", 10_000)?;
        require_text(&self.migration_type, "Migration type", 64)?;
        require_text(&self.verification_profile, "Verification profile", 64)?;
        if !(1..=100).contains(&self.maximum_parallel_tasks) {
            return Err(AppError::validation(
                "Maximum parallel tasks must be between 1 and 100.",
            ));
        }
        Ok([
            json_text(&self.source_condition, "sourceCondition", JsonShape::Object)?,
            json_text(&self.target_condition, "targetCondition", JsonShape::Object)?,
            json_text(&self.constraints, "constraints", JsonShape::Object)?,
        ])
    }
}

const CAMPAIGN_COLUMNS: &str = "SELECT c.id, c.name, c.objective, c.migration_type, c.source_condition_json, \
    c.target_condition_json, c.constraints_json, c.verification_profile, c.maximum_parallel_tasks, c.status, \
    c.created_at, c.updated_at, \
    (SELECT COUNT(*) FROM campaign_repositories cr WHERE cr.campaign_id = c.id) AS repository_count";

fn map_campaign(r: &rusqlite::Row<'_>) -> rusqlite::Result<Campaign> {
    Ok(Campaign {
        id: r.get("id")?,
        name: r.get("name")?,
        objective: r.get("objective")?,
        migration_type: r.get("migration_type")?,
        source_condition: json_col(r, "source_condition_json")?,
        target_condition: json_col(r, "target_condition_json")?,
        constraints: json_col(r, "constraints_json")?,
        verification_profile: r.get("verification_profile")?,
        maximum_parallel_tasks: r.get("maximum_parallel_tasks")?,
        status: r.get("status")?,
        repository_count: r.get("repository_count")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

/// Create a campaign in `draft` together with its selected repositories.
/// Call inside `Database::write`: if any repository is missing or duplicated
/// the whole operation rolls back and no campaign remains.
pub fn create_with_repositories(
    conn: &Connection,
    def: &CampaignDefinition,
    repository_ids: &[String],
) -> AppResult<Campaign> {
    let [source, target, constraints] = def.validate()?;
    let mut seen = HashSet::new();
    if let Some(dup) = repository_ids.iter().find(|id| !seen.insert(id.as_str())) {
        return Err(
            AppError::validation("A repository was selected more than once.")
                .with_context("repositoryId", dup.as_str()),
        );
    }
    let id = new_id();
    let now = time_util::now();
    conn.execute(
        "INSERT INTO campaigns (id, name, objective, migration_type, source_condition_json, target_condition_json,
             constraints_json, verification_profile, maximum_parallel_tasks, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'draft', ?10, ?10)",
        params![
            id,
            def.name.trim(),
            def.objective,
            def.migration_type,
            source,
            target,
            constraints,
            def.verification_profile,
            def.maximum_parallel_tasks,
            now
        ],
    )?;
    for repository_id in repository_ids {
        insert_membership(conn, &id, repository_id, &now)?;
    }
    get(conn, &id)
}

fn insert_membership(
    conn: &Connection,
    campaign_id: &str,
    repository_id: &str,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO campaign_repositories (id, campaign_id, repository_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![new_id(), campaign_id, repository_id, now],
    )
    .map_err(|e| {
        let err: AppError = e.into();
        if err.message.contains("does not exist") {
            AppError::validation("A selected repository does not exist.")
                .with_context("repositoryId", repository_id)
        } else {
            err
        }
    })?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Campaign> {
    conn.query_row(
        &format!("{CAMPAIGN_COLUMNS} FROM campaigns c WHERE c.id = ?1"),
        [id],
        map_campaign,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Campaign", id))
}

pub fn list(conn: &Connection, params: &ListParams) -> AppResult<Page<Campaign>> {
    let statuses = params.statuses_as(CampaignStatus::parse)?;
    let values: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
    let mut filter = Filter::default();
    filter.any_of("c.status", &values);
    filter.search(
        &["c.name", "c.objective", "c.migration_type"],
        params.search_pattern(),
    );
    paginate(
        conn,
        CAMPAIGN_COLUMNS,
        "campaigns c",
        &filter,
        "c.updated_at DESC, c.id",
        params,
        map_campaign,
    )
}

pub fn update_definition(
    conn: &Connection,
    id: &str,
    def: &CampaignDefinition,
) -> AppResult<Campaign> {
    let current = get(conn, id)?;
    if !current.status.is_editable() {
        return Err(AppError::validation(format!(
            "A {} campaign cannot be edited.",
            current.status.as_str()
        )));
    }
    let [source, target, constraints] = def.validate()?;
    conn.execute(
        "UPDATE campaigns SET name = ?2, objective = ?3, migration_type = ?4, source_condition_json = ?5,
             target_condition_json = ?6, constraints_json = ?7, verification_profile = ?8,
             maximum_parallel_tasks = ?9, updated_at = ?10 WHERE id = ?1",
        params![
            id,
            def.name.trim(),
            def.objective,
            def.migration_type,
            source,
            target,
            constraints,
            def.verification_profile,
            def.maximum_parallel_tasks,
            time_util::now()
        ],
    )?;
    get(conn, id)
}

pub fn set_status(conn: &Connection, id: &str, to: CampaignStatus) -> AppResult<Campaign> {
    let current = get(conn, id)?;
    if !current.status.can_transition_to(to) {
        return Err(AppError::validation(format!(
            "Campaign cannot move from {} to {}.",
            current.status.as_str(),
            to.as_str()
        )));
    }
    conn.execute(
        "UPDATE campaigns SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, to, time_util::now()],
    )?;
    get(conn, id)
}

/// Delete a draft or cancelled campaign and its membership rows. Fails if
/// agent tasks or verification runs reference it (their history is kept).
pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let current = get(conn, id)?;
    if !matches!(
        current.status,
        CampaignStatus::Draft | CampaignStatus::Cancelled
    ) {
        return Err(AppError::validation(
            "Only draft or cancelled campaigns can be deleted.",
        ));
    }
    let changed = conn
        .execute("DELETE FROM campaigns WHERE id = ?1", [id])
        .map_err(|e| {
            let err: AppError = e.into();
            if err.message.contains("still in use") {
                AppError::validation(
                    "This campaign has agent or verification history and cannot be deleted.",
                )
            } else {
                err
            }
        })?;
    expect_changed(changed, "Campaign", id)
}

// ---------------------------------------------------------------- membership

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignRepository {
    pub id: String,
    pub campaign_id: String,
    pub repository_id: String,
    pub repository_name: String,
    pub classification: Classification,
    pub risk_score: Option<f64>,
    pub risk_explanation: Value,
    pub wave_number: Option<u32>,
    pub status: MembershipStatus,
    pub exclusion_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

const MEMBER_COLUMNS: &str = "SELECT cr.id, cr.campaign_id, cr.repository_id, r.name AS repository_name, \
    cr.classification, cr.risk_score, cr.risk_explanation_json, cr.wave_number, cr.status, cr.exclusion_reason, \
    cr.created_at, cr.updated_at";
const MEMBER_FROM: &str = "campaign_repositories cr JOIN repositories r ON r.id = cr.repository_id";

fn map_member(r: &rusqlite::Row<'_>) -> rusqlite::Result<CampaignRepository> {
    Ok(CampaignRepository {
        id: r.get("id")?,
        campaign_id: r.get("campaign_id")?,
        repository_id: r.get("repository_id")?,
        repository_name: r.get("repository_name")?,
        classification: r.get("classification")?,
        risk_score: r.get("risk_score")?,
        risk_explanation: json_col(r, "risk_explanation_json")?,
        wave_number: r.get("wave_number")?,
        status: r.get("status")?,
        exclusion_reason: r.get("exclusion_reason")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

pub fn get_member(conn: &Connection, id: &str) -> AppResult<CampaignRepository> {
    conn.query_row(
        &format!("{MEMBER_COLUMNS} FROM {MEMBER_FROM} WHERE cr.id = ?1"),
        [id],
        map_member,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Campaign repository", id))
}

pub fn list_members(
    conn: &Connection,
    campaign_id: &str,
    params: &ListParams,
) -> AppResult<Page<CampaignRepository>> {
    let statuses = params.statuses_as(MembershipStatus::parse)?;
    let values: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
    let mut filter = Filter::default();
    filter
        .eq("cr.campaign_id", campaign_id.to_string())
        .any_of("cr.status", &values);
    filter.search(&["r.name"], params.search_pattern());
    paginate(
        conn,
        MEMBER_COLUMNS,
        MEMBER_FROM,
        &filter,
        "COALESCE(cr.wave_number, 1000000), r.name COLLATE NOCASE",
        params,
        map_member,
    )
}

fn require_editable(conn: &Connection, campaign_id: &str) -> AppResult<()> {
    let campaign = get(conn, campaign_id)?;
    if !campaign.status.is_editable() {
        return Err(AppError::validation(format!(
            "Repositories cannot be changed on a {} campaign.",
            campaign.status.as_str()
        )));
    }
    Ok(())
}

pub fn add_repositories(
    conn: &Connection,
    campaign_id: &str,
    repository_ids: &[String],
) -> AppResult<()> {
    require_editable(conn, campaign_id)?;
    let now = time_util::now();
    for id in repository_ids {
        insert_membership(conn, campaign_id, id, &now)?;
    }
    conn.execute(
        "UPDATE campaigns SET updated_at = ?2 WHERE id = ?1",
        params![campaign_id, now],
    )?;
    Ok(())
}

pub fn remove_member(conn: &Connection, member_id: &str) -> AppResult<()> {
    let member = get_member(conn, member_id)?;
    require_editable(conn, &member.campaign_id)?;
    let changed = conn.execute(
        "DELETE FROM campaign_repositories WHERE id = ?1",
        [member_id],
    )?;
    expect_changed(changed, "Campaign repository", member_id)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assessment {
    pub classification: Classification,
    pub risk_score: Option<f64>,
    pub risk_explanation: Value,
    pub wave_number: Option<u32>,
}

pub fn assess_member(
    conn: &Connection,
    member_id: &str,
    a: &Assessment,
) -> AppResult<CampaignRepository> {
    if let Some(score) = a.risk_score {
        require_unit_interval(score, "Risk score")?;
    }
    let explanation = json_text(&a.risk_explanation, "riskExplanation", JsonShape::Array)?;
    let changed = conn.execute(
        "UPDATE campaign_repositories SET classification = ?2, risk_score = ?3, risk_explanation_json = ?4,
             wave_number = ?5, updated_at = ?6 WHERE id = ?1",
        params![member_id, a.classification, a.risk_score, explanation, a.wave_number, time_util::now()],
    )?;
    expect_changed(changed, "Campaign repository", member_id)?;
    get_member(conn, member_id)
}

pub fn set_member_status(
    conn: &Connection,
    member_id: &str,
    status: MembershipStatus,
) -> AppResult<CampaignRepository> {
    if status == MembershipStatus::Excluded {
        return Err(AppError::validation(
            "Use exclude_member to exclude a repository with a reason.",
        ));
    }
    let changed = conn.execute(
        "UPDATE campaign_repositories SET status = ?2, exclusion_reason = NULL, updated_at = ?3 WHERE id = ?1",
        params![member_id, status, time_util::now()],
    )?;
    expect_changed(changed, "Campaign repository", member_id)?;
    get_member(conn, member_id)
}

pub fn exclude_member(
    conn: &Connection,
    member_id: &str,
    reason: &str,
) -> AppResult<CampaignRepository> {
    require_text(reason, "Exclusion reason", 2000)?;
    let changed = conn.execute(
        "UPDATE campaign_repositories SET status = 'excluded', exclusion_reason = ?2, updated_at = ?3 WHERE id = ?1",
        params![member_id, reason.trim(), time_util::now()],
    )?;
    expect_changed(changed, "Campaign repository", member_id)?;
    get_member(conn, member_id)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::db::repositories::tests::local;
    use crate::db::{repositories, Database};
    use crate::error::ErrorCode;
    use serde_json::json;

    pub fn definition(name: &str) -> CampaignDefinition {
        CampaignDefinition {
            name: name.into(),
            objective: "Replace request with fetch".into(),
            migration_type: "dependency_replacement".into(),
            source_condition: json!({ "dependency": "request" }),
            target_condition: json!({ "dependency": "undici" }),
            constraints: json!({}),
            verification_profile: "node".into(),
            maximum_parallel_tasks: 2,
        }
    }

    fn count(db: &Database, table: &str) -> i64 {
        db.read(|c| Ok(c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?))
            .unwrap()
    }

    #[test]
    fn creates_campaign_with_repositories_atomically() {
        let db = Database::open_in_memory();
        let (a, b) = db.write(|c| Ok((local(c, "a"), local(c, "b")))).unwrap();
        let campaign = db
            .write(|c| {
                create_with_repositories(
                    c,
                    &definition("Drop request"),
                    &[a.id.clone(), b.id.clone()],
                )
            })
            .unwrap();
        assert_eq!(campaign.status, CampaignStatus::Draft);
        assert_eq!(campaign.repository_count, 2);
        assert_eq!(
            campaign.source_condition,
            json!({ "dependency": "request" })
        );
        let members = db
            .read(|c| list_members(c, &campaign.id, &ListParams::default()))
            .unwrap();
        assert_eq!(
            members
                .items
                .iter()
                .map(|m| m.repository_name.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
    }

    #[test]
    fn failed_attachment_leaves_no_partial_campaign() {
        let db = Database::open_in_memory();
        let a = db.write(|c| Ok(local(c, "a"))).unwrap();
        let err = db
            .write(|c| {
                create_with_repositories(
                    c,
                    &definition("Broken"),
                    &[a.id.clone(), "missing-repo".into()],
                )
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationFailed);
        assert_eq!(count(&db, "campaigns"), 0);
        assert_eq!(count(&db, "campaign_repositories"), 0);

        let dup = db.write(|c| {
            create_with_repositories(c, &definition("Dup"), &[a.id.clone(), a.id.clone()])
        });
        assert_eq!(dup.unwrap_err().code, ErrorCode::ValidationFailed);
        assert_eq!(count(&db, "campaigns"), 0);
    }

    #[test]
    fn one_membership_per_campaign_and_repository() {
        let db = Database::open_in_memory();
        let a = db.write(|c| Ok(local(c, "a"))).unwrap();
        let campaign = db
            .write(|c| create_with_repositories(c, &definition("x"), std::slice::from_ref(&a.id)))
            .unwrap();
        let err = db
            .write(|c| add_repositories(c, &campaign.id, std::slice::from_ref(&a.id)))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationFailed);
        assert_eq!(count(&db, "campaign_repositories"), 1);
    }

    #[test]
    fn status_transitions_are_enforced() {
        let db = Database::open_in_memory();
        let campaign = db
            .write(|c| create_with_repositories(c, &definition("x"), &[]))
            .unwrap();
        assert!(db
            .write(|c| set_status(c, &campaign.id, CampaignStatus::Running))
            .is_err());
        db.write(|c| set_status(c, &campaign.id, CampaignStatus::Planning))
            .unwrap();
        db.write(|c| set_status(c, &campaign.id, CampaignStatus::Ready))
            .unwrap();
        db.write(|c| set_status(c, &campaign.id, CampaignStatus::Running))
            .unwrap();
        assert!(db
            .write(|c| update_definition(c, &campaign.id, &definition("renamed")))
            .is_err());
        assert!(db.write(|c| delete(c, &campaign.id)).is_err());
        db.write(|c| set_status(c, &campaign.id, CampaignStatus::Cancelled))
            .unwrap();
        db.write(|c| delete(c, &campaign.id)).unwrap();
    }

    #[test]
    fn repository_in_campaign_cannot_be_removed_but_campaign_delete_cascades() {
        let db = Database::open_in_memory();
        let a = db.write(|c| Ok(local(c, "a"))).unwrap();
        let campaign = db
            .write(|c| create_with_repositories(c, &definition("x"), std::slice::from_ref(&a.id)))
            .unwrap();
        let err = db.write(|c| repositories::remove(c, &a.id)).unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationFailed);
        assert!(err.message.contains("belongs to a campaign"));

        db.write(|c| delete(c, &campaign.id)).unwrap();
        assert_eq!(count(&db, "campaign_repositories"), 0);
        db.write(|c| repositories::remove(c, &a.id)).unwrap();
    }

    #[test]
    fn assessment_exclusion_and_filters() {
        let db = Database::open_in_memory();
        let (a, b) = db.write(|c| Ok((local(c, "a"), local(c, "b")))).unwrap();
        let campaign = db
            .write(|c| create_with_repositories(c, &definition("x"), &[a.id.clone(), b.id.clone()]))
            .unwrap();
        let members = db
            .read(|c| list_members(c, &campaign.id, &ListParams::default()))
            .unwrap()
            .items;
        let assessed = db
            .write(|c| {
                assess_member(
                    c,
                    &members[0].id,
                    &Assessment {
                        classification: Classification::Affected,
                        risk_score: Some(0.3),
                        risk_explanation: json!(["uses request in 4 files"]),
                        wave_number: Some(0),
                    },
                )
            })
            .unwrap();
        assert_eq!(assessed.classification, Classification::Affected);
        let bad = Assessment {
            classification: Classification::Affected,
            risk_score: Some(2.0),
            risk_explanation: json!([]),
            wave_number: None,
        };
        assert!(db
            .write(|c| assess_member(c, &members[0].id, &bad))
            .is_err());

        assert!(db
            .write(|c| exclude_member(c, &members[1].id, "  "))
            .is_err());
        db.write(|c| exclude_member(c, &members[1].id, "archived repository"))
            .unwrap();
        let excluded = db
            .read(|c| {
                list_members(
                    c,
                    &campaign.id,
                    &ListParams {
                        statuses: vec!["excluded".into()],
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(excluded.total, 1);
        assert_eq!(
            excluded.items[0].exclusion_reason.as_deref(),
            Some("archived repository")
        );

        // Re-including clears the reason; removal works while the campaign is editable.
        let back = db
            .write(|c| set_member_status(c, &members[1].id, MembershipStatus::Pending))
            .unwrap();
        assert_eq!(back.exclusion_reason, None);
        assert!(db
            .write(|c| set_member_status(c, &members[1].id, MembershipStatus::Excluded))
            .is_err());
        db.write(|c| exclude_member(c, &members[1].id, "archived repository"))
            .unwrap();

        // The CHECK constraint guards excluded rows without a reason.
        let raw = db.write(|c| {
            Ok(c.execute(
                "UPDATE campaign_repositories SET exclusion_reason = NULL WHERE id = ?1",
                [&members[1].id],
            )?)
        });
        assert_eq!(raw.unwrap_err().code, ErrorCode::ValidationFailed);
    }

    #[test]
    fn membership_removal_only_while_editable() {
        let db = Database::open_in_memory();
        let (a, b) = db.write(|c| Ok((local(c, "a"), local(c, "b")))).unwrap();
        let campaign = db
            .write(|c| create_with_repositories(c, &definition("x"), &[a.id.clone(), b.id.clone()]))
            .unwrap();
        let members = db
            .read(|c| list_members(c, &campaign.id, &ListParams::default()))
            .unwrap()
            .items;
        db.write(|c| remove_member(c, &members[0].id)).unwrap();
        assert_eq!(
            db.read(|c| get(c, &campaign.id)).unwrap().repository_count,
            1
        );
        db.write(|c| set_status(c, &campaign.id, CampaignStatus::Planning))
            .unwrap();
        db.write(|c| set_status(c, &campaign.id, CampaignStatus::Ready))
            .unwrap();
        assert_eq!(
            db.write(|c| remove_member(c, &members[1].id))
                .unwrap_err()
                .code,
            ErrorCode::ValidationFailed
        );
        assert_eq!(
            db.write(|c| add_repositories(c, &campaign.id, std::slice::from_ref(&a.id)))
                .unwrap_err()
                .code,
            ErrorCode::ValidationFailed
        );
    }

    #[test]
    fn list_filters_by_status_and_search() {
        let db = Database::open_in_memory();
        let one = db
            .write(|c| create_with_repositories(c, &definition("Upgrade React"), &[]))
            .unwrap();
        db.write(|c| create_with_repositories(c, &definition("Drop moment"), &[]))
            .unwrap();
        db.write(|c| set_status(c, &one.id, CampaignStatus::Planning))
            .unwrap();
        let planning = db
            .read(|c| {
                list(
                    c,
                    &ListParams {
                        statuses: vec!["planning".into()],
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(planning.total, 1);
        let search = db
            .read(|c| {
                list(
                    c,
                    &ListParams {
                        search: Some("moment".into()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(search.items[0].name, "Drop moment");
    }
}
