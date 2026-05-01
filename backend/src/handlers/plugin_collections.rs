//! Plugin Collections Handlers
//!
//! API endpoints for plugin typed collections CRUD operations.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error as DieselError;
use tracing::error;
use uuid::Uuid;

use crate::db::Pool;
use crate::handlers::helpers;
use crate::handlers::errors;
use crate::models::{
    Claims, CollectionListResponse, CollectionQueryParams, CollectionRowResponse,
    CollectionSchemaResponse, CreateCollectionRowRequest, NewPluginCollectionRow,
    PluginCollectionRowUpdate, UpdateCollectionRowRequest,
};
use crate::repository::plugin_collections as collection_repo;
use crate::repository::plugins as plugin_repo;
use crate::services::plugins::validation;

/// Path params: /plugins/{uuid}/collections/{name}
#[derive(serde::Deserialize)]
pub struct CollectionPath {
    uuid: Uuid,
    name: String,
}

/// Path params: /plugins/{uuid}/collections/{name}/rows/{row_uuid}
#[derive(serde::Deserialize)]
pub struct CollectionRowPath {
    uuid: Uuid,
    name: String,
    row_uuid: Uuid,
}

// =============================================================================
// Helpers
// =============================================================================

fn get_claims(req: &HttpRequest) -> Result<Claims, HttpResponse> {
    req.extensions()
        .get::<Claims>()
        .cloned()
        .ok_or_else(|| errors::unauthorized("Authentication required"))
}

// =============================================================================
// Schema endpoints
// =============================================================================

/// List all collections for a plugin
pub async fn list_collections(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let _claims = match get_claims(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let plugin_uuid = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let plugin = match plugin_repo::get_plugin_by_uuid(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(DieselError::NotFound) => return errors::not_found_msg("Plugin not found"),
        Err(e) => {
            error!("Failed to get plugin: {}", e);
            return errors::internal("Failed to get plugin");
        }
    };

    let schemas = match collection_repo::get_schemas_by_plugin(&mut conn, plugin.id) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to get collection schemas: {}", e);
            return errors::internal("Failed to get collections");
        }
    };

    let responses: Vec<CollectionSchemaResponse> = schemas
        .into_iter()
        .map(|s| {
            let row_count =
                collection_repo::count_rows_by_schema(&mut conn, s.id).unwrap_or(0);
            CollectionSchemaResponse {
                uuid: s.uuid,
                collection_name: s.collection_name,
                schema: s.schema,
                version: s.version,
                row_count,
            }
        })
        .collect();

    HttpResponse::Ok().json(responses)
}

/// Get a single collection schema
pub async fn get_collection_schema(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<CollectionPath>,
) -> impl Responder {
    let _claims = match get_claims(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let path = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let plugin = match plugin_repo::get_plugin_by_uuid(&mut conn, path.uuid) {
        Ok(p) => p,
        Err(DieselError::NotFound) => return errors::not_found_msg("Plugin not found"),
        Err(e) => {
            error!("Failed to get plugin: {}", e);
            return errors::internal("Failed to get plugin");
        }
    };

    let schema = match collection_repo::get_schema_by_name(&mut conn, plugin.id, &path.name) {
        Ok(s) => s,
        Err(DieselError::NotFound) => {
            return errors::not_found_msg("Collection not found")
        }
        Err(e) => {
            error!("Failed to get collection schema: {}", e);
            return errors::internal("Failed to get collection");
        }
    };

    let row_count = collection_repo::count_rows_by_schema(&mut conn, schema.id).unwrap_or(0);

    HttpResponse::Ok().json(CollectionSchemaResponse {
        uuid: schema.uuid,
        collection_name: schema.collection_name,
        schema: schema.schema,
        version: schema.version,
        row_count,
    })
}

// =============================================================================
// Row endpoints
// =============================================================================

/// List rows in a collection
pub async fn list_collection_rows(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<CollectionPath>,
    query: web::Query<CollectionQueryParams>,
) -> impl Responder {
    let _claims = match get_claims(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let path = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let plugin = match plugin_repo::get_plugin_by_uuid(&mut conn, path.uuid) {
        Ok(p) => p,
        Err(DieselError::NotFound) => return errors::not_found_msg("Plugin not found"),
        Err(e) => {
            error!("Failed to get plugin: {}", e);
            return errors::internal("Failed to get plugin");
        }
    };

    let schema = match collection_repo::get_schema_by_name(&mut conn, plugin.id, &path.name) {
        Ok(s) => s,
        Err(DieselError::NotFound) => {
            return errors::not_found_msg("Collection not found")
        }
        Err(e) => {
            error!("Failed to get collection schema: {}", e);
            return errors::internal("Failed to get collection");
        }
    };

    let limit = helpers::clamp_limit(query.limit);
    let offset = helpers::clamp_offset(query.offset);

    // Parse filter JSON if provided
    let filter_json = query.filter.as_ref().and_then(|f| {
        serde_json::from_str(f).ok()
    });

    let (rows, total) = match collection_repo::list_rows(
        &mut conn,
        schema.id,
        limit,
        offset,
        filter_json,
        query.sort_by.clone(),
        query.sort_order.clone(),
    ) {
        Ok(result) => result,
        Err(e) => {
            error!("Failed to list collection rows: {}", e);
            return errors::internal("Failed to list rows");
        }
    };

    HttpResponse::Ok().json(CollectionListResponse {
        rows: rows.into_iter().map(CollectionRowResponse::from).collect(),
        total,
    })
}

/// Create a row in a collection
pub async fn create_collection_row(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<CollectionPath>,
    body: web::Json<CreateCollectionRowRequest>,
) -> impl Responder {
    let claims = match get_claims(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let path = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let plugin = match plugin_repo::get_plugin_by_uuid(&mut conn, path.uuid) {
        Ok(p) => p,
        Err(DieselError::NotFound) => return errors::not_found_msg("Plugin not found"),
        Err(e) => {
            error!("Failed to get plugin: {}", e);
            return errors::internal("Failed to get plugin");
        }
    };

    let schema = match collection_repo::get_schema_by_name(&mut conn, plugin.id, &path.name) {
        Ok(s) => s,
        Err(DieselError::NotFound) => {
            return errors::not_found_msg("Collection not found")
        }
        Err(e) => {
            error!("Failed to get collection schema: {}", e);
            return errors::internal("Failed to get collection");
        }
    };

    // Parse the collection definition from the schema for validation
    if let Ok(definition) = serde_json::from_value::<crate::models::CollectionDefinition>(schema.schema.clone()) {
        if let Err(e) = validation::validate_row_data(&body.data, &definition) {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": "Validation error",
                "message": e
            }));
        }
    }

    let user_uuid = Uuid::parse_str(&claims.sub).ok();

    let new_row = NewPluginCollectionRow {
        plugin_id: plugin.id,
        schema_id: schema.id,
        data: body.data.clone(),
        created_by: user_uuid,
    };

    match collection_repo::create_row(&mut conn, new_row) {
        Ok(row) => HttpResponse::Created().json(CollectionRowResponse::from(row)),
        Err(e) => {
            error!("Failed to create collection row: {}", e);
            errors::internal("Failed to create row")
        }
    }
}

/// Get a single row
pub async fn get_collection_row(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<CollectionRowPath>,
) -> impl Responder {
    let _claims = match get_claims(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let path = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Verify plugin exists
    if let Err(DieselError::NotFound) = plugin_repo::get_plugin_by_uuid(&mut conn, path.uuid) {
        return errors::not_found_msg("Plugin not found");
    }

    match collection_repo::get_row_by_uuid(&mut conn, path.row_uuid) {
        Ok(row) => HttpResponse::Ok().json(CollectionRowResponse::from(row)),
        Err(DieselError::NotFound) => errors::not_found_msg("Row not found"),
        Err(e) => {
            error!("Failed to get collection row: {}", e);
            errors::internal("Failed to get row")
        }
    }
}

/// Update a row
pub async fn update_collection_row(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<CollectionRowPath>,
    body: web::Json<UpdateCollectionRowRequest>,
) -> impl Responder {
    let _claims = match get_claims(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let path = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let plugin = match plugin_repo::get_plugin_by_uuid(&mut conn, path.uuid) {
        Ok(p) => p,
        Err(DieselError::NotFound) => return errors::not_found_msg("Plugin not found"),
        Err(e) => {
            error!("Failed to get plugin: {}", e);
            return errors::internal("Failed to get plugin");
        }
    };

    let schema = match collection_repo::get_schema_by_name(&mut conn, plugin.id, &path.name) {
        Ok(s) => s,
        Err(DieselError::NotFound) => {
            return errors::not_found_msg("Collection not found")
        }
        Err(e) => {
            error!("Failed to get collection schema: {}", e);
            return errors::internal("Failed to get collection");
        }
    };

    // Validate updated data
    if let Ok(definition) = serde_json::from_value::<crate::models::CollectionDefinition>(schema.schema.clone()) {
        if let Err(e) = validation::validate_row_data(&body.data, &definition) {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": "Validation error",
                "message": e
            }));
        }
    }

    let update = PluginCollectionRowUpdate {
        data: Some(body.data.clone()),
    };

    match collection_repo::update_row(&mut conn, path.row_uuid, update) {
        Ok(row) => HttpResponse::Ok().json(CollectionRowResponse::from(row)),
        Err(DieselError::NotFound) => errors::not_found_msg("Row not found"),
        Err(e) => {
            error!("Failed to update collection row: {}", e);
            errors::internal("Failed to update row")
        }
    }
}

/// Delete a row
pub async fn delete_collection_row(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<CollectionRowPath>,
) -> impl Responder {
    let _claims = match get_claims(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let path = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Verify plugin exists
    if let Err(DieselError::NotFound) = plugin_repo::get_plugin_by_uuid(&mut conn, path.uuid) {
        return errors::not_found_msg("Plugin not found");
    }

    match collection_repo::delete_row(&mut conn, path.row_uuid) {
        Ok(count) if count > 0 => HttpResponse::NoContent().finish(),
        Ok(_) => errors::not_found_msg("Row not found"),
        Err(e) => {
            error!("Failed to delete collection row: {}", e);
            errors::internal("Failed to delete row")
        }
    }
}
