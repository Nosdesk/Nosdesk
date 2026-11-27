//! The API fill: what an export lacks, asked of Zendesk by id when the
//! operator gives a credential. Standard endpoints only (Show Many Users,
//! Show Many Organizations, the group and brand lists, a ticket's
//! comments, the credential's own user), never an incremental export. One
//! request at a time, since the fill shares the customer's live API quota;
//! the client waits out a 429 for as long as `Retry-After` says.
//!
//! A request that fails costs what it asked for and the fill carries on.
//! Only a refused credential (401), a request the outbound policy refuses,
//! or a throttle that outlasts the retry budget stops it: every request
//! after would fail the same way.

use std::collections::{HashMap, HashSet};

use tokio_util::sync::CancellationToken;

use super::{api, client, map, ZendeskSource};
use crate::services::imports::helpdesk::source::SourceError;

/// Ids per Show Many request: Zendesk's limit.
pub const SHOW_MANY: usize = 100;

/// What asking by id came to.
#[derive(Debug)]
pub struct ById<T> {
    pub returned: Vec<T>,
    /// Asked for in a request Zendesk answered, and not returned. Only
    /// when the answer can confirm it (see [`ZendeskSource::admin_credential`]).
    pub gone: Vec<i64>,
    /// Requests that failed, in words; their ids are neither returned nor
    /// gone.
    pub errors: Vec<String>,
    /// Why the fill stopped, when it did; the ids not asked for by then
    /// are neither returned nor gone.
    pub stopped: Option<String>,
}

/// A fill failure in words for the operator. A refused credential keeps the
/// message the API fetch gives.
pub fn failure(e: &SourceError) -> String {
    match e {
        SourceError::Unauthorized(why) if !is_forbidden(e) => format!(
            "Zendesk refused the credential ({why}). OAuth tokens expire (30 minutes by \
             default): give a fresh one and run again with --refetch"
        ),
        other => other.to_string(),
    }
}

/// A 403: this record is beyond the credential, not the credential refused.
fn is_forbidden(e: &SourceError) -> bool {
    matches!(e, SourceError::Unauthorized(m) if m.contains("answered 403"))
}

/// Whether `e` stops the whole fill rather than costing one request.
pub fn stops_the_fill(e: &SourceError) -> bool {
    match e {
        SourceError::Unauthorized(_) => !is_forbidden(e),
        SourceError::Network(m) => {
            m.starts_with(client::EGRESS_REFUSED) || m.contains(client::RATE_LIMITED)
        }
        SourceError::Io(_) | SourceError::Cancelled => true,
        SourceError::Protocol(_) => false,
    }
}

impl ZendeskSource {
    /// Whether the credential is an admin's, read from its own user. Only
    /// an admin sees every user, so only an admin's answer that leaves
    /// someone out says they are gone; an agent's may leave out people it
    /// can't see. `Err` only when the fill must stop.
    pub(super) async fn admin_credential(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Result<bool, String>, SourceError> {
        #[derive(serde::Deserialize)]
        struct Me {
            user: Option<api::User>,
        }
        let url = self.http.url("/api/v2/users/me.json", &[]);
        match self.http.get_json::<Me>(&url, cancel).await {
            Ok(Some(me)) => Ok(Ok(me
                .user
                .and_then(|u| u.role)
                .is_some_and(|r| r == "admin"))),
            Ok(None) => Ok(Err("/api/v2/users/me.json answered 404".into())),
            Err(e) if stops_the_fill(&e) => Err(e),
            Err(e) => Ok(Err(failure(&e))),
        }
    }

    /// Users by id, deleted ones included, in requests of [`SHOW_MANY`]. No
    /// `brand_id`: ids are account-wide, and Show Many rejects
    /// `brand_id=all`. Absentees count as gone only when `confirm`.
    pub(super) async fn users_by_id(
        &self,
        ids: &[i64],
        confirm: bool,
        cancel: &CancellationToken,
    ) -> Result<ById<api::User>, SourceError> {
        self.by_id(
            "/api/v2/users/show_many.json",
            &[("include_deleted", "true")],
            ids,
            confirm,
            |l: api::UsersList| l.users,
            |u: &api::User| u.id,
            cancel,
        )
        .await
    }

    pub(super) async fn organizations_by_id(
        &self,
        ids: &[i64],
        cancel: &CancellationToken,
    ) -> Result<ById<api::Organization>, SourceError> {
        self.by_id(
            "/api/v2/organizations/show_many.json",
            &[],
            ids,
            false,
            |l: api::OrganizationsList| l.organizations,
            |o: &api::Organization| o.id,
            cancel,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn by_id<L, T>(
        &self,
        path: &str,
        extra: &[(&str, &str)],
        ids: &[i64],
        confirm: bool,
        items: fn(L) -> Vec<T>,
        id_of: fn(&T) -> i64,
        cancel: &CancellationToken,
    ) -> Result<ById<T>, SourceError>
    where
        L: serde::de::DeserializeOwned,
    {
        let mut out = ById {
            returned: Vec::new(),
            gone: Vec::new(),
            errors: Vec::new(),
            stopped: None,
        };
        for chunk in ids.chunks(SHOW_MANY) {
            let list = chunk
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let mut query = vec![("ids", list.as_str())];
            query.extend_from_slice(extra);
            let url = self.http.url(path, &query);
            let page = match self.http.get_json::<L>(&url, cancel).await {
                Ok(Some(page)) => items(page),
                // Not an answer about these ids: none of them is taken for
                // gone on it.
                Ok(None) => {
                    out.errors
                        .push(format!("{path} answered 404 for {} ids", chunk.len()));
                    continue;
                }
                Err(SourceError::Cancelled) => return Err(SourceError::Cancelled),
                Err(e) if stops_the_fill(&e) => {
                    out.stopped = Some(failure(&e));
                    return Ok(out);
                }
                Err(e) => {
                    out.errors
                        .push(format!("{} (for {} ids)", failure(&e), chunk.len()));
                    continue;
                }
            };
            let wanted: HashSet<i64> = chunk.iter().copied().collect();
            let got: HashSet<i64> = page.iter().map(id_of).collect();
            if confirm {
                out.gone
                    .extend(chunk.iter().copied().filter(|id| !got.contains(id)));
            }
            out.returned
                .extend(page.into_iter().filter(|t| wanted.contains(&id_of(t))));
        }
        Ok(out)
    }

    /// The account's group or brand names, or why they couldn't be had.
    /// `Err` only when the fill must stop.
    pub(super) async fn names(
        &self,
        what: Names,
        cancel: &CancellationToken,
    ) -> Result<Result<HashMap<i64, String>, String>, SourceError> {
        let listed = match what {
            Names::Groups => {
                self.list_all(
                    "/api/v2/groups.json",
                    |p: api::GroupsPage| (p.groups, p.meta),
                    cancel,
                )
                .await
            }
            Names::Brands => {
                self.list_all(
                    "/api/v2/brands.json",
                    |p: api::BrandsPage| (p.brands, p.meta),
                    cancel,
                )
                .await
            }
        };
        match listed {
            Ok(items) => Ok(Ok(map::Labels::names(items))),
            Err(e) if stops_the_fill(&e) => Err(e),
            Err(e) => Ok(Err(failure(&e))),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Names {
    Groups,
    Brands,
}
