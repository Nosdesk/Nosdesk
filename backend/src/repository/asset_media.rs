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
