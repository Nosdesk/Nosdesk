//! Read side of the rules conflicts inspector: which automated rules can
//! set each other off, and which ran on the same tickets lately.
//!
//! Only rules that run on their own and are running (live, or dry run's
//! shadow runs) are checked. Manual rules run when an agent picks them;
//! draft, paused and archived rules don't run at all. Workspace isolation
//! is RLS, as for the rest of the rules tables.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_types::{Array, BigInt, Int4, Timestamptz};
use diesel::QueryResult;
use serde::Serialize;

use crate::db::DbConnection;
use crate::models::{Rule, RuleState, RuleTriggerKind};

/// At most this many co-firing pairs come back, most recent first.
const CO_FIRE_LIMIT: i64 = 200;
/// Sample tickets kept for each co-firing pair.
const CO_FIRE_SAMPLES: i32 = 5;

/// A checked rule, as much as the inspector shows of it.
#[derive(Debug, Clone, Serialize)]
pub struct ConflictRule {
    pub id: i32,
    pub name: String,
    pub trigger_kind: RuleTriggerKind,
    pub state: RuleState,
    pub priority: i32,
}

/// `writer`'s steps change `fields`, which `reader`'s conditions check, so
/// a run of `writer` can decide whether `reader` matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleOverlap {
    pub writer_rule_id: i32,
    pub reader_rule_id: i32,
    pub fields: Vec<String>,
}

/// Two checked rules that both ran on `ticket_count` tickets since the
/// window opened. `first_rule_id` is the lower id.
#[derive(Debug, Clone, Serialize, QueryableByName)]
pub struct RuleCoFire {
    #[diesel(sql_type = Int4)]
    pub first_rule_id: i32,
    #[diesel(sql_type = Int4)]
    pub second_rule_id: i32,
    #[diesel(sql_type = BigInt)]
    pub ticket_count: i64,
    #[diesel(sql_type = Timestamptz)]
    pub last_co_fired_at: DateTime<Utc>,
    #[diesel(sql_type = Array<Int4>)]
    pub sample_ticket_ids: Vec<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleConflicts {
    pub since: DateTime<Utc>,
    pub rules: Vec<ConflictRule>,
    pub overlaps: Vec<RuleOverlap>,
    pub co_fires: Vec<RuleCoFire>,
}

// sync-audit-only: read-only
/// The workspace's checked rules, the pairs where one writes what the
/// other reads, and the pairs that both ran on a ticket since `since`.
pub fn find(conn: &mut DbConnection, since: DateTime<Utc>) -> QueryResult<RuleConflicts> {
    use crate::schema::rules::dsl;

    let checked: Vec<Rule> = dsl::rules
        .filter(dsl::archived_at.is_null())
        .filter(dsl::trigger_kind.ne(RuleTriggerKind::Manual))
        .filter(dsl::state.eq_any([RuleState::Live, RuleState::DryRun]))
        .order((dsl::priority.asc(), dsl::name.asc(), dsl::id.asc()))
        .load(conn)?;
    let overlaps = overlaps(&checked);
    let ids: Vec<i32> = checked.iter().map(|r| r.id).collect();
    let co_fires = if ids.len() < 2 {
        Vec::new()
    } else {
        // One row per (rule, ticket) first, so a rule that ran on a ticket
        // many times doesn't multiply the join.
        diesel::sql_query(
            "WITH fired AS ( \
                 SELECT rule_id, ticket_id, MAX(applied_at) AS last_at \
                 FROM rule_applications \
                 WHERE rule_id = ANY($1) \
                   AND status IN ('succeeded', 'dry_run') \
                   AND applied_at >= $2 \
                 GROUP BY rule_id, ticket_id \
             ) \
             SELECT a.rule_id AS first_rule_id, \
                    b.rule_id AS second_rule_id, \
                    COUNT(*) AS ticket_count, \
                    MAX(GREATEST(a.last_at, b.last_at)) AS last_co_fired_at, \
                    (ARRAY_AGG(a.ticket_id ORDER BY GREATEST(a.last_at, b.last_at) DESC, \
                        a.ticket_id DESC))[1:$3] AS sample_ticket_ids \
             FROM fired a \
             JOIN fired b ON b.ticket_id = a.ticket_id AND b.rule_id > a.rule_id \
             GROUP BY a.rule_id, b.rule_id \
             ORDER BY last_co_fired_at DESC, first_rule_id, second_rule_id \
             LIMIT $4",
        )
        .bind::<Array<Int4>, _>(&ids)
        .bind::<Timestamptz, _>(since)
        .bind::<Int4, _>(CO_FIRE_SAMPLES)
        .bind::<BigInt, _>(CO_FIRE_LIMIT)
        .load(conn)?
    };
    Ok(RuleConflicts {
        since,
        rules: checked
            .into_iter()
            .map(|r| ConflictRule {
                id: r.id,
                name: r.name,
                trigger_kind: r.trigger_kind,
                state: r.state,
                priority: r.priority,
            })
            .collect(),
        overlaps,
        co_fires,
    })
}

/// Every ordered pair of distinct rules where the first's `writes_set`
/// meets the second's `reads_set`, in the order `rules` come. A rule that
/// reads its own writes is the save linter's concern, not a pair.
pub fn overlaps(rules: &[Rule]) -> Vec<RuleOverlap> {
    let mut out = Vec::new();
    for writer in rules {
        for reader in rules {
            if writer.id == reader.id {
                continue;
            }
            let mut fields: Vec<String> = writer
                .writes_set
                .iter()
                .flatten()
                .filter(|w| reader.reads_set.iter().flatten().any(|r| r == *w))
                .cloned()
                .collect();
            if fields.is_empty() {
                continue;
            }
            fields.sort();
            fields.dedup();
            out.push(RuleOverlap {
                writer_rule_id: writer.id,
                reader_rule_id: reader.id,
                fields,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: i32, reads: &[&str], writes: &[&str]) -> Rule {
        let set = |s: &[&str]| s.iter().map(|f| Some(f.to_string())).collect();
        Rule {
            id,
            workspace_id: 1,
            name: format!("rule {id}"),
            description: None,
            trigger_kind: RuleTriggerKind::TicketUpdated,
            trigger_config: serde_json::json!({}),
            conditions: serde_json::json!([]),
            actions: serde_json::json!([]),
            reads_set: set(reads),
            writes_set: set(writes),
            state: RuleState::Live,
            priority: 100,
            last_fired_at: None,
            fire_count: 0,
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            archived_at: None,
        }
    }

    #[test]
    fn a_write_another_rule_reads_is_a_pair() {
        let escalate = rule(1, &["ticket.tag_ids"], &["ticket.priority"]);
        let route = rule(2, &["ticket.priority"], &["ticket.assignee_uuid"]);
        assert_eq!(
            overlaps(&[escalate, route]),
            vec![RuleOverlap {
                writer_rule_id: 1,
                reader_rule_id: 2,
                fields: vec!["ticket.priority".to_string()],
            }]
        );
    }

    #[test]
    fn each_direction_is_its_own_pair() {
        let close = rule(
            1,
            &["ticket.priority"],
            &["ticket.workflow_state_id", "ticket.workflow_state.category"],
        );
        let reopen = rule(2, &["ticket.workflow_state.category"], &["ticket.priority"]);
        let pairs = overlaps(&[close, reopen]);
        assert_eq!(pairs.len(), 2);
        assert_eq!((pairs[0].writer_rule_id, pairs[0].reader_rule_id), (1, 2));
        assert_eq!(pairs[0].fields, vec!["ticket.workflow_state.category"]);
        assert_eq!((pairs[1].writer_rule_id, pairs[1].reader_rule_id), (2, 1));
        assert_eq!(pairs[1].fields, vec!["ticket.priority"]);
    }

    #[test]
    fn a_rule_reading_its_own_write_or_no_shared_field_is_no_pair() {
        let own = rule(1, &["ticket.priority"], &["ticket.priority"]);
        let other = rule(2, &["ticket.title"], &["ticket.tag_ids"]);
        assert!(overlaps(&[own, other]).is_empty());
    }
}
