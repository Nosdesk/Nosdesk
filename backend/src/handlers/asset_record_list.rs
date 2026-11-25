//! The workspace's asset records across every asset: the Records tab.
//!
//! - `GET /api/assets/records`: a page of the records the filters match,
//!   with their assets, and what the whole filtered set adds up to (count,
//!   open work, costs per currency) whatever page is loaded. Grouped, each
//!   group's totals too, and the rows arrive group by group.
//! - `GET /api/assets/records/facets`: the vendors and assets the filters
//!   offer, and how many records the workspace has.
//! - `GET /api/assets/records/export`: the filtered set in the records
//!   import template's columns, as CSV or JSON.
//!
//! Filters: `q` (summary, vendor, reference, the asset's tag or name),
//! `type` (slug or id), `status` (`open` or `completed`), `vendor`, `asset`
//! (id) and `record` (id), each repeatable; `from` and `to` (days of the
//! work, inclusive). Then `sort` (`date`, `cost`, `asset`, `vendor`), `dir`,
//! `group` (`vendor`, `type`, `asset`, `month`), `offset` and `limit`.
//!
//! Records are staff only: anyone else gets 404, as for one asset's.

use actix_web::{http::header, HttpRequest, HttpResponse};
use chrono::NaiveDate;
use serde_json::{json, Value};
use tracing::error;

use crate::errors;
use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::asset_records::{
    records_json, require_staff, respond, workspace_currency, Refusal,
};
use crate::repository;
use crate::repository::asset_record_list::{
    self as list_repo, GroupAxis, ListFilter, SetTotals, Sort,
};
use crate::repository::asset_record_types as types_repo;
use crate::services::imports::assets as imports_assets;
use crate::utils::money;

const PAGE_DEFAULT: i64 = 100;
const PAGE_MAX: i64 = 500;
/// The most records one export holds, so a wide filter can't exhaust the
/// process. A capped export says so in `X-Export-Rows` and `X-Export-Total`.
const EXPORT_MAX: i64 = 50_000;

/// The list's query, read from a query string whose filters repeat.
#[derive(Debug, Default)]
struct ListQuery {
    q: Option<String>,
    types: Vec<String>,
    status: Vec<String>,
    vendors: Vec<String>,
    assets: Vec<i32>,
    records: Vec<i32>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    sort: Sort,
    descending: bool,
    group: Option<GroupAxis>,
    offset: i64,
    limit: i64,
    format: Option<String>,
}

fn parse_query(raw: &str) -> Result<ListQuery, String> {
    let mut out = ListQuery {
        descending: true,
        limit: PAGE_DEFAULT,
        ..Default::default()
    };
    let day = |key: &str, value: &str| {
        value
            .parse::<NaiveDate>()
            .map_err(|_| format!("{key} must be a day such as 2024-05-03"))
    };
    let id = |key: &str, value: &str| {
        value
            .trim()
            .parse::<i32>()
            .map_err(|_| format!("{key} must be an id"))
    };
    for (key, raw_value) in url::form_urlencoded::parse(raw.as_bytes()) {
        let value = raw_value.trim();
        if value.is_empty() {
            continue;
        }
        match key.as_ref() {
            // As sent: Postgres folds vendors on both sides alike.
            "vendor" => out.vendors.push(raw_value.into_owned()),
            "q" => out.q = Some(value.to_string()),
            "type" => out.types.push(value.to_string()),
            "status" => match value {
                "open" | "completed" => out.status.push(value.to_string()),
                _ => return Err("status must be open or completed".into()),
            },
            "asset" => out.assets.push(id("asset", value)?),
            "record" => out.records.push(id("record", value)?),
            "from" => out.from = Some(day("from", value)?),
            "to" => out.to = Some(day("to", value)?),
            "sort" => {
                out.sort = Sort::parse(value)
                    .ok_or("sort must be date, cost, asset or vendor".to_string())?
            }
            "dir" => match value {
                "asc" => out.descending = false,
                "desc" => out.descending = true,
                _ => return Err("dir must be asc or desc".into()),
            },
            "group" => {
                out.group = match value {
                    "none" => None,
                    _ => Some(
                        GroupAxis::parse(value)
                            .ok_or("group must be vendor, type, asset or month".to_string())?,
                    ),
                }
            }
            "offset" => {
                out.offset = value
                    .parse::<i64>()
                    .map_err(|_| "offset must be a number".to_string())?
                    .max(0)
            }
            "limit" => {
                out.limit = value
                    .parse::<i64>()
                    .map_err(|_| "limit must be a number".to_string())?
                    .clamp(1, PAGE_MAX)
            }
            "format" => out.format = Some(value.to_string()),
            _ => {}
        }
    }
    if let (Some(from), Some(to)) = (out.from, out.to) {
        if from > to {
            return Err("from must not be after to".into());
        }
    }
    Ok(out)
}

/// The repository's filter for `query`, with type slugs resolved to ids.
fn filter_for(
    conn: &mut crate::db::DbConnection,
    query: &ListQuery,
) -> Result<ListFilter, Refusal> {
    let record_type_ids = if query.types.is_empty() {
        None
    } else {
        let types = types_repo::list(conn, true)?;
        Some(
            types
                .iter()
                .filter(|t| {
                    query
                        .types
                        .iter()
                        .any(|asked| *asked == t.slug || asked.parse::<i32>() == Ok(t.id))
                })
                .map(|t| t.id)
                .collect(),
        )
    };
    // Both statuses, or neither, is every record.
    let asked = |status: &str| query.status.iter().any(|s| s == status);
    let open = match (asked("open"), asked("completed")) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None,
    };
    Ok(ListFilter {
        q: query.q.clone(),
        record_type_ids,
        open,
        vendors: query.vendors.clone(),
        asset_ids: query.assets.clone(),
        from: query.from,
        to: query.to,
        ids: query.records.clone(),
    })
}

/// Costs per currency: every other currency by code, then `last` (the
/// workspace currency), so it sits on the Cost column's edge.
pub(crate) fn costs_json(totals: &SetTotals, last: Option<&str>) -> Value {
    let mut codes: Vec<&String> = totals.costs.keys().collect();
    codes.sort_by_key(|code| (Some(code.as_str()) == last, code.as_str()));
    Value::Array(
        codes
            .into_iter()
            .map(|code| {
                let minor = totals.costs[code];
                json!({
                    "currency": code,
                    "amount": money::format_minor(minor, code).ok(),
                    "amount_minor": minor,
                })
            })
            .collect(),
    )
}

/// `GET /api/assets/records`.
pub async fn list(mut tc: TenantConn, auth: AuthContext, req: HttpRequest) -> HttpResponse {
    let query = match parse_query(req.query_string()) {
        Ok(q) => q,
        Err(message) => return errors::bad_request(message),
    };
    let result = tc.run_result(|conn| {
        require_staff(&auth)?;
        let currency = workspace_currency(conn)?;
        let filter = filter_for(conn, &query)?;
        let totals = list_repo::totals(conn, &filter)?;
        let groups = match query.group {
            Some(axis) => Some((
                axis,
                list_repo::groups(conn, &filter, axis, currency.as_deref())?,
            )),
            None => None,
        };
        let rows = list_repo::page(
            conn,
            &filter,
            query.sort,
            query.descending,
            groups.as_ref().map(|(axis, g)| (*axis, g.as_slice())),
            query.offset,
            Some(query.limit),
        )?;
        let records: Vec<_> = rows.iter().map(|r| r.record.clone()).collect();
        let mut out = records_json(conn, &records)?;
        for (json, row) in out.iter_mut().zip(&rows) {
            if let Value::Object(map) = json {
                map.insert("asset".into(), json!(row.asset));
                if let Some(group) = &row.group {
                    map.insert("group".into(), json!(group));
                }
            }
        }
        let loaded = query.offset + rows.len() as i64;
        let groups_json = groups.map(|(_, groups)| {
            groups
                .iter()
                .map(|g| {
                    json!({
                        "key": g.key,
                        "label": g.label,
                        "count": g.totals.count,
                        "open": g.totals.open,
                        "totals": costs_json(&g.totals, currency.as_deref()),
                    })
                })
                .collect::<Vec<_>>()
        });
        Ok::<_, Refusal>(json!({
            "records": out,
            "total": totals.count,
            "open": totals.open,
            "totals": costs_json(&totals, currency.as_deref()),
            "currency": currency,
            "groups": groups_json,
            "group": query.group.map(GroupAxis::as_str),
            "offset": query.offset,
            // An empty page ends the list even if the set changed under it.
            "next_offset": (!rows.is_empty() && loaded < totals.count).then_some(loaded),
        }))
    });
    match result {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(r) => respond(r),
    }
}

/// `GET /api/assets/records/facets`.
pub async fn facets(mut tc: TenantConn, auth: AuthContext) -> HttpResponse {
    let result = tc.run_result(|conn| {
        require_staff(&auth)?;
        let facets = list_repo::facets(conn)?;
        Ok::<_, Refusal>(json!({
            "total": facets.total,
            "vendors": facets.vendors,
            "assets": facets.assets,
        }))
    });
    match result {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(r) => respond(r),
    }
}

/// `GET /api/assets/records/export?format=csv|json`: every record the
/// filters match, in the list's order, one row each in the records import
/// template's columns, so an export imports back: with one type, a column
/// per field of it; otherwise the fields in one JSON column.
pub async fn export(mut tc: TenantConn, auth: AuthContext, req: HttpRequest) -> HttpResponse {
    let query = match parse_query(req.query_string()) {
        Ok(q) => q,
        Err(message) => return errors::bad_request(message),
    };
    let json_out = match query.format.as_deref().unwrap_or("csv") {
        "csv" => false,
        "json" => true,
        _ => return errors::bad_request("format must be csv or json"),
    };
    let Some(workspace_id) = tc.workspace_id() else {
        return errors::forbidden("Export requires a resolved workspace context for this request");
    };
    let result = tc.run_result(|conn| {
        require_staff(&auth)?;
        let filter = filter_for(conn, &query)?;
        let total = list_repo::totals(conn, &filter)?.count;
        let rows = list_repo::page(
            conn,
            &filter,
            query.sort,
            query.descending,
            None,
            0,
            Some(EXPORT_MAX),
        )?;
        let records: Vec<_> = rows.into_iter().map(|r| r.record).collect();
        let tz = repository::asset_records::workspace_timezone(conn)?;
        let lookups = crate::handlers::assets::record_lookups(conn, &records, tz)?;
        let mut asset_ids: Vec<i32> = records.iter().map(|r| r.asset_id).collect();
        asset_ids.sort_unstable();
        asset_ids.dedup();
        let assets = repository::assets::list_for_export(
            conn,
            repository::assets::AssetListFilters {
                search: None,
                warranty: None,
                location: None,
                status: None,
                status_id: None,
                groups: None,
                low_stock_only: false,
                ids: Some(&asset_ids),
                // Records are staff only.
                visible: None,
            },
        )?;
        let slug = repository::workspaces::find_by_id(conn, workspace_id)?
            .map(|w| w.slug)
            .unwrap_or_else(|| "workspace".to_string());
        // Records of one type get a column per field of that type, as the
        // type's import template has.
        let one_type = match filter.record_type_ids.as_deref() {
            Some([id]) => lookups.types.get(id).cloned(),
            _ => None,
        };
        Ok::<_, Refusal>((records, lookups, assets, slug, one_type, total))
    });
    let (records, lookups, assets, slug, one_type, total) = match result {
        Ok(v) => v,
        Err(r) => return respond(r),
    };
    let lookups = imports_assets::RecordExportLookups {
        assets: assets.iter().map(|a| (a.id, a)).collect(),
        types: lookups.types,
        emails: lookups.emails,
        ticket_numbers: lookups.ticket_numbers,
        tags: lookups.tags,
        tz: lookups.tz,
    };
    let headers = imports_assets::record_export_headers(one_type.as_ref());
    let cells = imports_assets::build_record_rows(&records, one_type.as_ref(), &lookups);
    let built = if json_out {
        serde_json::to_vec(&imports_assets::cells_json(&headers, &cells)).map_err(|e| e.to_string())
    } else {
        imports_assets::write_cells_csv(&headers, &cells).map_err(|e| e.to_string())
    };
    let body = match built {
        Ok(body) => body,
        Err(e) => {
            error!(error = %e, "Failed to build the asset records export");
            return errors::internal("Failed to build the records export");
        }
    };
    let (content_type, ext) = if json_out {
        ("application/json; charset=utf-8", "json")
    } else {
        ("text/csv; charset=utf-8", "csv")
    };
    let date = chrono::Utc::now().format("%Y%m%d");
    HttpResponse::Ok()
        .insert_header(("X-Export-Rows", records.len().to_string()))
        .insert_header(("X-Export-Total", total.to_string()))
        .insert_header((header::CONTENT_TYPE, content_type))
        .insert_header((
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"asset-records-{slug}-{date}.{ext}\""),
        ))
        .body(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_repeat_and_blank_ones_are_skipped() {
        let q = parse_query("type=repair&type=7&vendor=Acme%2C+Pty+&vendor=&asset=3&status=open")
            .expect("reads");
        assert_eq!(q.types, vec!["repair", "7"]);
        assert_eq!(q.vendors, vec!["Acme, Pty "], "vendors stay as sent");
        assert_eq!(q.assets, vec![3]);
        assert_eq!(q.status, vec!["open"]);
        assert!(q.descending);
        assert_eq!((q.offset, q.limit), (0, PAGE_DEFAULT));
    }

    #[test]
    fn bad_values_are_refused() {
        assert!(parse_query("status=pending").is_err());
        assert!(parse_query("sort=price").is_err());
        assert!(parse_query("from=2024-05-04&to=2024-05-03").is_err());
        assert!(parse_query("asset=CAM-1").is_err());
        assert_eq!(parse_query("limit=10000").expect("reads").limit, PAGE_MAX);
        assert_eq!(parse_query("group=none").expect("reads").group, None);
    }

    #[test]
    fn the_workspace_currency_comes_last() {
        let mut totals = SetTotals::default();
        for (code, minor) in [("USD", 100), ("AUD", 5000), ("EUR", 10)] {
            totals.costs.insert(code.to_string(), minor);
        }
        let codes: Vec<String> = costs_json(&totals, Some("AUD"))
            .as_array()
            .expect("array")
            .iter()
            .map(|c| c["currency"].as_str().expect("code").to_string())
            .collect();
        assert_eq!(codes, vec!["EUR", "USD", "AUD"]);
    }
}
