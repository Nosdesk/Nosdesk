//! References a foreign key doesn't carry: ids and uuids inside JSONB, HTML,
//! text and collaborative documents.
//!
//! [`REFERENCES`] names every column that holds one, with where in the value
//! it sits ([`Shape`]), what it names ([`Entity`]) and what the import does
//! when the archive doesn't carry the row it names ([`Missing`]).
//! [`NO_IDS`] names every other JSONB, Yjs and loose id column on a carried
//! table, with why it holds none; a test fails when a migration adds one
//! that neither list names.
//!
//! The plan runs the registry twice through one walker ([`rewrite_row`]):
//! once while planning, with a [`Lookup`] that knows only what the archive
//! carries and where its people land, which settles every reference that
//! can't be kept and reports it; then for each row as it loads, with a
//! [`Lookup`] that gives each kept reference its new key. The report and
//! the rows can't disagree.
//!
//! A reference the import can't keep never keeps its archive value, which
//! here may be another workspace's row: a rule turns draft, a view drops the
//! predicate or scope, a mention or ticket link becomes its text, and a row
//! that is nothing without it (a notification about a ticket that didn't
//! come) stays out.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;

use regex::{Captures, Regex};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use super::plan::Row;
use crate::services::yjs::rewrite::{self as yjs, Counts, Rewrite, Targets};

/// What a reference names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Entity {
    /// A person, through the people map.
    User,
    Ticket,
    /// A ticket by its `uuid`.
    TicketUuid,
    Comment,
    Tag,
    Group,
    WorkflowState,
    Category,
    Project,
    Cycle,
    Asset,
    AssetGroup,
    AssetLoan,
    /// A documentation page by its `id`.
    Page,
    /// A documentation page by its `uuid`.
    PageUuid,
    /// CSV import history, which never moves.
    ImportJob,
}

impl Entity {
    /// The table and key column a reference names. `None` for people.
    pub fn key(self) -> Option<(&'static str, &'static str)> {
        Some(match self {
            Entity::User => return None,
            Entity::Ticket => ("tickets", "id"),
            Entity::TicketUuid => ("tickets", "uuid"),
            Entity::Comment => ("comments", "id"),
            Entity::Tag => ("tags", "id"),
            Entity::Group => ("groups", "id"),
            Entity::WorkflowState => ("workflow_states", "id"),
            Entity::Category => ("ticket_categories", "id"),
            Entity::Project => ("projects", "id"),
            Entity::Cycle => ("cycles", "id"),
            Entity::Asset => ("assets", "id"),
            Entity::AssetGroup => ("asset_groups", "id"),
            Entity::AssetLoan => ("asset_loans", "id"),
            Entity::Page => ("documentation_pages", "id"),
            Entity::PageUuid => ("documentation_pages", "uuid"),
            Entity::ImportJob => ("import_jobs", "id"),
        })
    }

    /// Named by a uuid rather than an integer.
    fn by_uuid(self) -> bool {
        matches!(
            self,
            Entity::User | Entity::TicketUuid | Entity::PageUuid | Entity::ImportJob
        )
    }

    pub const ALL: &'static [Entity] = &[
        Entity::User,
        Entity::Ticket,
        Entity::TicketUuid,
        Entity::Comment,
        Entity::Tag,
        Entity::Group,
        Entity::WorkflowState,
        Entity::Category,
        Entity::Project,
        Entity::Cycle,
        Entity::Asset,
        Entity::AssetGroup,
        Entity::AssetLoan,
        Entity::Page,
        Entity::PageUuid,
        Entity::ImportJob,
    ];
}

/// A value at a JSON path: a reference, or editor HTML holding some.
#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Ref(Entity),
    Html,
    /// A stored file's URL naming the ticket or asset that owns it
    /// ([`file_owner`]); the file restore moves the file to match.
    File,
}

/// Where a column's references sit.
#[derive(Debug, Clone, Copy)]
pub enum Shape {
    /// JSON paths, `.`-separated, `*` for each array element.
    Paths(&'static [(&'static str, Kind)]),
    /// A rule's condition tree: `{kind: and|or, children}`, `{kind: not,
    /// child}`, leaves `{kind: leaf, field, value}`, the value named by its
    /// field.
    Conditions(&'static [(&'static str, Entity)]),
    /// A saved view's filter, by the row's `dataset`.
    ViewFilter,
    /// `saved_views.scope_id`: a project id or a person, by the row's
    /// `scope`.
    ViewScope,
    /// Custom field values keyed by property name; the asset kind's (the
    /// row's `kind`) or the user field schema's `format` says which name
    /// people (`user-uuid`) or assets (`asset-ref`).
    Attributes(Schema),
    /// The column names a row of the type in `type_column`.
    Typed {
        type_column: &'static str,
        types: &'static [(&'static str, Entity)],
    },
    /// `knowledge_gap_signals.source_ref`, by the row's `source_kind`.
    GapSource,
    /// The column itself, as `Kind`.
    Text(Kind),
    /// A Yjs document; the state vector column is written with it.
    Yjs { state_vector: &'static str },
    /// Names something no archive carries; always cleared, not reported.
    Reset,
}

/// Which schema keys custom field values.
#[derive(Debug, Clone, Copy)]
pub enum Schema {
    AssetKind,
    UserFields,
}

/// What the import does with a row holding a reference it can't keep,
/// beyond clearing that reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    /// Nothing more.
    Clear,
    /// The rule turns draft (a live or dry-run rule).
    DraftRule,
    /// The row stays out.
    DropRow,
}

/// One registry entry.
#[derive(Debug, Clone, Copy)]
pub struct Reference {
    pub table: &'static str,
    pub column: &'static str,
    pub shape: Shape,
    pub missing: Missing,
}

const fn r(table: &'static str, column: &'static str, shape: Shape, missing: Missing) -> Reference {
    Reference {
        table,
        column,
        shape,
        missing,
    }
}

use Entity as E;
use Kind::{File, Html, Ref};

/// Rule actions (`handlers/rules.rs`, `repository/rules.rs`): `set_status`,
/// `assign` direct or to a group, `add_tags`/`remove_tags`, and a reply's
/// HTML body. `unassign`, `set_priority` and `stop_processing` carry none.
const RULE_ACTIONS: Shape = Shape::Paths(&[
    ("*.config.workflow_state_id", Ref(E::WorkflowState)),
    ("*.config.user_uuid", Ref(E::User)),
    ("*.config.group_id", Ref(E::Group)),
    ("*.config.tag_ids.*", Ref(E::Tag)),
    ("*.config.body", Html),
]);

/// Rule condition leaves whose value is an id.
const RULE_CONDITIONS: Shape = Shape::Conditions(&[
    ("ticket.workflow_state_id", E::WorkflowState),
    ("ticket.category_id", E::Category),
    ("ticket.tag_ids", E::Tag),
    ("ticket.requester_uuid", E::User),
    ("ticket.assignee_uuid", E::User),
]);

/// Ticket view predicate fields (`CardData` paths) whose value is an id.
const VIEW_FIELDS: &[(&str, Entity)] = &[
    ("assignee_uuid", E::User),
    ("requester_uuid", E::User),
    ("workflow_state.id", E::WorkflowState),
    ("category_id", E::Category),
    ("cycle_id", E::Cycle),
    ("parent_ticket_id", E::Ticket),
    ("recurrence_template_id", E::Ticket),
    ("id", E::Ticket),
    ("uuid", E::TicketUuid),
    ("tag_ids", E::Tag),
    ("affected_devices.first.id", E::Asset),
];

/// A ticket view's scope lists.
const VIEW_SCOPES: &[(&str, Entity)] = &[
    ("project_ids", E::Project),
    ("cycle_ids", E::Cycle),
    ("group_ids", E::Group),
];

/// Every column holding a reference no foreign key carries.
pub const REFERENCES: &[Reference] = &[
    // Rules and their history.
    r("rules", "actions", RULE_ACTIONS, Missing::DraftRule),
    r("rules", "conditions", RULE_CONDITIONS, Missing::DraftRule),
    r("rule_versions", "actions", RULE_ACTIONS, Missing::Clear),
    r(
        "rule_versions",
        "conditions",
        RULE_CONDITIONS,
        Missing::Clear,
    ),
    r(
        "rule_applications",
        "actions_taken",
        Shape::Paths(&[
            ("*.comment_id", Ref(E::Comment)),
            ("*.workflow_state_id", Ref(E::WorkflowState)),
            ("*.assigned_to_uuid", Ref(E::User)),
            ("*.tag_ids_added.*", Ref(E::Tag)),
            ("*.tag_ids_removed.*", Ref(E::Tag)),
        ]),
        Missing::Clear,
    ),
    // Saved views.
    r("saved_views", "filter", Shape::ViewFilter, Missing::Clear),
    r(
        "saved_views",
        "shape",
        Shape::Paths(&[("cycle_id", Ref(E::Cycle))]),
        Missing::Clear,
    ),
    r(
        "saved_views",
        "scope_id",
        Shape::ViewScope,
        Missing::DropRow,
    ),
    // Notifications.
    r(
        "notifications",
        "metadata",
        Shape::Paths(&[
            ("ticket_id", Ref(E::Ticket)),
            ("page_id", Ref(E::Page)),
            ("asset_id", Ref(E::Asset)),
        ]),
        Missing::Clear,
    ),
    r(
        "notifications",
        "entity_id",
        Shape::Typed {
            type_column: "entity_type",
            types: &[
                ("ticket", E::Ticket),
                ("comment", E::Comment),
                ("documentation_page", E::Page),
                ("asset", E::Asset),
            ],
        },
        Missing::DropRow,
    ),
    // A dedupe key into the source's activity stream, which never moves;
    // left in, it could suppress a later notification here.
    r(
        "notifications",
        "source_sync_id",
        Shape::Reset,
        Missing::Clear,
    ),
    // Knowledge gaps.
    r(
        "knowledge_gap_signals",
        "payload",
        Shape::Paths(&[
            ("ticket_ids.*", Ref(E::Ticket)),
            ("recent_ticket_ids.*", Ref(E::Ticket)),
            ("category_id", Ref(E::Category)),
            ("page_uuid", Ref(E::PageUuid)),
        ]),
        Missing::Clear,
    ),
    r(
        "knowledge_gap_signals",
        "source_ref",
        Shape::GapSource,
        Missing::DropRow,
    ),
    // Comments: the body, and the channel's record (inbound mail, merges).
    r("comments", "content", Shape::Text(Html), Missing::Clear),
    r(
        "comments",
        "channel_metadata",
        Shape::Paths(&[
            ("forwarded_by_user_uuid", Ref(E::User)),
            ("source_ticket_ids.*", Ref(E::Ticket)),
            ("sources.*.id", Ref(E::Ticket)),
            ("sources.*.requester_uuid", Ref(E::User)),
            ("merged_into_ticket_id", Ref(E::Ticket)),
            ("merged_by_user_uuid", Ref(E::User)),
        ]),
        Missing::Clear,
    ),
    r(
        "canned_responses",
        "body",
        Shape::Text(Html),
        Missing::Clear,
    ),
    // Asset media name their asset in their path, as their files do.
    r("asset_media", "url", Shape::Text(File), Missing::DropRow),
    r(
        "asset_media",
        "thumbnail_url",
        Shape::Text(File),
        Missing::Clear,
    ),
    // Assets and people's custom fields.
    r(
        "asset_lifecycle_events",
        "metadata",
        Shape::Paths(&[
            ("import_job_id", Ref(E::ImportJob)),
            ("loan_id", Ref(E::AssetLoan)),
        ]),
        Missing::Clear,
    ),
    r(
        "assets",
        "attributes",
        Shape::Attributes(Schema::AssetKind),
        Missing::Clear,
    ),
    r(
        "asset_models",
        "default_attributes",
        Shape::Attributes(Schema::AssetKind),
        Missing::Clear,
    ),
    r(
        "user_profiles",
        "custom_fields",
        Shape::Attributes(Schema::UserFields),
        Missing::Clear,
    ),
    // A soft file reference nothing writes yet (the v1.2 compliance pack).
    r(
        "asset_disposals",
        "certificate_file_id",
        Shape::Reset,
        Missing::Clear,
    ),
    // Collaborative documents and their revisions.
    r(
        "article_contents",
        "yjs_document",
        Shape::Yjs {
            state_vector: "yjs_state_vector",
        },
        Missing::Clear,
    ),
    r(
        "article_content_revisions",
        "yjs_document_content",
        Shape::Yjs {
            state_vector: "yjs_state_vector",
        },
        Missing::Clear,
    ),
    r(
        "documentation_pages",
        "yjs_document",
        Shape::Yjs {
            state_vector: "yjs_state_vector",
        },
        Missing::Clear,
    ),
    r(
        "documentation_revisions",
        "yjs_document_snapshot",
        Shape::Yjs {
            state_vector: "yjs_state_vector",
        },
        Missing::Clear,
    ),
    r(
        "documentation_collections",
        "description_yjs",
        Shape::Yjs {
            state_vector: "description_state_vector",
        },
        Missing::Clear,
    ),
];

/// JSONB, Yjs and loose id columns on carried tables that name no row the
/// import moves, and why.
pub const NO_IDS: &[(&str, &str, &str)] = &[
    // JSONB.
    (
        "asset_kinds",
        "attribute_schema",
        "a field schema; `assetKind` names a kind by slug, which the kinds keep",
    ),
    (
        "user_field_schema",
        "schema",
        "a field schema; `assetKind` names a kind by slug",
    ),
    (
        "assignment_log",
        "context",
        "the rule's name and priority as display values",
    ),
    (
        "assignment_rules",
        "conditions",
        "priority, category names and title text; targets are columns",
    ),
    (
        "channel_messages",
        "raw_metadata",
        "mail headers as received",
    ),
    ("channels", "config", "mailbox settings"),
    ("channels", "runtime_state", "IMAP uids and Message-IDs"),
    ("cycles", "completion_snapshot", "counts and category names"),
    ("email_suppressions", "metadata", "never written; always {}"),
    ("notifications", "channels_delivered", "channel names"),
    (
        "outbound_emails",
        "headers_json",
        "headers of mail as it was sent",
    ),
    (
        "plugin_activity",
        "details",
        "versions, sources and trust levels",
    ),
    (
        "plugin_collection_rows",
        "data",
        "a plugin's own data, by its own schema",
    ),
    (
        "plugin_collection_schemas",
        "schema",
        "a plugin's collection schema",
    ),
    ("plugin_data", "value", "a plugin's settings and storage"),
    ("plugins", "consented_permissions", "permission names"),
    ("plugins", "manifest", "the plugin's manifest"),
    ("plugins", "signature_metadata", "the signature envelope"),
    (
        "rule_applications",
        "actions_skipped",
        "step indexes and reasons",
    ),
    ("rule_applications", "condition_evaluation", "always NULL"),
    ("rules", "trigger_config", "always {}; nothing reads a key"),
    (
        "rule_versions",
        "trigger_config",
        "always {}; nothing reads a key",
    ),
    ("saved_views", "viz_config", "metric and grouping names"),
    (
        "site_settings",
        "email_logo",
        "a branding file path, which the file restore owns",
    ),
    (
        "site_settings",
        "email_logo_light",
        "a branding file path, which the file restore owns",
    ),
    ("site_settings", "feature_flags", "flag names and values"),
    (
        "user_auth_identities",
        "metadata",
        "the identity provider's claims",
    ),
    ("webhooks", "headers", "HTTP headers"),
    ("working_calendars", "schedule", "opening hours"),
    (
        "workspace_ldap_settings",
        "attribute_map",
        "LDAP attribute names",
    ),
    (
        "workspace_ldap_settings",
        "group_config",
        "LDAP group names and roles",
    ),
    (
        "workspace_ldap_settings",
        "provisioning",
        "a provisioning switch",
    ),
    // Yjs state vectors, written with their documents.
    (
        "article_contents",
        "yjs_state_vector",
        "the state vector of yjs_document",
    ),
    (
        "article_content_revisions",
        "yjs_state_vector",
        "the state vector of the revision",
    ),
    (
        "documentation_pages",
        "yjs_state_vector",
        "the state vector of yjs_document",
    ),
    (
        "documentation_revisions",
        "yjs_state_vector",
        "the state vector of the revision",
    ),
    (
        "documentation_collections",
        "description_state_vector",
        "the state vector of the description",
    ),
    // Other binary and loose id columns.
    (
        "article_contents",
        "yjs_client_id",
        "a Yjs client id, not a row",
    ),
    (
        "documentation_pages",
        "yjs_client_id",
        "a Yjs client id, not a row",
    ),
    ("plugins", "icon_svg", "an image"),
    ("plugins", "bundle_js", "the plugin's code"),
    ("channel_messages", "external_id", "an RFC Message-ID"),
    ("groups", "external_id", "the directory's group id"),
    ("outbound_emails", "message_id", "an RFC Message-ID"),
    (
        "outbound_emails",
        "provider_message_id",
        "the mail provider's id",
    ),
    (
        "user_auth_identities",
        "external_id",
        "the identity provider's subject",
    ),
    (
        "workspace_identity_providers",
        "client_id",
        "an OAuth client id",
    ),
];

/// Every registered column of `table`.
pub fn references_of(table: &str) -> impl Iterator<Item = &'static Reference> + '_ {
    REFERENCES.iter().filter(move |r| r.table == table)
}

// ---------------------------------------------------------------------------
// Lookups
// ---------------------------------------------------------------------------

/// Where a reference lands. Values are canonical: integers in decimal, uuids
/// lower-case and hyphenated. `None`: the target won't have it.
pub trait Lookup {
    fn user(&self, uuid: &str) -> Option<String>;
    fn key(&self, entity: Entity, key: &str) -> Option<String>;

    fn resolve(&self, entity: Entity, key: &str) -> Option<String> {
        match entity {
            Entity::User => self.user(key),
            other => self.key(other, key),
        }
    }
}

/// A value as `entity`'s canonical key, `None` when it isn't one (an empty
/// or placeholder value: `"all"`, `0`, `null`).
pub fn canonical(entity: Entity, value: &Value) -> Option<String> {
    if entity.by_uuid() {
        return value
            .as_str()
            .and_then(|s| Uuid::parse_str(s.trim()).ok())
            .map(|u| u.to_string());
    }
    let n = match value {
        Value::Number(n) => n.as_i64()?,
        Value::String(s) => s.trim().parse::<i64>().ok()?,
        _ => return None,
    };
    (n > 0).then(|| n.to_string())
}

/// `new` with the JSON type `old` had.
fn retyped(old: &Value, new: &str) -> Value {
    match old {
        Value::Number(_) => new
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(new.to_string())),
        _ => Value::String(new.to_string()),
    }
}

/// One reference through the lookup.
enum Outcome {
    /// Not a reference; left as it is.
    NotARef,
    Kept(Value),
    Missing,
}

fn map(entity: Entity, value: &Value, lookup: &dyn Lookup) -> Outcome {
    let Some(key) = canonical(entity, value) else {
        return Outcome::NotARef;
    };
    match lookup.resolve(entity, &key) {
        Some(new) => Outcome::Kept(retyped(value, &new)),
        None => Outcome::Missing,
    }
}

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

/// What the walker reads from other tables: which custom fields name people
/// or assets.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// Asset kind slug to its property names and what each names.
    pub asset_kinds: HashMap<String, Vec<(String, Entity)>>,
    pub user_fields: Vec<(String, Entity)>,
}

impl Context {
    /// From the archive's `asset_kinds` and `user_field_schema` rows.
    pub fn from_rows<'a>(
        asset_kinds: impl IntoIterator<Item = &'a Row>,
        user_field_schema: impl IntoIterator<Item = &'a Row>,
    ) -> Self {
        let mut ctx = Context::default();
        for row in asset_kinds {
            if let (Some(slug), Some(schema)) = (
                row.get("slug").and_then(Value::as_str),
                row.get("attribute_schema"),
            ) {
                ctx.asset_kinds
                    .insert(slug.to_string(), reference_fields(schema));
            }
        }
        for row in user_field_schema {
            if let Some(schema) = row.get("schema") {
                ctx.user_fields.extend(reference_fields(schema));
            }
        }
        ctx
    }
}

/// The properties of a field schema whose values (or array items) are
/// people or assets.
fn reference_fields(schema: &Value) -> Vec<(String, Entity)> {
    let Some(props) = schema.get("properties").and_then(Value::as_object) else {
        return Vec::new();
    };
    let entity = |prop: &Value| match prop.get("format").and_then(Value::as_str) {
        Some("user-uuid") => Some(E::User),
        Some("asset-ref") => Some(E::Asset),
        _ => None,
    };
    props
        .iter()
        .filter_map(|(name, prop)| {
            entity(prop)
                .or_else(|| prop.get("items").and_then(entity))
                .map(|e| (name.clone(), e))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The report
// ---------------------------------------------------------------------------

/// What happened to references the import couldn't keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Values cleared (set to null, or left out of a list).
    Cleared,
    /// Rules that turned draft.
    RuleDrafted,
    /// View predicates or scope entries dropped.
    PredicateDropped,
    /// Rows left out.
    RowDropped,
    /// Mentions now their "@name".
    MentionAsText,
    /// Ticket links now their link text.
    TicketLinkAsText,
    /// Notes or asset images now their alt text.
    ImageAsText,
    /// Embedded pages now their title.
    EmbedAsText,
    /// Documents that don't decode, loaded as they are.
    NotADocument,
}

/// One line of the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReferenceReport {
    pub table: String,
    pub column: String,
    pub effect: Effect,
    pub count: usize,
}

/// Tallies by table, column and effect.
#[derive(Debug, Default)]
pub struct Tally(BTreeMap<(String, String, Effect), usize>);

impl Tally {
    fn add(&mut self, table: &str, column: &str, effect: Effect, n: usize) {
        if n > 0 {
            *self
                .0
                .entry((table.to_string(), column.to_string(), effect))
                .or_default() += n;
        }
    }

    fn add_counts(&mut self, table: &str, column: &str, counts: &Counts) {
        self.add(
            table,
            column,
            Effect::MentionAsText,
            counts.mentions_as_text,
        );
        self.add(
            table,
            column,
            Effect::TicketLinkAsText,
            counts.ticket_links_as_text,
        );
        self.add(table, column, Effect::ImageAsText, counts.images_as_text);
        self.add(table, column, Effect::EmbedAsText, counts.embeds_as_text);
    }

    pub fn into_reports(self) -> Vec<ReferenceReport> {
        self.0
            .into_iter()
            .map(|((table, column, effect), count)| ReferenceReport {
                table,
                column,
                effect,
                count,
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// The walker
// ---------------------------------------------------------------------------

/// What one column's walk found.
#[derive(Default)]
struct Found {
    /// References cleared or left out.
    cleared: usize,
    /// View predicates and scope entries dropped.
    predicates: usize,
    /// Nodes in HTML or a document that became text.
    text: Counts,
    not_a_document: bool,
}

impl Found {
    fn missing(&self) -> bool {
        self.lost() || self.text.unresolved() > 0
    }

    /// A reference itself went, not just a node turned to text: what drafts
    /// a rule or leaves a row out.
    fn lost(&self) -> bool {
        self.cleared + self.predicates > 0
    }
}

/// Rewrite every registered reference in one row of `table` through
/// `lookup`. `Some(reason)` when the row stays out. With a `tally`, what
/// couldn't be kept is counted.
pub fn rewrite_row(
    table: &str,
    row: &mut Row,
    ctx: &Context,
    lookup: &dyn Lookup,
    mut tally: Option<&mut Tally>,
) -> Option<&'static str> {
    let mut draft = false;
    let mut drop = false;
    for reference in references_of(table) {
        let found = rewrite_column(reference, row, ctx, lookup);
        if !found.missing() && !found.not_a_document {
            continue;
        }
        match reference.missing {
            Missing::DraftRule if found.lost() => draft = true,
            Missing::DropRow if found.lost() => drop = true,
            _ => {}
        }
        if let Some(tally) = tally.as_deref_mut() {
            let (t, c) = (reference.table, reference.column);
            if reference.missing != Missing::DropRow {
                tally.add(t, c, Effect::Cleared, found.cleared);
            }
            tally.add(t, c, Effect::PredicateDropped, found.predicates);
            tally.add_counts(t, c, &found.text);
            tally.add(
                t,
                c,
                Effect::NotADocument,
                usize::from(found.not_a_document),
            );
            if reference.missing == Missing::DropRow && found.lost() {
                tally.add(t, c, Effect::RowDropped, 1);
            }
        }
    }
    if drop {
        return Some(super::plan::UNRESOLVED_REFERENCE);
    }
    if draft {
        let state = row.get("state").and_then(Value::as_str);
        if matches!(state, Some("live" | "dry_run")) {
            row.insert("state".into(), Value::String("draft".into()));
            if let Some(tally) = tally {
                tally.add(table, "state", Effect::RuleDrafted, 1);
            }
        }
    }
    None
}

fn rewrite_column(
    reference: &Reference,
    row: &mut Row,
    ctx: &Context,
    lookup: &dyn Lookup,
) -> Found {
    let mut found = Found::default();
    let column = reference.column;
    if row.get(column).is_none_or(Value::is_null) {
        return found;
    }
    match reference.shape {
        Shape::Paths(paths) => {
            let Some(value) = row.get_mut(column) else {
                return found;
            };
            for (path, kind) in paths {
                let segments: Vec<&str> = path.split('.').collect();
                walk_path(value, &segments, *kind, lookup, &mut found);
            }
        }
        Shape::Conditions(fields) => {
            if let Some(value) = row.get_mut(column) {
                walk_conditions(value, fields, lookup, &mut found);
            }
        }
        Shape::ViewFilter => {
            let dataset = row
                .get("dataset")
                .and_then(Value::as_str)
                .unwrap_or("tickets")
                .to_string();
            if let Some(value) = row.get_mut(column) {
                view_filter(&dataset, value, lookup, &mut found);
            }
        }
        Shape::ViewScope => {
            let entity = match row.get("scope").and_then(Value::as_str) {
                Some("project") => Some(E::Project),
                Some("private") => Some(E::User),
                _ => None,
            };
            if let Some(entity) = entity {
                scalar(row, column, entity, lookup, &mut found);
            }
        }
        Shape::Attributes(schema) => {
            let fields: &[(String, Entity)] = match schema {
                Schema::UserFields => &ctx.user_fields,
                Schema::AssetKind => row
                    .get("kind")
                    .and_then(Value::as_str)
                    .and_then(|k| ctx.asset_kinds.get(k))
                    .map_or(&[], Vec::as_slice),
            };
            let fields = fields.to_vec();
            if let Some(Value::Object(values)) = row.get_mut(column) {
                for (name, entity) in fields {
                    if let Some(value) = values.get_mut(&name) {
                        leaf(value, Ref(entity), lookup, &mut found, true);
                    }
                }
            }
        }
        Shape::Typed { type_column, types } => {
            let kind = row.get(type_column).and_then(Value::as_str);
            if let Some((_, entity)) = types.iter().find(|(t, _)| Some(*t) == kind) {
                scalar(row, column, *entity, lookup, &mut found);
            }
        }
        Shape::GapSource => gap_source(row, column, lookup, &mut found),
        Shape::Text(kind) => {
            if let Some(value) = row.get_mut(column) {
                leaf(value, kind, lookup, &mut found, false);
            }
        }
        Shape::Yjs { state_vector } => {
            yjs_column(row, column, state_vector, lookup, &mut found);
        }
        Shape::Reset => {
            row.insert(column.to_string(), Value::Null);
        }
    }
    found
}

/// A column that is one reference: kept, or cleared.
fn scalar(row: &mut Row, column: &str, entity: Entity, lookup: &dyn Lookup, found: &mut Found) {
    let Some(value) = row.get(column) else {
        return;
    };
    match map(entity, value, lookup) {
        Outcome::NotARef => {}
        Outcome::Kept(new) => {
            row.insert(column.to_string(), new);
        }
        Outcome::Missing => {
            row.insert(column.to_string(), Value::Null);
            found.cleared += 1;
        }
    }
}

/// A value that is a reference (or an array of them, when `arrays`), or
/// HTML: rewritten in place. A reference that can't be kept becomes null;
/// in an array it is left out.
fn leaf(value: &mut Value, kind: Kind, lookup: &dyn Lookup, found: &mut Found, arrays: bool) {
    match (kind, value) {
        (Html, Value::String(html)) => {
            let (new, counts) = rewrite_html(html, lookup);
            found.text.add(&counts);
            *html = new;
        }
        (Html, _) => {}
        (File, value) => {
            let Value::String(path) = &*value else {
                return;
            };
            match rewrite_file_path(path, lookup) {
                FilePath::NotOwned => {}
                FilePath::Kept(new) => *value = Value::String(new),
                FilePath::Missing => {
                    *value = Value::Null;
                    found.cleared += 1;
                }
            }
        }
        (Ref(entity), Value::Array(items)) if arrays => {
            items.retain_mut(|item| keep(item, entity, lookup, found));
        }
        (Ref(entity), value) => {
            if !keep(value, entity, lookup, found) {
                *value = Value::Null;
            }
        }
    }
}

/// Rewrite one reference; false when it can't be kept (counted).
fn keep(value: &mut Value, entity: Entity, lookup: &dyn Lookup, found: &mut Found) -> bool {
    match map(entity, value, lookup) {
        Outcome::NotARef => true,
        Outcome::Kept(new) => {
            *value = new;
            true
        }
        Outcome::Missing => {
            found.cleared += 1;
            false
        }
    }
}

fn walk_path(value: &mut Value, path: &[&str], kind: Kind, lookup: &dyn Lookup, found: &mut Found) {
    let Some((head, rest)) = path.split_first() else {
        return;
    };
    if *head == "*" {
        let Value::Array(items) = value else {
            return;
        };
        if rest.is_empty() {
            match kind {
                Ref(entity) => items.retain_mut(|item| keep(item, entity, lookup, found)),
                Html | File => items
                    .iter_mut()
                    .for_each(|item| leaf(item, kind, lookup, found, false)),
            }
        } else {
            for item in items {
                walk_path(item, rest, kind, lookup, found);
            }
        }
        return;
    }
    let Some(child) = value.as_object_mut().and_then(|o| o.get_mut(*head)) else {
        return;
    };
    if rest.is_empty() {
        if !child.is_null() {
            leaf(child, kind, lookup, found, false);
        }
    } else {
        walk_path(child, rest, kind, lookup, found);
    }
}

fn walk_conditions(
    value: &mut Value,
    fields: &[(&str, Entity)],
    lookup: &dyn Lookup,
    found: &mut Found,
) {
    match value {
        Value::Array(nodes) => {
            for node in nodes {
                walk_conditions(node, fields, lookup, found);
            }
        }
        Value::Object(node) => {
            if node.get("kind").and_then(Value::as_str) == Some("leaf") {
                let field = node.get("field").and_then(Value::as_str).unwrap_or("");
                if let Some((_, entity)) = fields.iter().find(|(f, _)| *f == field) {
                    if let Some(v) = node.get_mut("value") {
                        leaf(v, Ref(*entity), lookup, found, true);
                    }
                }
                return;
            }
            for key in ["children", "child"] {
                if let Some(child) = node.get_mut(key) {
                    walk_conditions(child, fields, lookup, found);
                }
            }
        }
        _ => {}
    }
}

/// A saved view's filter. Tickets: a predicate leaf naming a row the
/// target won't have is dropped, whole (narrowing a list of values would
/// change what it means), and a scope list loses the entry, falling back
/// to `all` when none is left. Assets: the `groups` list loses the group;
/// an `import` (CSV import history, never carried) is dropped.
fn view_filter(dataset: &str, value: &mut Value, lookup: &dyn Lookup, found: &mut Found) {
    let Value::Object(filter) = value else {
        return;
    };
    match dataset {
        "tickets" => {
            if let Some(predicate) = filter.get_mut("predicate") {
                view_predicate(predicate, lookup, found);
            }
            if let Some(Value::Object(scope)) = filter.get_mut("scope") {
                for (key, entity) in VIEW_SCOPES {
                    let Some(Value::Array(ids)) = scope.get_mut(*key) else {
                        continue;
                    };
                    let before = ids.len();
                    let mut dropped = Found::default();
                    ids.retain_mut(|id| keep(id, *entity, lookup, &mut dropped));
                    found.predicates += dropped.cleared;
                    if before > 0 && ids.is_empty() {
                        scope.insert((*key).to_string(), Value::String("all".into()));
                    }
                }
            }
        }
        "assets" => {
            if let Some(Value::String(csv)) = filter.get("groups").cloned() {
                let mut kept = Vec::new();
                for id in csv.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    match map(E::AssetGroup, &Value::String(id.to_string()), lookup) {
                        Outcome::Kept(Value::String(new)) => kept.push(new),
                        Outcome::Kept(other) => kept.push(other.to_string()),
                        Outcome::NotARef => kept.push(id.to_string()),
                        Outcome::Missing => found.predicates += 1,
                    }
                }
                if kept.is_empty() {
                    filter.remove("groups");
                } else {
                    filter.insert("groups".into(), Value::String(kept.join(",")));
                }
            }
            if let Some(import) = filter.get("import") {
                if let Outcome::Missing = map(E::ImportJob, import, lookup) {
                    filter.remove("import");
                    found.predicates += 1;
                }
            }
        }
        _ => {}
    }
}

/// A predicate group's leaves; one naming a row the target won't have is
/// dropped.
fn view_predicate(group: &mut Value, lookup: &dyn Lookup, found: &mut Found) {
    let Some(Value::Array(children)) = group.get_mut("children") else {
        return;
    };
    children.retain_mut(|child| {
        if child.get("children").is_some() {
            view_predicate(child, lookup, found);
            return true;
        }
        let field = child.get("field").and_then(Value::as_str).unwrap_or("");
        let Some((_, entity)) = VIEW_FIELDS.iter().find(|(f, _)| *f == field) else {
            return true;
        };
        let Some(value) = child.get_mut("value") else {
            return true;
        };
        let mut inner = Found::default();
        leaf(value, Ref(*entity), lookup, &mut inner, true);
        if inner.cleared > 0 {
            found.predicates += 1;
            return false;
        }
        true
    });
}

/// `knowledge_gap_signals.source_ref`: a ticket or page id, or a cluster
/// fingerprint `category:{id}|…`, by `source_kind`.
fn gap_source(row: &mut Row, column: &str, lookup: &dyn Lookup, found: &mut Found) {
    match row.get("source_kind").and_then(Value::as_str) {
        Some("ticket") => scalar(row, column, E::Ticket, lookup, found),
        Some("page") => scalar(row, column, E::Page, lookup, found),
        Some("cluster_key") => {
            let Some(Value::String(key)) = row.get(column) else {
                return;
            };
            let mut missing = false;
            let parts: Vec<String> = key
                .split('|')
                .map(|part| match part.split_once(':') {
                    Some(("category", id)) => {
                        match map(E::Category, &Value::String(id.to_string()), lookup) {
                            Outcome::Kept(new) => {
                                format!("category:{}", new.as_str().unwrap_or(id))
                            }
                            Outcome::NotARef => part.to_string(),
                            Outcome::Missing => {
                                missing = true;
                                part.to_string()
                            }
                        }
                    }
                    _ => part.to_string(),
                })
                .collect();
            if missing {
                found.cleared += 1;
            } else {
                row.insert(column.to_string(), Value::String(parts.join("|")));
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// HTML
// ---------------------------------------------------------------------------

fn regex(pattern: &str) -> Regex {
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("reference pattern")
}

/// The editor's `ticket_link` node (`schema.ts` `toDOM`): an empty span.
static TICKET_LINK: LazyLock<Regex> =
    LazyLock::new(|| regex(r#"(?s)<span\b[^>]*\bdata-ticket-link\b[^>]*>.*?</span>"#));
/// The editor's `mention` node: a span holding "@name".
static MENTION: LazyLock<Regex> =
    LazyLock::new(|| regex(r#"(?s)<span\b[^>]*\bdata-mention="true"[^>]*>(.*?)</span>"#));
/// The editor's `embedded_document` node: a div holding the title.
static EMBED: LazyLock<Regex> =
    LazyLock::new(|| regex(r#"(?s)<div\b[^>]*\bdata-embedded-document\b[^>]*>(.*?)</div>"#));
static IMG: LazyLock<Regex> = LazyLock::new(|| regex(r#"<img\b[^>]*>"#));
/// A plain-text mention (`MentionInput`, saved replies): `@[Name](uuid)`.
static MARKDOWN_MENTION: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"@\[([^\]]+)\]\(([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})\)",
    )
});
/// An asset photo: `…/assets/{id}/media/…`.
static ASSET_MEDIA: LazyLock<Regex> = LazyLock::new(|| regex(r"(/assets/)(\d{1,10})(/media/)"));
static TAG: LazyLock<Regex> = LazyLock::new(|| regex(r"<[^>]+>"));

/// The attributes the HTML pass reads, each as `\s+name="value"`.
static ATTRS: LazyLock<HashMap<&'static str, Regex>> = LazyLock::new(|| {
    [
        "data-ticket-id",
        "data-href",
        "data-uuid",
        "data-name",
        "data-avatar-url",
        "data-document-uuid",
        "src",
        "alt",
    ]
    .into_iter()
    .map(|name| {
        (
            name,
            regex(&format!(r#"\s+{}="([^"]*)""#, regex::escape(name))),
        )
    })
    .collect()
});

/// The value of `name` in an HTML tag.
fn html_attr(tag: &str, name: &str) -> Option<String> {
    ATTRS.get(name)?.captures(tag).map(|c| c[1].to_string())
}

/// `tag` with `name`'s value replaced.
fn with_html_attr(tag: &str, name: &str, value: &str) -> String {
    match ATTRS.get(name) {
        Some(re) => re
            .replacen(tag, 1, regex::NoExpand(&format!(r#" {name}="{value}""#)))
            .into_owned(),
        None => tag.to_string(),
    }
}

/// `tag` without `name`.
fn without_html_attr(tag: &str, name: &str) -> String {
    match ATTRS.get(name) {
        Some(re) => re.replacen(tag, 1, "").into_owned(),
        None => tag.to_string(),
    }
}

/// Editor HTML (a comment, a saved reply, a rule's reply) with its ticket
/// links, mentions, embedded pages and notes or asset images rewritten, as
/// the Yjs pass rewrites a document.
pub fn rewrite_html(html: &str, lookup: &dyn Lookup) -> (String, Counts) {
    let mut counts = Counts::default();
    let out = TICKET_LINK.replace_all(html, |c: &Captures| {
        let el = &c[0];
        let Some(old) = html_attr(el, "data-ticket-id") else {
            return el.to_string();
        };
        match map(E::Ticket, &Value::String(old.clone()), lookup) {
            Outcome::NotARef => el.to_string(),
            Outcome::Kept(new) => {
                let new = new.as_str().unwrap_or(&old).to_string();
                if new == old {
                    return el.to_string();
                }
                counts.ticket_links += 1;
                let el = with_html_attr(el, "data-ticket-id", &new);
                match new.parse::<i32>() {
                    Ok(id) => with_html_attr(
                        &el,
                        "data-href",
                        &crate::utils::ticket_link::ticket_route_by_id(id),
                    ),
                    Err(_) => el,
                }
            }
            Outcome::Missing => {
                counts.ticket_links_as_text += 1;
                html_attr(el, "data-href")
                    .filter(|h| !h.is_empty())
                    .unwrap_or_else(|| format!("#{old}"))
            }
        }
    });
    let out = MENTION.replace_all(&out, |c: &Captures| {
        let el = &c[0];
        let Some(old) = html_attr(el, "data-uuid") else {
            return el.to_string();
        };
        match map(E::User, &Value::String(old.clone()), lookup) {
            Outcome::NotARef => el.to_string(),
            Outcome::Kept(new) => {
                let new = new.as_str().unwrap_or(&old).to_string();
                if new.eq_ignore_ascii_case(&old) {
                    return el.to_string();
                }
                counts.mentions += 1;
                let el = with_html_attr(el, "data-uuid", &new);
                without_html_attr(&el, "data-avatar-url")
            }
            Outcome::Missing => {
                counts.mentions_as_text += 1;
                let inner = TAG.replace_all(&c[1], "").trim().to_string();
                if inner.is_empty() {
                    html_attr(el, "data-name")
                        .map(|n| format!("@{n}"))
                        .unwrap_or_default()
                } else {
                    inner
                }
            }
        }
    });
    let out = MARKDOWN_MENTION.replace_all(&out, |c: &Captures| {
        match map(E::User, &Value::String(c[2].to_string()), lookup) {
            Outcome::NotARef => c[0].to_string(),
            Outcome::Kept(new) => {
                let new = new.as_str().unwrap_or(&c[2]).to_string();
                if !new.eq_ignore_ascii_case(&c[2]) {
                    counts.mentions += 1;
                }
                format!("@[{}]({new})", &c[1])
            }
            Outcome::Missing => {
                counts.mentions_as_text += 1;
                format!("@{}", &c[1])
            }
        }
    });
    let out = EMBED.replace_all(&out, |c: &Captures| {
        let el = &c[0];
        let Some(old) = html_attr(el, "data-document-uuid") else {
            return el.to_string();
        };
        match map(E::PageUuid, &Value::String(old.clone()), lookup) {
            Outcome::NotARef => el.to_string(),
            Outcome::Kept(new) => {
                let new = new.as_str().unwrap_or(&old).to_string();
                if new.eq_ignore_ascii_case(&old) {
                    return el.to_string();
                }
                counts.embeds += 1;
                with_html_attr(el, "data-document-uuid", &new)
            }
            Outcome::Missing => {
                counts.embeds_as_text += 1;
                format!("<p>{}</p>", c[1].trim())
            }
        }
    });
    let out = IMG.replace_all(&out, |c: &Captures| {
        let el = &c[0];
        let Some(src) = html_attr(el, "src") else {
            return el.to_string();
        };
        match rewrite_file_path(&src, lookup) {
            FilePath::NotOwned => el.to_string(),
            FilePath::Kept(new) => {
                if new == src {
                    return el.to_string();
                }
                counts.images += 1;
                with_html_attr(el, "src", &new)
            }
            FilePath::Missing => {
                counts.images_as_text += 1;
                html_attr(el, "alt").unwrap_or_default()
            }
        }
    });
    (out.into_owned(), counts)
}

/// The row a stored file's path or URL names as its owner: a notes image
/// (`tickets/{id}/notes/`, under `/uploads/` or `/api/files/`) follows its
/// ticket, an asset's media (`/assets/{id}/media/`) its asset. The same
/// test reads an editor's image `src`, a document's `image.src` (the Yjs
/// pass) and a storage key with a leading `/`, so the file restore moves a
/// file exactly where the references to it now point.
pub fn file_owner(path: &str) -> Option<(Entity, &str)> {
    if let Some(id) = yjs::notes_image_ticket(path) {
        return Some((E::Ticket, id));
    }
    ASSET_MEDIA
        .captures(path)
        .and_then(|m| m.get(2))
        .map(|m| (E::Asset, m.as_str()))
}

/// `path` with the owner [`file_owner`] found replaced by `new`.
pub fn with_file_owner(path: &str, entity: Entity, new: &str) -> String {
    match entity {
        E::Ticket => yjs::with_notes_image_ticket(path, new),
        E::Asset => ASSET_MEDIA
            .replacen(path, 1, |c: &Captures| format!("{}{new}{}", &c[1], &c[3]))
            .into_owned(),
        _ => path.to_string(),
    }
}

/// A stored file's path or URL through the lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilePath {
    /// It names no owner; left as it is.
    NotOwned,
    /// Rewritten to its owner's key on the target.
    Kept(String),
    /// Its owner isn't imported.
    Missing,
}

/// `path` with its owner ([`file_owner`]) rewritten through `lookup`.
pub fn rewrite_file_path(path: &str, lookup: &dyn Lookup) -> FilePath {
    let Some((entity, old)) = file_owner(path) else {
        return FilePath::NotOwned;
    };
    match map(entity, &Value::String(old.to_string()), lookup) {
        Outcome::NotARef => FilePath::NotOwned,
        Outcome::Kept(new) => {
            let new = match &new {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            FilePath::Kept(with_file_owner(path, entity, &new))
        }
        Outcome::Missing => FilePath::Missing,
    }
}

// ---------------------------------------------------------------------------
// Yjs
// ---------------------------------------------------------------------------

/// A bytea value as an archive row holds it (`row_to_json`: `\x` and hex).
pub fn bytea(value: &Value) -> Option<Vec<u8>> {
    let hex_text = value.as_str()?.strip_prefix("\\x")?;
    hex::decode(hex_text).ok()
}

/// Bytes as an archive row holds a bytea value.
pub fn bytea_value(bytes: &[u8]) -> Value {
    Value::String(format!("\\x{}", hex::encode(bytes)))
}

/// The lookup as the Yjs pass's targets.
struct YjsTargets<'a>(&'a dyn Lookup);

impl Targets for YjsTargets<'_> {
    fn ticket(&self, id: &str) -> Option<String> {
        match map(E::Ticket, &Value::String(id.to_string()), self.0) {
            Outcome::NotARef => Some(id.to_string()),
            Outcome::Kept(new) => new.as_str().map(str::to_string),
            Outcome::Missing => None,
        }
    }
    fn user(&self, uuid: &str) -> Option<String> {
        match map(E::User, &Value::String(uuid.to_string()), self.0) {
            Outcome::NotARef => Some(uuid.to_string()),
            Outcome::Kept(new) => new.as_str().map(str::to_string),
            Outcome::Missing => None,
        }
    }
    fn page(&self, uuid: &str) -> Option<String> {
        match map(E::PageUuid, &Value::String(uuid.to_string()), self.0) {
            Outcome::NotARef => Some(uuid.to_string()),
            Outcome::Kept(new) => new.as_str().map(str::to_string),
            Outcome::Missing => None,
        }
    }
}

fn yjs_column(
    row: &mut Row,
    column: &str,
    state_vector: &str,
    lookup: &dyn Lookup,
    found: &mut Found,
) {
    let Some(bytes) = row.get(column).and_then(bytea) else {
        found.not_a_document = true;
        return;
    };
    match yjs::rewrite(&bytes, &YjsTargets(lookup)) {
        Rewrite::Unchanged => {}
        Rewrite::NotADocument => found.not_a_document = true,
        Rewrite::Rewritten {
            update,
            state_vector: vector,
            counts,
        } => {
            row.insert(column.to_string(), bytea_value(&update));
            row.insert(state_vector.to_string(), bytea_value(&vector));
            // The id of the last client to write it, which is no longer so.
            if row.contains_key("yjs_client_id") {
                row.insert("yjs_client_id".into(), Value::Null);
            }
            found.text = counts;
        }
    }
}

/// Every JSONB column a registry entry or [`NO_IDS`] names, for the
/// completeness test.
pub fn classified() -> HashSet<(&'static str, &'static str)> {
    REFERENCES
        .iter()
        .map(|r| (r.table, r.column))
        .chain(NO_IDS.iter().map(|(t, c, _)| (*t, *c)))
        .collect()
}

#[cfg(test)]
mod tests;
