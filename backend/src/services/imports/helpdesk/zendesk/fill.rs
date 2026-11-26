//! The API fill: what an export lacks, asked of Zendesk by id when the
//! operator gives a credential. Standard endpoints only (Show Many Users,
//! Show Many Organizations, the group and brand lists, a ticket's
//! comments), never an incremental export. One request at a time, since
//! the fill shares the customer's live API quota; the client waits out a
//! 429 for as long as `Retry-After` says.

use std::collections::HashMap;

use tokio_util::sync::CancellationToken;

use super::{api, map, ZendeskSource};
use crate::services::imports::helpdesk::source::SourceError;

/// Ids per Show Many request: Zendesk's limit.
pub const SHOW_MANY: usize = 100;

/// What asking by id came to.
#[derive(Debug)]
pub struct ById<T> {
    pub returned: Vec<T>,
    /// Asked for in a request that succeeded, and not returned.
    pub gone: Vec<i64>,
    /// Why the fill stopped, when it did. The ids not yet asked for, and
    /// those of the request that failed, are neither returned nor gone.
    pub failed: Option<String>,
}

/// A fill failure in words for the operator. A refused credential keeps the
/// message the API fetch gives.
pub fn failure(e: &SourceError) -> String {
    match e {
        SourceError::Unauthorized(why) => format!(
            "Zendesk refused the credential ({why}). OAuth tokens expire (30 minutes by \
             default): give a fresh one and run again with --refetch"
        ),
        other => other.to_string(),
    }
}

impl ZendeskSource {
    /// Users by id, deleted ones included, in requests of [`SHOW_MANY`]. No
    /// `brand_id`: ids are account-wide, and Show Many rejects
    /// `brand_id=all`.
    pub(super) async fn users_by_id(
        &self,
        ids: &[i64],
        cancel: &CancellationToken,
    ) -> Result<ById<api::User>, SourceError> {
        self.by_id(
            "/api/v2/users/show_many.json",
            &[("include_deleted", "true")],
            ids,
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
            |l: api::OrganizationsList| l.organizations,
            |o: &api::Organization| o.id,
            cancel,
        )
        .await
    }

    async fn by_id<L, T>(
        &self,
        path: &str,
        extra: &[(&str, &str)],
        ids: &[i64],
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
            failed: None,
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
                // Nothing by those ids at all.
                Ok(None) => Vec::new(),
                Err(SourceError::Cancelled) => return Err(SourceError::Cancelled),
                Err(e) => {
                    out.failed = Some(failure(&e));
                    return Ok(out);
                }
            };
            let wanted: std::collections::HashSet<i64> = chunk.iter().copied().collect();
            let got: std::collections::HashSet<i64> = page.iter().map(id_of).collect();
            out.gone
                .extend(chunk.iter().copied().filter(|id| !got.contains(id)));
            out.returned
                .extend(page.into_iter().filter(|t| wanted.contains(&id_of(t))));
        }
        Ok(out)
    }

    /// The account's group or brand names, or why they couldn't be had.
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
            Err(SourceError::Cancelled) => Err(SourceError::Cancelled),
            Err(e) => Ok(Err(failure(&e))),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Names {
    Groups,
    Brands,
}
