use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error;
use serde::Deserialize;
use uuid::Uuid;

use crate::db::Pool;
use crate::models::{NewTicketCategory, TicketCategoryUpdate, Claims};
use crate::repository;
use crate::utils::rbac::require_admin;

// ============================================================================
// Category Endpoints for Regular Users
// ============================================================================

/// Get categories visible to the current user
pub async fn get_categories(
    req: HttpRequest,
    pool: web::Data<Pool>,
) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID"),
    };

    let is_admin = claims.role == "admin";

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::categories::get_categories_for_user(&mut conn, &user_uuid, is_admin) {
        Ok(categories) => HttpResponse::Ok().json(categories),
        Err(_) => HttpResponse::InternalServerError().json("Failed to get categories"),
    }
}

// ============================================================================
// Admin Category CRUD Endpoints
// ============================================================================

/// Get all categories with visibility info (admin only)
pub async fn get_all_categories_admin(
    req: HttpRequest,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::categories::get_all_categories_with_visibility(&mut conn) {
        Ok(categories) => HttpResponse::Ok().json(categories),
        Err(_) => HttpResponse::InternalServerError().json("Failed to get categories"),
    }
}

/// Get a single category with visibility info (admin only)
pub async fn get_category_admin(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let category_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::categories::get_category_with_visibility(&mut conn, category_id) {
        Ok(category) => HttpResponse::Ok().json(category),
        Err(e) => match e {
            Error::NotFound => HttpResponse::NotFound().json("Category not found"),
            _ => HttpResponse::InternalServerError().json("Failed to get category"),
        },
    }
}

/// Request body for creating a category
#[derive(Debug, Deserialize)]
pub struct CreateCategoryRequest {
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub visible_to_group_ids: Option<Vec<i32>>, // If None or empty, category is public
}

/// Create a new category (admin only)
pub async fn create_category(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<CreateCategoryRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    let created_by = Uuid::parse_str(&claims.sub).ok();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Get next display order
    let display_order = repository::categories::get_next_display_order(&mut conn).unwrap_or_default();

    let new_category = NewTicketCategory {
        name: body.name.clone(),
        description: body.description.clone(),
        color: body.color.clone(),
        icon: body.icon.clone(),
        display_order,
        is_active: true,
        created_by,
    };

    match repository::categories::create_category(&mut conn, new_category) {
        Ok(category) => {
            // Set visibility if specified
            if let Some(ref group_ids) = body.visible_to_group_ids {
                if !group_ids.is_empty() {
                    if let Err(_) = repository::categories::set_category_visibility(
                        &mut conn,
                        category.id,
                        group_ids.clone(),
                        created_by,
                    ) {
                        return HttpResponse::InternalServerError()
                            .json("Failed to set category visibility");
                    }
                }
            }

            // Return category with visibility info
            match repository::categories::get_category_with_visibility(&mut conn, category.id) {
                Ok(category_with_vis) => HttpResponse::Created().json(category_with_vis),
                Err(_) => HttpResponse::Created().json(category),
            }
        }
        Err(_) => HttpResponse::InternalServerError().json("Failed to create category"),
    }
}

/// Request body for updating a category
#[derive(Debug, Deserialize)]
pub struct UpdateCategoryRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub is_active: Option<bool>,
    pub visible_to_group_ids: Option<Vec<i32>>, // If provided, replaces existing visibility
}

/// Update an existing category (admin only)
pub async fn update_category(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<UpdateCategoryRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    let updated_by = Uuid::parse_str(&claims.sub).ok();
    let category_id = path.into_inner();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let category_update = TicketCategoryUpdate {
        name: body.name.clone(),
        description: body.description.clone(),
        color: body.color.clone(),
        icon: body.icon.clone(),
        display_order: None,
        is_active: body.is_active,
        updated_at: None,
    };

    match repository::categories::update_category(&mut conn, category_id, category_update) {
        Ok(_) => {
            // Update visibility if specified
            if let Some(ref group_ids) = body.visible_to_group_ids {
                if let Err(_) = repository::categories::set_category_visibility(
                    &mut conn,
                    category_id,
                    group_ids.clone(),
                    updated_by,
                ) {
                    return HttpResponse::InternalServerError()
                        .json("Failed to update category visibility");
                }
            }

            // Return updated category with visibility info
            match repository::categories::get_category_with_visibility(&mut conn, category_id) {
                Ok(category) => HttpResponse::Ok().json(category),
                Err(_) => HttpResponse::InternalServerError().json("Failed to get updated category"),
            }
        }
        Err(e) => match e {
            Error::NotFound => HttpResponse::NotFound().json("Category not found"),
            _ => HttpResponse::InternalServerError().json("Failed to update category"),
        },
    }
}

/// Delete (soft) a category (admin only)
pub async fn delete_category(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let category_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::categories::delete_category(&mut conn, category_id) {
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => match e {
            Error::NotFound => HttpResponse::NotFound().json("Category not found"),
            _ => HttpResponse::InternalServerError().json("Failed to delete category"),
        },
    }
}

// ============================================================================
// Category Ordering
// ============================================================================

/// Request body for reordering categories
#[derive(Debug, Deserialize)]
pub struct ReorderCategoriesRequest {
    pub orders: Vec<CategoryOrder>,
}

#[derive(Debug, Deserialize)]
pub struct CategoryOrder {
    pub id: i32,
    pub display_order: i32,
}

/// Reorder categories (admin only)
pub async fn reorder_categories(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<ReorderCategoriesRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let orders: Vec<(i32, i32)> = body
        .orders
        .iter()
        .map(|o| (o.id, o.display_order))
        .collect();

    match repository::categories::update_category_orders(&mut conn, orders) {
        Ok(_) => {
            // Return all categories with updated order
            match repository::categories::get_all_categories_with_visibility(&mut conn) {
                Ok(categories) => HttpResponse::Ok().json(categories),
                Err(_) => HttpResponse::InternalServerError().json("Failed to get updated categories"),
            }
        }
        Err(_) => HttpResponse::InternalServerError().json("Failed to reorder categories"),
    }
}

// ============================================================================
// Category Visibility
// ============================================================================

/// Request body for setting category visibility
#[derive(Debug, Deserialize)]
pub struct SetCategoryVisibilityRequest {
    pub group_ids: Vec<i32>, // Empty array = public (visible to all)
}

/// Set category visibility (admin only)
pub async fn set_category_visibility(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<SetCategoryVisibilityRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    let created_by = Uuid::parse_str(&claims.sub).ok();
    let category_id = path.into_inner();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::categories::set_category_visibility(
        &mut conn,
        category_id,
        body.group_ids.clone(),
        created_by,
    ) {
        Ok(_) => {
            // Return updated category with visibility info
            match repository::categories::get_category_with_visibility(&mut conn, category_id) {
                Ok(category) => HttpResponse::Ok().json(category),
                Err(_) => HttpResponse::InternalServerError().json("Failed to get updated category"),
            }
        }
        Err(_) => HttpResponse::InternalServerError().json("Failed to set category visibility"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, App, http::StatusCode};
    use crate::test_helpers::{setup_test_pool, create_test_claims, TestFixtures};
    use crate::models::UserRole;

    /// Helper to create a test app with category routes
    fn test_app(pool: crate::db::Pool) -> App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        App::new()
            .app_data(web::Data::new(pool))
            .route("/categories", web::get().to(get_categories))
            .route("/admin/categories", web::get().to(get_all_categories_admin))
            .route("/admin/categories", web::post().to(create_category))
            .route("/admin/categories/{id}", web::get().to(get_category_admin))
            .route("/admin/categories/{id}", web::put().to(update_category))
            .route("/admin/categories/{id}", web::delete().to(delete_category))
    }

    #[actix_web::test]
    async fn get_categories_requires_auth() {
        let pool = setup_test_pool();
        let app = test::init_service(test_app(pool)).await;

        let req = test::TestRequest::get().uri("/categories").to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn get_categories_with_auth_succeeds() {
        let pool = setup_test_pool();
        let mut conn = pool.get().unwrap();

        // Create test user
        let user = TestFixtures::create_user(&mut conn, "catuser", UserRole::User);
        let claims = create_test_claims(&user);

        let app = test::init_service(test_app(pool.clone())).await;

        // Create request with auth claims injected
        let req = test::TestRequest::get()
            .uri("/categories")
            .to_request();

        // Inject claims into extensions
        req.extensions_mut().insert(claims);

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn admin_categories_requires_admin_role() {
        let pool = setup_test_pool();
        let mut conn = pool.get().unwrap();

        // Create regular user (not admin)
        let user = TestFixtures::create_user(&mut conn, "regularuser", UserRole::User);
        let claims = create_test_claims(&user);

        let app = test::init_service(test_app(pool.clone())).await;

        let req = test::TestRequest::get()
            .uri("/admin/categories")
            .to_request();
        req.extensions_mut().insert(claims);

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[actix_web::test]
    async fn admin_can_get_all_categories() {
        let pool = setup_test_pool();
        let mut conn = pool.get().unwrap();

        // Create admin user
        let admin = TestFixtures::create_user(&mut conn, "admincat", UserRole::Admin);
        let claims = create_test_claims(&admin);

        let app = test::init_service(test_app(pool.clone())).await;

        let req = test::TestRequest::get()
            .uri("/admin/categories")
            .to_request();
        req.extensions_mut().insert(claims);

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn admin_can_create_category() {
        let pool = setup_test_pool();
        let mut conn = pool.get().unwrap();

        let admin = TestFixtures::create_user(&mut conn, "createcat", UserRole::Admin);
        let claims = create_test_claims(&admin);

        let app = test::init_service(test_app(pool.clone())).await;

        let req = test::TestRequest::post()
            .uri("/admin/categories")
            .set_json(serde_json::json!({
                "name": "Test Category",
                "description": "A test category"
            }))
            .to_request();
        req.extensions_mut().insert(claims);

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Parse response to verify category was created
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["name"], "Test Category");
    }

    #[actix_web::test]
    async fn admin_can_delete_category() {
        let pool = setup_test_pool();
        let mut conn = pool.get().unwrap();

        let admin = TestFixtures::create_user(&mut conn, "delcat", UserRole::Admin);
        let category = TestFixtures::create_category(&mut conn, "DeleteMe");
        let claims = create_test_claims(&admin);

        let app = test::init_service(test_app(pool.clone())).await;

        let req = test::TestRequest::delete()
            .uri(&format!("/admin/categories/{}", category.id))
            .to_request();
        req.extensions_mut().insert(claims);

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}
