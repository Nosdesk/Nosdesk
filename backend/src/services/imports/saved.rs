//! Saved matches: the answers Match values keeps for the next import of a
//! type (`import_value_answers`).
//!
//! The commit saves an answer for each value the admin answered or saw and
//! let stand: a value answered on Match values, a Guessed value, and a value
//! left at its No match default. A value that matched exactly, or by a saved
//! match nobody changed, asked nothing and is not saved again. Never saved:
//! Skip these rows, Replace with, Leave blank on a required field, a blank
//! cell, and a number or date answer, which the next import doesn't read.
//! A Create is saved as a link to the entry the commit made, so the next
//! import matches it exactly.
//!
//! [`list`] reads them back for the Saved matches page, each with what it
//! does in the workspace, in field order. An answer whose target is gone
//! is left out, as Match values leaves it out.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{ImportJob, ImportValueAnswer, NewImportValueAnswer};
use crate::repository::imports as repo;

use super::mapping::{ColumnMapping, StoredSource};
use super::types::{CreatedLink, FieldSpec, ImportOptions, ImportType, ValueAnswer, ValueAnswers};
use super::values::{self, Layer, LookupKind, Matcher, Resolution};

/// One answer to keep for the next import.
#[derive(Debug, Clone, PartialEq)]
pub struct ToSave {
    pub field: String,
    pub context: String,
    /// The value as answers key it.
    pub key: String,
    /// The value as the file first spells it.
    pub text: String,
    pub answer: ValueAnswer,
    /// The answer links to an entry this import created.
    pub created: bool,
}

/// The answers a commit keeps, from the values it settled and the entries
/// it created.
pub fn answers_to_save(resolution: &Resolution, created: &[CreatedLink]) -> Vec<ToSave> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for column in &resolution.columns {
        // The next import reads a number or a date as it is, never from a
        // saved answer.
        if matches!(column.matcher, Matcher::Number(_) | Matcher::Date(_)) {
            continue;
        }
        for value in &column.values {
            if value.key.is_empty() {
                continue;
            }
            let asked =
                value.answer.is_some() || matches!(value.layer, Layer::Guessed | Layer::NoMatch);
            if !asked {
                continue;
            }
            let Some(answer) = value.effective() else {
                continue;
            };
            let (answer, made) = match answer {
                ValueAnswer::Skip | ValueAnswer::Replace { .. } => continue,
                ValueAnswer::LeaveBlank if column.required => continue,
                ValueAnswer::Create => {
                    let Some(link) = created.iter().find(|c| {
                        c.field == column.field && c.context == column.context && c.key == value.key
                    }) else {
                        // Nothing was made (its rows were skipped): ask again.
                        continue;
                    };
                    (
                        ValueAnswer::Match {
                            target: serde_json::Value::String(link.id.to_string()),
                        },
                        true,
                    )
                }
                other => (other.clone(), false),
            };
            if !seen.insert((
                column.field.clone(),
                column.context.clone(),
                value.key.clone(),
            )) {
                continue;
            }
            out.push(ToSave {
                field: column.field.clone(),
                context: column.context.clone(),
                key: value.key.clone(),
                text: value.text.clone(),
                answer,
                created: made,
            });
        }
    }
    out
}

/// Save `answers` for the next import of `job_type`, answered by `actor`
/// in `job_id`; the latest answer for a value replaces the one before.
pub fn save(
    conn: &mut DbConnection,
    job_type: ImportType,
    answers: Vec<ToSave>,
    actor: Option<Uuid>,
    job_id: Uuid,
) -> diesel::QueryResult<usize> {
    let rows: Vec<NewImportValueAnswer> = answers
        .into_iter()
        .filter_map(|a| {
            Some(NewImportValueAnswer {
                job_type: job_type.as_str().to_string(),
                field: a.field,
                context: a.context,
                normalized_value: a.key,
                answer: serde_json::to_value(&a.answer).ok()?,
                answered_by: actor,
                job_id: Some(job_id),
                source_text: Some(a.text),
                created: a.created,
            })
        })
        .collect();
    repo::upsert_value_answers(conn, &rows)
}

/// What a saved answer does in the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Links to an entry: `label` names it.
    Link,
    KeptAsText,
    /// A status left as it is.
    LeftAsIs,
    /// A group left off.
    LeftOff,
    /// A person left unassigned.
    LeftUnassigned,
    LeftBlank,
}

/// Who saved an answer, and in which import.
#[derive(Debug, Clone, Serialize)]
pub struct SavedBy {
    /// `None` once the person's account is gone.
    pub by: Option<Uuid>,
    pub by_name: Option<String>,
    pub at: chrono::DateTime<chrono::Utc>,
    pub job_id: Option<Uuid>,
}

/// One saved match as the Saved matches page lists it.
#[derive(Debug, Clone, Serialize)]
pub struct SavedEntry {
    pub id: Uuid,
    pub field: String,
    pub field_label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_key: Option<String>,
    /// Empty when the field is answered the same on every row.
    pub context: String,
    /// The context's name: a kind, a manufacturer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_label: Option<String>,
    /// The value as the file spells it.
    pub text: String,
    pub outcome: Outcome,
    /// The linked entry's id or key, for a link.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// The linked entry's name, for a link.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The import that saved it created the entry.
    pub created: bool,
    pub saved: SavedBy,
}

/// One header of the latest mapped import and where it went.
#[derive(Debug, Clone, Serialize)]
pub struct SavedColumn {
    pub header: String,
    /// `None` when the column went to no field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_key: Option<String>,
    /// Set to Don't import.
    pub ignored: bool,
}

/// The header memory: the type's latest mapped import, header by header.
#[derive(Debug, Clone, Serialize)]
pub struct SavedColumns {
    pub job_id: Uuid,
    pub by: Option<Uuid>,
    pub by_name: Option<String>,
    pub at: chrono::DateTime<chrono::Utc>,
    pub rows: Vec<SavedColumn>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SavedMatches {
    pub job_type: &'static str,
    /// In field order, then context, then value.
    pub entries: Vec<SavedEntry>,
    pub columns: Option<SavedColumns>,
}

/// Every saved match for `job_type` that still applies, and the header
/// memory of its latest mapped import.
pub fn list(
    conn: &mut DbConnection,
    job_type: ImportType,
    fields: &[FieldSpec],
) -> diesel::QueryResult<SavedMatches> {
    let rows = repo::list_value_answers(conn, job_type.as_str())?;
    let mut names = Names::default();
    let entries = entries(conn, job_type, fields, rows, &mut names)?;
    let columns = match repo::latest_with_mapping(conn, job_type.as_str())? {
        Some(job) => columns(conn, &job, fields, &mut names)?,
        None => None,
    };
    Ok(SavedMatches {
        job_type: job_type.as_str(),
        entries,
        columns,
    })
}

/// Account names by id, each read once.
#[derive(Default)]
struct Names(HashMap<Uuid, Option<String>>);

impl Names {
    fn get(
        &mut self,
        conn: &mut DbConnection,
        id: Option<Uuid>,
    ) -> diesel::QueryResult<Option<String>> {
        let Some(id) = id else {
            return Ok(None);
        };
        if let Some(name) = self.0.get(&id) {
            return Ok(name.clone());
        }
        let name = match crate::repository::users::get_user_by_uuid(&id, conn) {
            Ok(user) => Some(user.name),
            Err(diesel::result::Error::NotFound) => None,
            Err(e) => return Err(e),
        };
        self.0.insert(id, name.clone());
        Ok(name)
    }
}

fn entries(
    conn: &mut DbConnection,
    job_type: ImportType,
    fields: &[FieldSpec],
    rows: Vec<ImportValueAnswer>,
    names: &mut Names,
) -> diesel::QueryResult<Vec<SavedEntry>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    // Every field as if mapped, and the saved answers as if sent, gives a
    // matcher per field and context the answers name.
    let mapping: ColumnMapping = fields
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id.clone(), i))
        .collect();
    let mut parsed: Vec<(ImportValueAnswer, ValueAnswer)> = Vec::new();
    let mut named = ValueAnswers::new();
    for row in rows {
        let Ok(answer) = serde_json::from_value::<ValueAnswer>(row.answer.clone()) else {
            continue;
        };
        named
            .entry(row.field.clone())
            .or_default()
            .entry(row.context.clone())
            .or_default()
            .insert(row.normalized_value.clone(), answer.clone());
        parsed.push((row, answer));
    }
    let matchers = values::matchers_for_answers(
        conn,
        job_type,
        fields,
        &mapping,
        &ImportOptions::default(),
        &named,
    )?;
    let order: HashMap<&str, usize> = fields
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id.as_str(), i))
        .collect();

    let mut out = Vec::new();
    for (row, answer) in parsed {
        let Some(column) = matchers
            .iter()
            .find(|m| m.spec.id == row.field && m.context == row.context)
        else {
            continue;
        };
        if values::fits(
            &column.matcher,
            column.spec.required,
            &row.normalized_value,
            &answer,
        )
        .is_err()
        {
            continue;
        }
        let lookup = column.matcher.lookup();
        let (outcome, target, label) = match &answer {
            ValueAnswer::Match { target } => {
                let Some(choice) = column.matcher.pickable(target) else {
                    continue;
                };
                (
                    Outcome::Link,
                    Some(choice.value.clone()),
                    Some(choice.label.clone()),
                )
            }
            ValueAnswer::KeepText => (Outcome::KeptAsText, None, None),
            ValueAnswer::LeaveBlank => {
                let outcome = match lookup {
                    Some(LookupKind::Group) => Outcome::LeftOff,
                    Some(LookupKind::Person) => Outcome::LeftUnassigned,
                    _ if row.field == "status" => Outcome::LeftAsIs,
                    _ => Outcome::LeftBlank,
                };
                (outcome, None, None)
            }
            // Never saved; one that is reads as nothing here.
            ValueAnswer::Skip | ValueAnswer::Replace { .. } | ValueAnswer::Create => continue,
        };
        let by_name = names.get(conn, row.answered_by)?;
        out.push(SavedEntry {
            id: row.id,
            field: row.field.clone(),
            field_label: column.spec.label.clone(),
            label_key: column.spec.label_key.clone(),
            context: row.context.clone(),
            context_label: column.context_label.clone(),
            text: row
                .source_text
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| row.normalized_value.clone()),
            outcome,
            target,
            label,
            created: row.created && outcome == Outcome::Link,
            saved: SavedBy {
                by: row.answered_by,
                by_name,
                at: row.updated_at,
                job_id: row.job_id,
            },
        });
    }
    out.sort_by(|a, b| {
        let at = |e: &SavedEntry| order.get(e.field.as_str()).copied().unwrap_or(usize::MAX);
        at(a)
            .cmp(&at(b))
            .then_with(|| {
                sort_key(a.context_label.as_deref().unwrap_or(&a.context))
                    .cmp(&sort_key(b.context_label.as_deref().unwrap_or(&b.context)))
            })
            .then_with(|| sort_key(&a.text).cmp(&sort_key(&b.text)))
    });
    Ok(out)
}

fn sort_key(text: &str) -> String {
    values::normalize_value(text)
}

/// The header memory as the next import's Map step reads it: each header
/// of `job`'s file in source order and the field it went to.
fn columns(
    conn: &mut DbConnection,
    job: &ImportJob,
    fields: &[FieldSpec],
    names: &mut Names,
) -> diesel::QueryResult<Option<SavedColumns>> {
    let Some(stored) = job
        .source
        .clone()
        .and_then(|v| serde_json::from_value::<StoredSource>(v).ok())
    else {
        return Ok(None);
    };
    let Some(mapping) = job
        .mapping
        .clone()
        .and_then(|v| serde_json::from_value::<ColumnMapping>(v).ok())
    else {
        return Ok(None);
    };
    let by_index: BTreeMap<usize, &str> = mapping
        .iter()
        .map(|(field, &index)| (index, field.as_str()))
        .collect();
    let rows = stored
        .source
        .headers
        .iter()
        .map(|h| {
            let field = by_index.get(&h.index).copied();
            let spec = field.and_then(|f| fields.iter().find(|s| s.id == f));
            SavedColumn {
                header: h.name.clone(),
                field: field.map(String::from),
                field_label: spec.map(|s| s.label.clone()),
                label_key: spec.and_then(|s| s.label_key.clone()),
                ignored: field.is_none() && stored.ignored.contains(&h.index),
            }
        })
        .collect();
    Ok(Some(SavedColumns {
        job_id: job.id,
        by: job.created_by,
        by_name: names.get(conn, job.created_by)?,
        at: job.updated_at,
        rows,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::imports::types::{DecimalMark, ValueType};
    use crate::services::imports::values::{Choice, ResolvedColumn, ResolvedValue};

    fn link(target: &str) -> ValueAnswer {
        ValueAnswer::Match {
            target: serde_json::Value::String(target.to_string()),
        }
    }

    fn value(
        key: &str,
        layer: Layer,
        suggested: Option<ValueAnswer>,
        answer: Option<ValueAnswer>,
    ) -> ResolvedValue {
        ResolvedValue {
            key: key.to_string(),
            text: key.to_uppercase(),
            rows: vec![2],
            layer,
            reason: None,
            suggested,
            answer,
            saved: None,
        }
    }

    fn column(
        field: &str,
        required: bool,
        matcher: Matcher,
        values: Vec<ResolvedValue>,
    ) -> ResolvedColumn {
        ResolvedColumn {
            field: field.to_string(),
            index: 0,
            context: String::new(),
            family: None,
            context_label: None,
            value_type: ValueType::Enum,
            required,
            matcher,
            blank_is_value: false,
            values,
        }
    }

    fn choices() -> Matcher {
        Matcher::Choice {
            choices: vec![Choice::new("camera", "Camera")],
            aliases: Vec::new(),
            synonyms: Vec::new(),
            strict: false,
        }
    }

    fn keys(saved: &[ToSave]) -> Vec<(&str, &str)> {
        saved
            .iter()
            .map(|s| (s.field.as_str(), s.key.as_str()))
            .collect()
    }

    /// Answered, Guessed and No match values are saved; exact and saved
    /// ones, Skip, Replace with, Leave blank on a required field, a blank
    /// cell and number answers aren't.
    #[test]
    fn only_what_the_admin_was_asked_is_saved() {
        let resolution = Resolution {
            columns: vec![
                column(
                    "kind",
                    true,
                    choices(),
                    vec![
                        value("camera", Layer::Exact, Some(link("camera")), None),
                        value("cam", Layer::Guessed, Some(link("camera")), None),
                        value("b/cast", Layer::Saved, Some(link("camera")), None),
                        value("tripod", Layer::NoMatch, Some(ValueAnswer::Skip), None),
                        value(
                            "rig",
                            Layer::NoMatch,
                            Some(ValueAnswer::Skip),
                            Some(link("camera")),
                        ),
                        value(
                            "gone",
                            Layer::NoMatch,
                            Some(ValueAnswer::Skip),
                            Some(ValueAnswer::LeaveBlank),
                        ),
                        value(
                            "",
                            Layer::NoMatch,
                            Some(ValueAnswer::LeaveBlank),
                            Some(link("camera")),
                        ),
                    ],
                ),
                column(
                    "condition",
                    false,
                    choices(),
                    vec![
                        value("worn", Layer::NoMatch, Some(ValueAnswer::LeaveBlank), None),
                        value(
                            "x",
                            Layer::Exact,
                            Some(link("camera")),
                            Some(ValueAnswer::Replace { value: "y".into() }),
                        ),
                    ],
                ),
                column(
                    "quantity",
                    false,
                    Matcher::Number(DecimalMark::Dot),
                    vec![value(
                        "two",
                        Layer::NoMatch,
                        Some(ValueAnswer::Skip),
                        Some(ValueAnswer::LeaveBlank),
                    )],
                ),
            ],
            row_contexts: Default::default(),
        };
        let saved = answers_to_save(&resolution, &[]);
        assert_eq!(
            keys(&saved),
            vec![("kind", "cam"), ("kind", "rig"), ("condition", "worn")]
        );
        assert_eq!(saved[0].text, "CAM");
        assert_eq!(saved[2].answer, ValueAnswer::LeaveBlank);
    }

    /// A Create is saved as a link to what the commit made, and not at all
    /// when nothing was made.
    #[test]
    fn a_create_is_saved_as_a_link_to_what_was_made() {
        let lookup = Matcher::Lookup(values::Lookup::new(LookupKind::Manufacturer, Vec::new()));
        let resolution = Resolution {
            columns: vec![column(
                "manufacturer",
                false,
                lookup,
                vec![
                    value(
                        "blackmagic",
                        Layer::NoMatch,
                        Some(ValueAnswer::KeepText),
                        Some(ValueAnswer::Create),
                    ),
                    value(
                        "arri",
                        Layer::NoMatch,
                        Some(ValueAnswer::KeepText),
                        Some(ValueAnswer::Create),
                    ),
                ],
            )],
            row_contexts: Default::default(),
        };
        let created = [CreatedLink {
            field: "manufacturer".into(),
            context: String::new(),
            key: "blackmagic".into(),
            id: 42,
        }];
        let saved = answers_to_save(&resolution, &created);
        assert_eq!(keys(&saved), vec![("manufacturer", "blackmagic")]);
        assert_eq!(saved[0].answer, link("42"));
        assert!(saved[0].created);
    }
}
