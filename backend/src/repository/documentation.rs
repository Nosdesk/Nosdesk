use diesel::prelude::*;
use diesel::result::Error;
use diesel::sql_types::{Integer, Nullable};

use crate::db::DbConnection;
use crate::models::{
    DocumentationPage, DocumentationPageWithChildren,
    NewDocumentationPage, DocumentationPageUpdate, PageOrder
};
use crate::schema::documentation_pages;

// Get all documentation pages
pub fn get_documentation_pages(conn: &mut DbConnection) -> Result<Vec<DocumentationPage>, Error> {
    documentation_pages::table
        .order_by(documentation_pages::title.asc())
        .load::<DocumentationPage>(conn)
}

// Get a specific documentation page by ID
pub fn get_documentation_page(id: i32, conn: &mut DbConnection) -> Result<DocumentationPage, Error> {
    documentation_pages::table
        .find(id)
        .first::<DocumentationPage>(conn)
}

// Get a documentation page by its slug
pub fn get_documentation_page_by_slug(slug: &str, conn: &mut DbConnection) -> Result<DocumentationPage, Error> {
    documentation_pages::table
        .filter(documentation_pages::slug.eq(slug))
        .first::<DocumentationPage>(conn)
}

// Create a new documentation page
pub fn create_documentation_page(
    page: NewDocumentationPage,
    conn: &mut DbConnection,
) -> Result<DocumentationPage, Error> {
    diesel::insert_into(documentation_pages::table)
        .values(page)
        .get_result(conn)
}

// Update an existing documentation page
pub fn update_documentation_page(
    conn: &mut DbConnection,
    page_id: i32,
    page_update: &DocumentationPageUpdate,
) -> Result<DocumentationPage, Error> {
    diesel::update(documentation_pages::table.find(page_id))
        .set(page_update)
        .get_result(conn)
}

// Delete a documentation page
pub fn delete_documentation_page(id: i32, conn: &mut DbConnection) -> Result<usize, Error> {
    diesel::delete(documentation_pages::table.find(id)).execute(conn)
}

// Get top-level documentation pages
pub fn get_top_level_pages(conn: &mut DbConnection) -> Result<Vec<DocumentationPage>, Error> {
    documentation_pages::table
        .filter(documentation_pages::parent_id.is_null())
        .order_by(documentation_pages::title.asc())
        .load::<DocumentationPage>(conn)
}

// Get documentation pages by parent ID
pub fn get_pages_by_parent_id(parent_id: i32, conn: &mut DbConnection) -> Result<Vec<DocumentationPage>, Error> {
    documentation_pages::table
        .filter(documentation_pages::parent_id.eq(parent_id))
        .order_by(documentation_pages::title.asc())
        .load::<DocumentationPage>(conn)
}

// Get documentation pages by ticket ID
pub fn get_documentation_pages_by_ticket_id(conn: &mut DbConnection, ticket_id: i32) -> Result<Vec<DocumentationPage>, Error> {
    documentation_pages::table
        .filter(documentation_pages::ticket_id.eq(ticket_id))
        .order_by(documentation_pages::title.asc())
        .load::<DocumentationPage>(conn)
}

// Define a SQL function for coalesce
diesel::define_sql_function! {
    fn coalesce(x: Nullable<Integer>, y: Integer) -> Integer;
}

// Get ordered top-level documentation pages
pub fn get_ordered_top_level_pages(
    conn: &mut DbConnection,
) -> Result<Vec<DocumentationPage>, Error> {
    documentation_pages::table
        .filter(documentation_pages::parent_id.is_null())
        .order_by(coalesce(documentation_pages::display_order, 0).asc())
        .load::<DocumentationPage>(conn)
}

// Get ordered documentation pages by parent ID
pub fn get_ordered_pages_by_parent_id(
    conn: &mut DbConnection,
    parent_id: i32,
) -> Result<Vec<DocumentationPage>, Error> {
    documentation_pages::table
        .filter(documentation_pages::parent_id.eq(parent_id))
        .order_by(coalesce(documentation_pages::display_order, 0).asc())
        .load::<DocumentationPage>(conn)
}

// Reorder documentation pages
pub fn reorder_pages(
    conn: &mut DbConnection,
    parent_id: Option<i32>,
    page_orders: &[PageOrder],
) -> Result<Vec<DocumentationPage>, Error> {
    // Begin transaction
    conn.transaction(|conn| {
        let mut updated_pages = Vec::new();
        
        for order in page_orders {
            // Update the page's display_order and ensure it has the correct parent_id
            let updated_page = diesel::update(documentation_pages::table.find(order.page_id))
                .set((
                    documentation_pages::display_order.eq(order.display_order),
                    documentation_pages::parent_id.eq(parent_id),
                ))
                .get_result::<DocumentationPage>(conn)?;
                
            updated_pages.push(updated_page);
        }
        
        Ok(updated_pages)
    })
}

// Check if a page is a descendant of another page (to prevent circular references)
fn is_descendant_of(
    conn: &mut DbConnection,
    page_id: i32,
    potential_ancestor_id: i32,
) -> Result<bool, Error> {
    // Get all descendants of the potential ancestor recursively
    let descendants = get_all_descendant_ids(conn, potential_ancestor_id)?;
    Ok(descendants.contains(&page_id))
}

// Get all descendant IDs of a page recursively
fn get_all_descendant_ids(
    conn: &mut DbConnection,
    page_id: i32,
) -> Result<Vec<i32>, Error> {
    let mut all_descendants = Vec::new();
    let mut pages_to_check = vec![page_id];

    while !pages_to_check.is_empty() {
        // Get direct children of all pages in the current batch
        let children: Vec<DocumentationPage> = documentation_pages::table
            .filter(documentation_pages::parent_id.eq_any(&pages_to_check))
            .load(conn)?;

        // Clear the pages to check and add the children's IDs
        pages_to_check.clear();
        for child in children {
            all_descendants.push(child.id);
            pages_to_check.push(child.id);
        }
    }

    Ok(all_descendants)
}

// Move a page to a new parent
pub fn move_page_to_parent(
    conn: &mut DbConnection,
    page_id: i32,
    new_parent_id: Option<i32>,
    display_order: i32,
) -> Result<DocumentationPage, Error> {
    // Begin transaction
    conn.transaction(|conn| {
        // Validation 1: Cannot move a page to be its own parent
        if new_parent_id == Some(page_id) {
            return Err(Error::RollbackTransaction);
        }

        // Validation 2: Cannot move a page to be a child of its own descendant
        // (this would create a circular reference)
        if let Some(parent_id) = new_parent_id {
            if is_descendant_of(conn, parent_id, page_id)? {
                return Err(Error::RollbackTransaction);
            }
        }

        // Update the page's parent_id and display_order
        let updated_page = diesel::update(documentation_pages::table.find(page_id))
            .set((
                documentation_pages::parent_id.eq(new_parent_id),
                documentation_pages::display_order.eq(display_order),
            ))
            .get_result::<DocumentationPage>(conn)?;

        Ok(updated_page)
    })
}

// Get page with ordered children
pub fn get_page_with_ordered_children(
    conn: &mut DbConnection,
    page_id: i32,
) -> Result<DocumentationPageWithChildren, Error> {
    let page = get_documentation_page(page_id, conn)?;
    let children = get_ordered_pages_by_parent_id(conn, page_id)?;

    Ok(DocumentationPageWithChildren {
        page,
        children,
    })
}

// ============= Yjs Collaboration Methods =============

// Update documentation page Yjs state (for WebSocket sync auto-save)
pub fn update_documentation_yjs_state(
    conn: &mut DbConnection,
    page_id: i32,
    yjs_document: Vec<u8>,
) -> Result<DocumentationPage, Error> {
    use crate::schema::documentation_pages::dsl;

    diesel::update(dsl::documentation_pages.find(page_id))
        .set((
            dsl::yjs_document.eq(Some(yjs_document)),
            dsl::updated_at.eq(diesel::dsl::now),
        ))
        .get_result(conn)
}

// Create a documentation revision snapshot
// Note: This is simplified - the schema doesn't have a revision number or contributed_by
// Creates a basic revision with just the snapshot and metadata
pub fn create_documentation_revision(
    conn: &mut DbConnection,
    page_id: i32,
    yjs_state_vector: Vec<u8>,
    yjs_document_content: Vec<u8>,
    contributed_by: Vec<Option<uuid::Uuid>>,
) -> Result<i32, Error> {
    use crate::schema::documentation_pages::dsl as doc_dsl;
    use crate::schema::documentation_revisions;

    conn.transaction(|conn| {
        // Get current revision number from the page and created_by user
        let page: DocumentationPage = doc_dsl::documentation_pages
            .find(page_id)
            .first(conn)?;

        // Get the latest revision number for this page
        let latest_revision: i32 = documentation_revisions::table
            .filter(documentation_revisions::page_id.eq(page_id))
            .select(diesel::dsl::max(documentation_revisions::revision_number))
            .first::<Option<i32>>(conn)?
            .unwrap_or(0);

        let new_revision_number = latest_revision + 1;

        // Use the first contributor or the created_by from the page
        let created_by = contributed_by.first()
            .and_then(|opt_uuid| *opt_uuid)
            .unwrap_or(page.created_by);

        // Insert new revision (schema has different fields than article_content_revisions)
        diesel::insert_into(documentation_revisions::table)
            .values((
                documentation_revisions::page_id.eq(page_id),
                documentation_revisions::revision_number.eq(new_revision_number),
                documentation_revisions::title.eq(&page.title), // Snapshot the title
                documentation_revisions::yjs_document_snapshot.eq(yjs_document_content),
                documentation_revisions::yjs_state_vector.eq(yjs_state_vector),
                documentation_revisions::created_by.eq(created_by),
            ))
            .execute(conn)?;

        Ok(new_revision_number)
    })
}

// Get all revisions for a documentation page
pub fn get_documentation_revisions(
    conn: &mut DbConnection,
    page_id: i32,
) -> Result<Vec<crate::models::DocumentationRevision>, Error> {
    use crate::schema::documentation_revisions::dsl;

    dsl::documentation_revisions
        .filter(dsl::page_id.eq(page_id))
        .order_by(dsl::revision_number.desc())
        .load(conn)
}

// Get a specific revision for a documentation page
pub fn get_documentation_revision(
    conn: &mut DbConnection,
    page_id: i32,
    revision_number: i32,
) -> Result<crate::models::DocumentationRevision, Error> {
    use crate::schema::documentation_revisions::dsl;

    dsl::documentation_revisions
        .filter(dsl::page_id.eq(page_id))
        .filter(dsl::revision_number.eq(revision_number))
        .first(conn)
}

// Get the latest revision for a documentation page
pub fn get_latest_documentation_revision(
    conn: &mut DbConnection,
    page_id: i32,
) -> Result<crate::models::DocumentationRevision, Error> {
    use crate::schema::documentation_revisions::dsl;

    dsl::documentation_revisions
        .filter(dsl::page_id.eq(page_id))
        .order_by(dsl::revision_number.desc())
        .first(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DocumentationStatus, UserRole};
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use uuid::Uuid;

    fn make_page(created_by: Uuid) -> NewDocumentationPage {
        NewDocumentationPage {
            uuid: Uuid::new_v4(),
            title: "Test Page".to_string(),
            slug: Some("test-page".to_string()),
            icon: None,
            cover_image: None,
            status: DocumentationStatus::Draft,
            created_by,
            last_edited_by: created_by,
            parent_id: None,
            ticket_id: None,
            display_order: None,
            is_public: false,
            is_template: false,
            yjs_state_vector: None,
            yjs_document: None,
            yjs_client_id: None,
            has_unsaved_changes: false,
        }
    }

    #[test]
    fn create_and_get_documentation_page() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "docuser", UserRole::Admin);

        let page = create_documentation_page(make_page(user.uuid), &mut conn).unwrap();
        let fetched = get_documentation_page(page.id, &mut conn).unwrap();
        assert_eq!(fetched.title, "Test Page");
    }

    #[test]
    fn get_by_slug() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "sluguser", UserRole::Admin);

        create_documentation_page(make_page(user.uuid), &mut conn).unwrap();
        let fetched = get_documentation_page_by_slug("test-page", &mut conn).unwrap();
        assert_eq!(fetched.title, "Test Page");
    }

    #[test]
    fn update_documentation_page_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "upduser", UserRole::Admin);

        let page = create_documentation_page(make_page(user.uuid), &mut conn).unwrap();
        let update = DocumentationPageUpdate {
            title: Some("Updated Title".to_string()),
            slug: None,
            icon: None,
            cover_image: None,
            status: None,
            last_edited_by: None,
            parent_id: None,
            ticket_id: None,
            display_order: None,
            is_public: None,
            is_template: None,
            archived_at: None,
            yjs_state_vector: None,
            yjs_document: None,
            yjs_client_id: None,
            has_unsaved_changes: None,
            updated_at: None,
        };
        let updated = update_documentation_page(&mut conn, page.id, &update).unwrap();
        assert_eq!(updated.title, "Updated Title");
    }

    #[test]
    fn delete_documentation_page_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "deluser", UserRole::Admin);

        let page = create_documentation_page(make_page(user.uuid), &mut conn).unwrap();
        let rows = delete_documentation_page(page.id, &mut conn).unwrap();
        assert_eq!(rows, 1);
        assert!(get_documentation_page(page.id, &mut conn).is_err());
    }

    #[test]
    fn top_level_pages_excludes_children() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "treeuser", UserRole::Admin);

        let parent = create_documentation_page(make_page(user.uuid), &mut conn).unwrap();

        let mut child_page = make_page(user.uuid);
        child_page.title = "Child Page".to_string();
        child_page.slug = Some("child-page".to_string());
        child_page.parent_id = Some(parent.id);
        create_documentation_page(child_page, &mut conn).unwrap();

        let top = get_top_level_pages(&mut conn).unwrap();
        assert!(top.iter().all(|p| p.parent_id.is_none()));
        assert!(top.iter().any(|p| p.id == parent.id));
    }
}
