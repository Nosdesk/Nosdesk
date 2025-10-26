use actix_web::{web, HttpResponse, HttpRequest, HttpMessage, Responder};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::db::Pool;
use crate::models::{NewDocumentationPage, DocumentationPageWithChildren, DocumentationStatus};
use crate::repository;
use crate::utils;

// Get all documentation pages
pub async fn get_documentation_pages(
    pool: web::Data<Pool>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::get_documentation_pages(&mut conn) {
        Ok(pages) => HttpResponse::Ok().json(pages),
        Err(_) => HttpResponse::InternalServerError().json("Failed to fetch pages"),
    }
}

// Get a single documentation page by ID
pub async fn get_documentation_page(
    id: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let page_id = id.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::get_documentation_page(page_id, &mut conn) {
        Ok(page) => HttpResponse::Ok().json(page),
        Err(_) => HttpResponse::NotFound().json("Page not found"),
    }
}

// Get a documentation page by its slug
pub async fn get_documentation_page_by_slug(
    slug: web::Path<String>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let page_slug = slug.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::get_documentation_page_by_slug(&page_slug, &mut conn) {
        Ok(page) => HttpResponse::Ok().json(page),
        Err(_) => HttpResponse::NotFound().json("Page not found"),
    }
}

// Create a new documentation page
pub async fn create_documentation_page(
    req: HttpRequest,
    page: web::Json<NewDocumentationPage>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    let mut new_page = page.into_inner();
    
    // Set the created_by and last_edited_by fields to the authenticated user's UUID
    let user_uuid = match utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID in token"),
    };
    
    new_page.created_by = user_uuid;
    new_page.last_edited_by = user_uuid;
    
    // Set a default display_order if not provided
    if new_page.display_order.is_none() {
        new_page.display_order = Some(0);
    }

    match repository::create_documentation_page(new_page, &mut conn) {
        Ok(created_page) => HttpResponse::Created().json(created_page),
        Err(_) => HttpResponse::InternalServerError().json("Failed to create page"),
    }
}

// Update an existing documentation page
pub async fn update_documentation_page(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    page: web::Json<NewDocumentationPage>,
) -> impl Responder {
    let page_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    // Check if the page exists and get its current state
    match repository::get_documentation_page(page_id, &mut conn) {
        Ok(existing_page) => {
            // Get the user UUID for last_edited_by
            let user_uuid = match utils::parse_uuid(&claims.sub) {
                Ok(uuid) => uuid,
                Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID in token"),
            };
            
            // Create update struct with the fields from the request
            let new_page = page.into_inner();
            let page_update = crate::models::DocumentationPageUpdate {
                title: Some(new_page.title),
                slug: new_page.slug,
                icon: new_page.icon,
                cover_image: new_page.cover_image,
                status: Some(new_page.status),
                last_edited_by: Some(user_uuid),
                parent_id: Some(new_page.parent_id),
                ticket_id: Some(new_page.ticket_id),
                display_order: new_page.display_order,
                is_public: Some(new_page.is_public),
                is_template: Some(new_page.is_template),
                archived_at: None,
                yjs_state_vector: new_page.yjs_state_vector,
                yjs_document: new_page.yjs_document,
                yjs_client_id: new_page.yjs_client_id,
                estimated_reading_time: new_page.estimated_reading_time,
                word_count: new_page.word_count,
                has_unsaved_changes: Some(new_page.has_unsaved_changes),
                updated_at: Some(chrono::Utc::now().naive_utc()),
            };

            // Update the page
            match repository::update_documentation_page(&mut conn, page_id, &page_update) {
                Ok(updated_page) => {
                    println!("Documentation page updated: {}", updated_page.id);
                    HttpResponse::Ok().json(updated_page)
                },
                Err(e) => {
                    println!("Error updating documentation page: {:?}", e);
                    HttpResponse::InternalServerError().json("Failed to update documentation page")
                },
            }
        },
        Err(_) => HttpResponse::NotFound().json("Documentation page not found"),
    }
}

// Delete a documentation page
pub async fn delete_documentation_page(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    let page_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    // Check if the page exists
    match repository::get_documentation_page(page_id, &mut conn) {
        Ok(_) => {
            // Delete the page
            match repository::delete_documentation_page(page_id, &mut conn) {
                Ok(_) => {
                    println!("Documentation page deleted by {}: {}", claims.name, page_id);
                    HttpResponse::NoContent().finish()
                },
                Err(e) => {
                    println!("Error deleting documentation page: {:?}", e);
                    HttpResponse::InternalServerError().json("Failed to delete documentation page")
                },
            }
        },
        Err(_) => HttpResponse::NotFound().json("Documentation page not found"),
    }
}

// Get top-level documentation pages
pub async fn get_top_level_documentation_pages(
    pool: web::Data<Pool>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::get_top_level_pages(&mut conn) {
        Ok(pages) => HttpResponse::Ok().json(pages),
        Err(_) => HttpResponse::InternalServerError().json("Failed to fetch top-level pages"),
    }
}

// Get documentation pages by parent ID
pub async fn get_documentation_pages_by_parent_id(
    parent_id: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let parent = parent_id.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::get_pages_by_parent_id(parent, &mut conn) {
        Ok(pages) => HttpResponse::Ok().json(pages),
        Err(_) => HttpResponse::InternalServerError().json("Failed to fetch pages by parent ID"),
    }
}

// Get a page with its children by parent ID
pub async fn get_page_with_children_by_parent_id(
    id: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let page_id = id.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // First get the page
    let page = match repository::get_documentation_page(page_id, &mut conn) {
        Ok(page) => page,
        Err(_) => return HttpResponse::NotFound().json("Page not found"),
    };

    // Then get its children
    let children = match repository::get_pages_by_parent_id(page_id, &mut conn) {
        Ok(children) => children,
        Err(_) => return HttpResponse::InternalServerError().json("Failed to fetch children"),
    };

    // Combine into a single response
    let page_with_children = DocumentationPageWithChildren {
        page,
        children,
    };

    HttpResponse::Ok().json(page_with_children)
}

// Get a page with its ordered children
pub async fn get_page_with_ordered_children(
    id: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let page_id = id.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::get_page_with_ordered_children(&mut conn, page_id) {
        Ok(page_with_children) => HttpResponse::Ok().json(page_with_children),
        Err(_) => HttpResponse::NotFound().json("Page not found or error fetching children"),
    }
}

// Get ordered documentation pages by parent ID
pub async fn get_ordered_pages_by_parent_id(
    parent_id: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let parent = parent_id.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::get_ordered_pages_by_parent_id(&mut conn, parent) {
        Ok(pages) => HttpResponse::Ok().json(pages),
        Err(_) => HttpResponse::InternalServerError().json("Failed to fetch ordered pages by parent ID"),
    }
}

#[derive(Deserialize)]
pub struct ReorderPagesRequest {
    pub parent_id: i32,
    pub page_orders: Vec<crate::models::PageOrder>,
}

// Reorder pages under a parent
pub async fn reorder_pages(
    pool: web::Data<Pool>,
    request: web::Json<ReorderPagesRequest>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::reorder_pages(&mut conn, Some(request.parent_id), &request.page_orders) {
        Ok(updated_pages) => HttpResponse::Ok().json(updated_pages),
        Err(e) => {
            eprintln!("Error reordering pages: {:?}", e);
            HttpResponse::InternalServerError().json("Failed to reorder pages")
        }
    }
}

#[derive(Deserialize)]
pub struct MovePageRequest {
    pub page_id: i32,
    pub new_parent_id: Option<i32>,
    pub display_order: Option<i32>,
}

// Move a page to a new parent
pub async fn move_page_to_parent(
    pool: web::Data<Pool>,
    request: web::Json<MovePageRequest>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let display_order = request.display_order.unwrap_or(0);
    
    match repository::move_page_to_parent(&mut conn, request.page_id, request.new_parent_id, display_order) {
        Ok(page) => HttpResponse::Ok().json(page),
        Err(e) => {
            eprintln!("Error moving page: {:?}", e);
            HttpResponse::InternalServerError().json("Failed to move page to new parent")
        }
    }
}

// Get top-level pages (with ordering)
pub async fn get_ordered_top_level_pages(
    pool: web::Data<Pool>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Get all top-level pages with appropriate ordering
    match repository::get_ordered_top_level_pages(&mut conn) {
        Ok(pages) => HttpResponse::Ok().json(pages),
        Err(_) => HttpResponse::InternalServerError().json("Failed to fetch top-level pages"),
    }
}

// Get documentation page by slug with its children
pub async fn get_documentation_page_by_slug_with_children(
    slug: web::Path<String>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let page_slug = slug.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // First get the page by slug
    let page = match repository::get_documentation_page_by_slug(&page_slug, &mut conn) {
        Ok(page) => page,
        Err(_) => return HttpResponse::NotFound().json("Page not found"),
    };

    // Then get its children
    let children = match repository::get_pages_by_parent_id(page.id, &mut conn) {
        Ok(children) => children,
        Err(_) => return HttpResponse::InternalServerError().json("Failed to fetch children"),
    };

    // Combine into a single response
    let page_with_children = DocumentationPageWithChildren {
        page,
        children,
    };

    HttpResponse::Ok().json(page_with_children)
}

// Get documentation pages for a ticket
pub async fn get_documentation_pages_by_ticket_id(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    let ticket_id = path.into_inner();
    
    // Get a connection from the pool
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };
    
    match repository::get_documentation_pages_by_ticket_id(&mut conn, ticket_id) {
        Ok(pages) => {
            println!("Found {} documentation pages for ticket {}", pages.len(), ticket_id);
            HttpResponse::Ok().json(pages)
        },
        Err(e) => {
            println!("Error fetching documentation pages for ticket {}: {:?}", ticket_id, e);
            HttpResponse::InternalServerError().json("Failed to fetch documentation pages")
        }
    }
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct CreateDocPageFromTicket {
    pub title: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub parent_id: Option<i32>,
}

// Create a documentation page from a ticket's article content
pub async fn create_documentation_page_from_ticket(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    page_data: web::Json<CreateDocPageFromTicket>,
) -> impl Responder {
    let ticket_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    // Get the ticket's article content
    let article_content = match repository::get_article_content_by_ticket_id(&mut conn, ticket_id) {
        Ok(content) => content,
        Err(_) => return HttpResponse::NotFound().json("Article content not found for ticket"),
    };

    // Generate a slug from the title
    let slug = page_data.title.to_lowercase().replace(" ", "-");

    let now = Utc::now().naive_utc();
    
    // Get the user UUID for created_by and last_edited_by
    let user_uuid = match utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID in token"),
    };

    let new_page = NewDocumentationPage {
        uuid: Uuid::new_v4(),
        title: page_data.title.clone(),
        slug: Some(slug),
        icon: page_data.icon.clone(),
        cover_image: None,
        status: DocumentationStatus::Draft,
        created_by: user_uuid,
        last_edited_by: user_uuid,
        parent_id: page_data.parent_id,
        ticket_id: Some(ticket_id),
        display_order: Some(0),
        is_public: false,
        is_template: false,
        yjs_state_vector: None,
        yjs_document: None,
        yjs_client_id: None,
        estimated_reading_time: None,
        word_count: None,
        has_unsaved_changes: false,
    };

    // Create the documentation page
    match repository::create_documentation_page(new_page, &mut conn) {
        Ok(page) => HttpResponse::Created().json(page),
        Err(e) => {
            eprintln!("Error creating documentation page: {:?}", e);
            HttpResponse::InternalServerError().json("Failed to create documentation page")
        }
    }
} 