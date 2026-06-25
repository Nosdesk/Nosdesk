//! LDAP group sync: import directory groups + their membership.
//!
//! AD lists a group's members by DN, but our identities key on objectGUID, so
//! membership is resolved through a DN->user_uuid map built from the DN the user
//! sync persists on each identity. This therefore runs AFTER the user sync so
//! the map is complete. Groups are mirrored into the `groups` tables via the
//! shared external-group sync surface (the same one the Microsoft Graph sync
//! uses), and groups that vanished from the directory are marked not-synced.
//!
//! v1 does a full paged scan each run (groups are far fewer than users and
//! change rarely); DirSync for groups is a possible later refinement.

use std::collections::{HashMap, HashSet};

use ldap3::adapters::{Adapter, EntriesOnly, PagedResults};
use ldap3::{Scope, SearchEntry};
use serde_json::Value;
use tracing::warn;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::WorkspaceLdapSettings;
use crate::repository::{groups as groups_repo, user_auth_identities};
use crate::services::ldap::attrs::{all_values, attr_name, first_value};
use crate::services::ldap::connector;
use crate::services::ldap::sync::{connect_and_bind, SyncError};

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct GroupSyncStats {
    pub groups_seen: usize,
    pub groups_synced: usize,
    pub members_resolved: usize,
    /// Member DNs that didn't map to a known user (not yet synced, or a nested
    /// group / contact rather than a user).
    pub members_unresolved: usize,
}

fn cfg<'a>(settings: &'a WorkspaceLdapSettings, key: &str, default: &'a str) -> &'a str {
    settings
        .group_config
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(default)
}

/// Group sync is enabled once a group base DN is configured.
pub fn is_configured(settings: &WorkspaceLdapSettings) -> bool {
    settings
        .group_config
        .get("group_base_dn")
        .and_then(Value::as_str)
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

/// Sync groups + membership for `workspace_id`. `conn` MUST be pinned to it.
pub async fn sync_groups(
    conn: &mut DbConnection,
    settings: &WorkspaceLdapSettings,
    workspace_id: i32,
    bind_password: &str,
) -> Result<GroupSyncStats, SyncError> {
    let base = cfg(settings, "group_base_dn", &settings.user_base_dn).to_string();
    let object_class = cfg(settings, "object_class", "group");
    let member_attr = cfg(settings, "member_attribute", "member");
    let name_attr = cfg(settings, "name_attribute", "cn");
    let ext_id_attr = attr_name(&settings.attribute_map, "external_id", "objectGUID");
    let object_sid_attr = attr_name(&settings.attribute_map, "object_sid", "objectSid");
    let filter = format!("(objectClass={object_class})");

    // DN -> user_uuid (lowercased) for explicit `member` entries; primary-group
    // SID -> [user_uuid] for the primary group AD omits from that list.
    let dn_map: HashMap<String, Uuid> = user_auth_identities::ldap_dn_map(conn, workspace_id)?
        .into_iter()
        .collect();
    let mut primary_map: HashMap<String, Vec<Uuid>> = HashMap::new();
    for (sid, uuid) in user_auth_identities::ldap_primary_group_members(conn, workspace_id)? {
        primary_map.entry(sid).or_default().push(uuid);
    }

    let mut svc = connect_and_bind(settings, bind_password).await?;
    let page = settings.page_size.max(1) as i32;
    let attrs: Vec<&str> = vec![
        ext_id_attr.as_str(),
        name_attr,
        member_attr,
        object_sid_attr.as_str(),
    ];
    let adapters: Vec<Box<dyn Adapter<_, _>>> = vec![
        Box::new(EntriesOnly::new()),
        Box::new(PagedResults::new(page)),
    ];
    let mut stream = svc
        .with_timeout(connector::OP_TIMEOUT)
        .streaming_search_with(adapters, &base, Scope::Subtree, &filter, attrs)
        .await?;

    let mut stats = GroupSyncStats::default();
    let mut synced_ext_ids: Vec<String> = Vec::new();
    while let Some(re) = stream.next().await? {
        stats.groups_seen += 1;
        let entry = SearchEntry::construct(re);
        let Some(ext_id) = first_value(&entry, &ext_id_attr) else {
            continue;
        };
        let name = first_value(&entry, name_attr).unwrap_or_else(|| ext_id.clone());

        let group = match groups_repo::upsert_external_group(
            conn,
            &ext_id,
            "ldap",
            &name,
            None, // description
            Some("security"),
            false, // mail_enabled
            true,  // security_enabled
        ) {
            Ok((g, _)) => g,
            Err(e) => {
                warn!(error = %e, group = %name, "ldap group sync: upsert failed");
                continue;
            }
        };
        synced_ext_ids.push(ext_id);

        // Explicit members: resolve each member DN to a user_uuid via the map.
        let mut members: HashSet<Uuid> = HashSet::new();
        for dn in all_values(&entry, member_attr) {
            match dn_map.get(&dn.to_lowercase()) {
                Some(uuid) => {
                    members.insert(*uuid);
                    stats.members_resolved += 1;
                }
                None => stats.members_unresolved += 1,
            }
        }
        // Primary-group members (e.g. Domain Users): users whose primaryGroupID
        // resolves to THIS group's SID, which AD leaves out of `member`.
        if let Some(sid) = first_value(&entry, &object_sid_attr) {
            if let Some(primaries) = primary_map.get(&sid) {
                for &uuid in primaries {
                    if members.insert(uuid) {
                        stats.members_resolved += 1;
                    }
                }
            }
        }
        let member_uuids: Vec<Uuid> = members.into_iter().collect();
        if let Err(e) = groups_repo::set_group_members(conn, group.id, member_uuids, None) {
            warn!(error = %e, group = %name, "ldap group sync: set members failed");
        }
        stats.groups_synced += 1;
    }
    let _ = stream.finish().await;

    // Groups no longer present in the directory: mark not-synced (kept, not
    // deleted, so any manual membership / metadata survives).
    let ext_id_refs: Vec<&str> = synced_ext_ids.iter().map(String::as_str).collect();
    if let Err(e) = groups_repo::mark_groups_not_synced(conn, "ldap", &ext_id_refs) {
        warn!(error = %e, "ldap group sync: mark-not-synced failed");
    }

    Ok(stats)
}
