//! Plugin Collections Handlers
//!
//! API endpoints for plugin typed collections CRUD operations.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error as DieselError;
use tracing::error;
use uuid::Uuid;

use crate::extractors::TenantConn;
use crate::handlers::errors;
use crate::handlers::helpers;
use crate::handlers::plugins::{authorize_plugin_data_request, PluginGate};
use crate::models::{
    Claims, CollectionListResponse, CollectionQueryParams, CollectionRowResponse,
    CollectionSchemaResponse, CreateCollectionRowRequest, NewPluginCollectionRow,
    PluginCollectionRowUpdate, UpdateCollectionRowRequest,
};
use crate::repository::plugin_collections as collection_repo;
use crate::services::plugins::validation;
use crate::utils::i18n;
use crate::utils::locale::request_locale;

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

/// In-closure outcome for schema lookup so the tc.run boundary can
/// distinguish plugin-not-found / collection-not-found from a generic
/// internal error without leaking HttpResponse into the txn closure.
enum SchemaLookup {
    Ok(crate::models::PluginCollectionSchema, crate::models::Plugin),
    Gate(PluginGate),
    CollectionNotFound,
}

// =============================================================================
// Schema endpoints
// =============================================================================

/// List all collections for a plugin
pub async fn list_collections(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<Uuid>,
) -> impl Responder {
    let _claims = match get_claims(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let plugin_uuid = path.into_inner();

    enum ListOutcome {
        Ok(Vec<CollectionSchemaResponse>),
        Gate(PluginGate),
    }

    let outcome = tc.run(|conn| {
        let plugin = match authorize_plugin_data_request(
            conn,
            plugin_uuid,
            "collection:read",
            "Plugin has not been granted collection access",
        )? {
            Ok(p) => p,
            Err(gate) => return Ok(ListOutcome::Gate(gate)),
        };

        let schemas = collection_repo::get_schemas_by_plugin(conn, plugin.id)?;

        let responses: Vec<CollectionSchemaResponse> = schemas
            .into_iter()
            .map(|s| {
                let row_count = collection_repo::count_rows_by_schema(conn, s.id).unwrap_or(0);
                CollectionSchemaResponse {
                    uuid: s.uuid,
                    collection_name: s.collection_name,
                    schema: s.schema,
                    version: s.version,
                    row_count,
                }
            })
            .collect();

        Ok::<_, DieselError>(ListOutcome::Ok(responses))
    });

    match outcome {
        Ok(ListOutcome::Ok(resp)) => HttpResponse::Ok().json(resp),
        Ok(ListOutcome::Gate(gate)) => gate.into_response(),
        Err(e) => {
            error!("Failed to list collections: {}", e);
            errors::internal("Failed to get collections")
        }
    }
}

/// Get a single collection schema
pub async fn get_collection_schema(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<CollectionPath>,
) -> impl Responder {
    let _claims = match get_claims(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let path = path.into_inner();

    let outcome = tc.run(|conn| {
        let plugin = match authorize_plugin_data_request(
            conn,
            path.uuid,
            "collection:read",
            "Plugin has not been granted collection access",
        )? {
            Ok(p) => p,
            Err(gate) => return Ok(SchemaLookup::Gate(gate)),
        };
        let schema = match collection_repo::get_schema_by_name(conn, plugin.id, &path.name) {
            Ok(s) => s,
            Err(DieselError::NotFound) => return Ok(SchemaLookup::CollectionNotFound),
            Err(e) => return Err(e),
        };
        Ok::<_, DieselError>(SchemaLookup::Ok(schema, plugin))
    });

    match outcome {
        Ok(SchemaLookup::Ok(schema, _plugin)) => {
            // count_rows_by_schema is a read that needs the same
            // workspace pin, so run it inside another tc.run rather
            // than reusing the connection outside the closure.
            let row_count = tc
                .run(|conn| collection_repo::count_rows_by_schema(conn, schema.id))
                .unwrap_or(0);
            HttpResponse::Ok().json(CollectionSchemaResponse {
                uuid: schema.uuid,
                collection_name: schema.collection_name,
                schema: schema.schema,
                version: schema.version,
                row_count,
            })
        }
        Ok(SchemaLookup::Gate(gate)) => gate.into_response(),
        Ok(SchemaLookup::CollectionNotFound) => errors::not_found_msg("Collection not found"),
        Err(e) => {
            error!("Failed to get collection schema: {}", e);
            errors::internal("Failed to get collection")
        }
    }
}

// =============================================================================
// Row endpoints
// =============================================================================

/// List rows in a collection
pub async fn list_collection_rows(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<CollectionPath>,
    query: web::Query<CollectionQueryParams>,
) -> impl Responder {
    let _claims = match get_claims(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let path = path.into_inner();
    let limit = helpers::clamp_limit(query.limit);
    let offset = helpers::clamp_offset(query.offset);

    // Parse filter JSON if provided
    let filter_json = query
        .filter
        .as_ref()
        .and_then(|f| serde_json::from_str(f).ok());
    let sort_by = query.sort_by.clone();
    let sort_order = query.sort_order.clone();

    enum RowsOutcome {
        Ok(Vec<crate::models::PluginCollectionRow>, i64),
        Gate(PluginGate),
        CollectionNotFound,
    }

    let outcome = tc.run(|conn| {
        let plugin = match authorize_plugin_data_request(
            conn,
            path.uuid,
            "collection:read",
            "Plugin has not been granted collection access",
        )? {
            Ok(p) => p,
            Err(gate) => return Ok(RowsOutcome::Gate(gate)),
        };
        let schema = match collection_repo::get_schema_by_name(conn, plugin.id, &path.name) {
            Ok(s) => s,
            Err(DieselError::NotFound) => return Ok(RowsOutcome::CollectionNotFound),
            Err(e) => return Err(e),
        };
        let (rows, total) = collection_repo::list_rows(
            conn,
            schema.id,
            limit,
            offset,
            filter_json,
            sort_by,
            sort_order,
        )?;
        Ok::<_, DieselError>(RowsOutcome::Ok(rows, total))
    });

    match outcome {
        Ok(RowsOutcome::Ok(rows, total)) => HttpResponse::Ok().json(CollectionListResponse {
            rows: rows.into_iter().map(CollectionRowResponse::from).collect(),
            total,
        }),
        Ok(RowsOutcome::Gate(gate)) => gate.into_response(),
        Ok(RowsOutcome::CollectionNotFound) => errors::not_found_msg("Collection not found"),
        Err(e) => {
            error!("Failed to list collection rows: {}", e);
            errors::internal("Failed to list rows")
        }
    }
}

/// Create a row in a collection
pub async fn create_collection_row(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<CollectionPath>,
    body: web::Json<CreateCollectionRowRequest>,
) -> impl Responder {
    let claims = match get_claims(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let path = path.into_inner();
    let user_uuid = Uuid::parse_str(&claims.sub).ok();
    let body_data = body.data.clone();
    let locale = request_locale(&req);

    enum CreateOutcome {
        Ok(crate::models::PluginCollectionRow),
        Gate(PluginGate),
        CollectionNotFound,
        ValidationError(String),
    }

    let outcome = tc.run(|conn| {
        let plugin = match authorize_plugin_data_request(
            conn,
            path.uuid,
            "collection:write",
            "Plugin has not been granted collection write access",
        )? {
            Ok(p) => p,
            Err(gate) => return Ok(CreateOutcome::Gate(gate)),
        };
        let schema = match collection_repo::get_schema_by_name(conn, plugin.id, &path.name) {
            Ok(s) => s,
            Err(DieselError::NotFound) => return Ok(CreateOutcome::CollectionNotFound),
            Err(e) => return Err(e),
        };

        // Parse the collection definition from the schema for validation
        if let Ok(definition) =
            serde_json::from_value::<crate::models::CollectionDefinition>(schema.schema.clone())
        {
            if let Err(e) = validation::validate_row_data(&body_data, &definition) {
                return Ok(CreateOutcome::ValidationError(e));
            }
        }

        let new_row = NewPluginCollectionRow {
            plugin_id: plugin.id,
            schema_id: schema.id,
            data: body_data.clone(),
            created_by: user_uuid,
        };
        let row = collection_repo::create_row(conn, new_row)?;
        Ok::<_, DieselError>(CreateOutcome::Ok(row))
    });

    match outcome {
        Ok(CreateOutcome::Ok(row)) => {
            HttpResponse::Created().json(CollectionRowResponse::from(row))
        }
        Ok(CreateOutcome::Gate(gate)) => gate.into_response(),
        Ok(CreateOutcome::CollectionNotFound) => errors::not_found_msg("Collection not found"),
        Ok(CreateOutcome::ValidationError(msg)) => {
            HttpResponse::BadRequest().json(serde_json::json!({
                "error": i18n::tr(&locale, "backend-error-validation"),
                "code": "backend-error-validation",
                "message": msg
            }))
        }
        Err(e) => {
            error!("Failed to create collection row: {}", e);
            errors::internal("Failed to create row")
        }
    }
}

/// Get a single row
pub async fn get_collection_row(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<CollectionRowPath>,
) -> impl Responder {
    let _claims = match get_claims(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let path = path.into_inner();

    enum GetOutcome {
        Ok(crate::models::PluginCollectionRow),
        Gate(PluginGate),
        RowNotFound,
    }

    let outcome = tc.run(|conn| {
        match authorize_plugin_data_request(
            conn,
            path.uuid,
            "collection:read",
            "Plugin has not been granted collection access",
        )? {
            Ok(_) => {}
            Err(gate) => return Ok(GetOutcome::Gate(gate)),
        }
        match collection_repo::get_row_by_uuid(conn, path.row_uuid) {
            Ok(row) => Ok::<_, DieselError>(GetOutcome::Ok(row)),
            Err(DieselError::NotFound) => Ok(GetOutcome::RowNotFound),
            Err(e) => Err(e),
        }
    });

    match outcome {
        Ok(GetOutcome::Ok(row)) => HttpResponse::Ok().json(CollectionRowResponse::from(row)),
        Ok(GetOutcome::Gate(gate)) => gate.into_response(),
        Ok(GetOutcome::RowNotFound) => errors::not_found_msg("Row not found"),
        Err(e) => {
            error!("Failed to get collection row: {}", e);
            errors::internal("Failed to get row")
        }
    }
}

/// Update a row
pub async fn update_collection_row(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<CollectionRowPath>,
    body: web::Json<UpdateCollectionRowRequest>,
) -> impl Responder {
    let _claims = match get_claims(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let path = path.into_inner();
    let body_data = body.data.clone();
    let locale = request_locale(&req);

    enum UpdateOutcome {
        Ok(crate::models::PluginCollectionRow),
        Gate(PluginGate),
        CollectionNotFound,
        RowNotFound,
        ValidationError(String),
    }

    let outcome = tc.run(|conn| {
        let plugin = match authorize_plugin_data_request(
            conn,
            path.uuid,
            "collection:write",
            "Plugin has not been granted collection write access",
        )? {
            Ok(p) => p,
            Err(gate) => return Ok(UpdateOutcome::Gate(gate)),
        };
        let schema = match collection_repo::get_schema_by_name(conn, plugin.id, &path.name) {
            Ok(s) => s,
            Err(DieselError::NotFound) => return Ok(UpdateOutcome::CollectionNotFound),
            Err(e) => return Err(e),
        };

        // Validate updated data
        if let Ok(definition) =
            serde_json::from_value::<crate::models::CollectionDefinition>(schema.schema.clone())
        {
            if let Err(e) = validation::validate_row_data(&body_data, &definition) {
                return Ok(UpdateOutcome::ValidationError(e));
            }
        }

        let update = PluginCollectionRowUpdate {
            data: Some(body_data.clone()),
        };

        match collection_repo::update_row(conn, path.row_uuid, update) {
            Ok(row) => Ok::<_, DieselError>(UpdateOutcome::Ok(row)),
            Err(DieselError::NotFound) => Ok(UpdateOutcome::RowNotFound),
            Err(e) => Err(e),
        }
    });

    match outcome {
        Ok(UpdateOutcome::Ok(row)) => HttpResponse::Ok().json(CollectionRowResponse::from(row)),
        Ok(UpdateOutcome::Gate(gate)) => gate.into_response(),
        Ok(UpdateOutcome::CollectionNotFound) => errors::not_found_msg("Collection not found"),
        Ok(UpdateOutcome::RowNotFound) => errors::not_found_msg("Row not found"),
        Ok(UpdateOutcome::ValidationError(msg)) => {
            HttpResponse::BadRequest().json(serde_json::json!({
                "error": i18n::tr(&locale, "backend-error-validation"),
                "code": "backend-error-validation",
                "message": msg
            }))
        }
        Err(e) => {
            error!("Failed to update collection row: {}", e);
            errors::internal("Failed to update row")
        }
    }
}

/// Delete a row
pub async fn delete_collection_row(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<CollectionRowPath>,
) -> impl Responder {
    let _claims = match get_claims(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let path = path.into_inner();

    enum DeleteOutcome {
        Deleted,
        Gate(PluginGate),
        RowNotFound,
    }

    let outcome = tc.run(|conn| {
        match authorize_plugin_data_request(
            conn,
            path.uuid,
            "collection:write",
            "Plugin has not been granted collection write access",
        )? {
            Ok(_) => {}
            Err(gate) => return Ok(DeleteOutcome::Gate(gate)),
        }
        match collection_repo::delete_row(conn, path.row_uuid)? {
            n if n > 0 => Ok::<_, DieselError>(DeleteOutcome::Deleted),
            _ => Ok(DeleteOutcome::RowNotFound),
        }
    });

    match outcome {
        Ok(DeleteOutcome::Deleted) => HttpResponse::NoContent().finish(),
        Ok(DeleteOutcome::Gate(gate)) => gate.into_response(),
        Ok(DeleteOutcome::RowNotFound) => errors::not_found_msg("Row not found"),
        Err(e) => {
            error!("Failed to delete collection row: {}", e);
            errors::internal("Failed to delete row")
        }
    }
}
