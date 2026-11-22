use diesel::prelude::*;
use diesel::result::Error;
use diesel::QueryResult;
use serde_json::json;

use crate::db::DbConnection;
use crate::models::{AssetMedia, AssetMediaUpdate, NewAssetMedia, SyncAggregate, SyncOp};
use crate::schema::asset_media;
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;

fn asset_media_sync_payload(row: &AssetMedia) -> serde_json::Value {
    json!({
        "id": row.id,
        "asset_id": row.asset_id,
        "url": row.url,
        "thumbnail_url": row.thumbnail_url,
        "name": row.name,
        "mime_type": row.mime_type,
        "file_size": row.file_size,
        "kind": row.kind,
        "sort_order": row.sort_order,
        "caption": row.caption,
        "uploaded_by": row.uploaded_by,
        "created_at": row.created_at,
        "record_id": row.record_id,
    })
}

fn emit_asset_media_event(
    conn: &mut DbConnection,
    row: &AssetMedia,
    op: SyncOp,
    event_type: &'static str,
) -> QueryResult<()> {
    emit::record(
        conn,
        SyncEmit {
            aggregate: SyncAggregate::AssetMedia,
            aggregate_id: row.id.to_string(),
            op,
            event_type,
            data: asset_media_sync_payload(row),
            groups: groups::workspace(),
            causation_id: None,
        },
    )?;
    Ok(())
}

pub fn list_for_asset(conn: &mut DbConnection, asset_id: i32) -> QueryResult<Vec<AssetMedia>> {
    asset_media::table
        .filter(asset_media::asset_id.eq(asset_id))
        .order((
            asset_media::sort_order.asc(),
            asset_media::created_at.desc(),
        ))
        .load(conn)
}

/// The name as uploaded of `asset_id`'s media stored at `url`, if any.
pub fn name_by_url(
    conn: &mut DbConnection,
    asset_id: i32,
    url: &str,
) -> QueryResult<Option<String>> {
    asset_media::table
        .filter(asset_media::asset_id.eq(asset_id))
        .filter(asset_media::url.eq(url))
        .select(asset_media::name)
        .first(conn)
        .optional()
}

/// The files of asset record `record_id`.
pub fn list_for_record(conn: &mut DbConnection, record_id: i32) -> QueryResult<Vec<AssetMedia>> {
    asset_media::table
        .filter(asset_media::record_id.eq(record_id))
        .order((asset_media::sort_order.asc(), asset_media::id.asc()))
        .load(conn)
}

/// Make `media_ids` the files of record `record_id` on asset `asset_id`:
/// each of them points at the record, and the record's other files no longer
/// do. An id that isn't one of the asset's files is left out, so the caller
/// compares what comes back with what it asked for. Returns the record's
/// files as they now stand.
pub fn set_record_files(
    conn: &mut DbConnection,
    asset_id: i32,
    record_id: i32,
    media_ids: &[i32],
) -> QueryResult<Vec<AssetMedia>> {
    // emit::record fires inside emit_asset_media_event.
    conn.transaction::<Vec<AssetMedia>, Error, _>(|conn| {
        let unlinked: Vec<AssetMedia> = diesel::update(
            asset_media::table
                .filter(asset_media::record_id.eq(record_id))
                .filter(asset_media::id.ne_all(media_ids)),
        )
        .set(asset_media::record_id.eq(None::<i32>))
        .get_results(conn)?;
        for row in &unlinked {
            emit_asset_media_event(conn, row, SyncOp::Update, "asset_media.updated")?;
        }
        let linked: Vec<AssetMedia> = diesel::update(
            asset_media::table
                .filter(asset_media::asset_id.eq(asset_id))
                .filter(asset_media::id.eq_any(media_ids))
                .filter(asset_media::record_id.is_distinct_from(record_id)),
        )
        .set(asset_media::record_id.eq(record_id))
        .get_results(conn)?;
        for row in &linked {
            emit_asset_media_event(conn, row, SyncOp::Update, "asset_media.updated")?;
        }
        list_for_record(conn, record_id)
    })
}

pub fn get_by_id(conn: &mut DbConnection, media_id: i32) -> QueryResult<AssetMedia> {
    asset_media::table.find(media_id).first(conn)
}

pub fn create(conn: &mut DbConnection, new_media: NewAssetMedia) -> QueryResult<AssetMedia> {
    // emit::record fires inside emit_asset_media_event.
    conn.transaction::<AssetMedia, Error, _>(|conn| {
        let row: AssetMedia = diesel::insert_into(asset_media::table)
            .values(&new_media)
            .get_result(conn)?;
        emit_asset_media_event(conn, &row, SyncOp::Insert, "asset_media.created")?;
        Ok(row)
    })
}

pub fn update(
    conn: &mut DbConnection,
    media_id: i32,
    update: AssetMediaUpdate,
) -> QueryResult<AssetMedia> {
    // emit::record fires inside emit_asset_media_event.
    conn.transaction::<AssetMedia, Error, _>(|conn| {
        let row: AssetMedia = diesel::update(asset_media::table.find(media_id))
            .set(&update)
            .get_result(conn)?;
        emit_asset_media_event(conn, &row, SyncOp::Update, "asset_media.updated")?;
        Ok(row)
    })
}

pub fn delete(conn: &mut DbConnection, media_id: i32) -> QueryResult<Option<AssetMedia>> {
    conn.transaction::<Option<AssetMedia>, Error, _>(|conn| {
        let row = asset_media::table
            .find(media_id)
            .first::<AssetMedia>(conn)
            .optional()?;
        let Some(row) = row else {
            return Ok(None);
        };
        diesel::delete(asset_media::table.find(media_id)).execute(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::AssetMedia,
                aggregate_id: media_id.to_string(),
                op: SyncOp::Delete,
                event_type: "asset_media.deleted",
                data: json!({ "id": media_id, "asset_id": row.asset_id }),
                groups: groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(Some(row))
    })
}
