//! Indexing logic for each entity type

use diesel::prelude::*;
use tantivy::{doc, IndexWriter, Term};
use tracing::{info, warn};

use crate::db::DbConnection;
use crate::models;

use super::extractors::{create_preview, extract_text_from_yjs, strip_html_and_mentions};
use super::schema::SearchSchema;
use super::types::{EntityType, IndexDocument};

/// Create an index document from a ticket with its article content
pub fn index_document_from_ticket(
    ticket: &models::Ticket,
    article_content: Option<&models::ArticleContent>,
) -> IndexDocument {
    // Extract text from Yjs document if available (from associated article_content)
    let content = article_content
        .and_then(|ac| ac.yjs_document.as_ref())
        .and_then(|data| extract_text_from_yjs(data))
        .unwrap_or_default();

    let preview = if !content.is_empty() {
        create_preview(&content, 150)
    } else {
        String::new()
    };

    // Build metadata from ticket fields. The workflow-state category is
    // the searchable status token; the cached lookup avoids a DB roundtrip
    // on the indexing hot path. Falls back to "backlog" if the cache is
    // cold (the next reindex picks up the correct value).
    let category_str =
        crate::repository::workflow_states::category_of_cached(ticket.workflow_state_id)
            .map(|c| c.as_str())
            .unwrap_or("backlog");
    let priority_str = ticket.priority.as_str();
    let metadata = format!("{} {}", category_str, priority_str);

    IndexDocument::new(EntityType::Ticket, ticket.id as i64, &ticket.title, content)
        .metadata(metadata)
        .url(format!("/tickets/{}", ticket.id))
        .preview(preview)
        .updated_at(ticket.updated_at.and_utc().timestamp())
}

/// Create an index document from a comment
pub fn index_document_from_comment(comment: &models::Comment, ticket_title: &str) -> IndexDocument {
    let plain_content = strip_html_and_mentions(&comment.content);
    let preview = create_preview(&plain_content, 150);

    // Include ticket reference in title for context
    let title = format!("Comment on: {}", ticket_title);

    IndexDocument::new(EntityType::Comment, comment.id as i64, title, plain_content)
        .url(format!("/tickets/{}", comment.ticket_id))
        .preview(preview)
        .updated_at(comment.created_at.and_utc().timestamp())
        .is_internal(comment.is_internal)
}

/// Create an index document from a documentation page
pub fn index_document_from_documentation(doc_page: &models::DocumentationPage) -> IndexDocument {
    // Extract text from Yjs document if available
    let content = doc_page
        .yjs_document
        .as_ref()
        .and_then(|data| extract_text_from_yjs(data))
        .unwrap_or_default();

    let preview = if !content.is_empty() {
        create_preview(&content, 150)
    } else {
        String::new()
    };

    let url = format!("/documentation/{}", doc_page.slug);

    IndexDocument::new(
        EntityType::Documentation,
        doc_page.id as i64,
        &doc_page.title,
        content,
    )
    .url(url)
    .preview(preview)
    .updated_at(doc_page.updated_at.and_utc().timestamp())
}

/// Create an index document from an attachment
pub fn index_document_from_attachment(
    attachment: &models::Attachment,
    ticket_id: i32,
    ticket_title: &str,
) -> IndexDocument {
    // Use transcription as content if available
    let content = attachment.transcription.clone().unwrap_or_default();
    let preview = if !content.is_empty() {
        create_preview(&content, 150)
    } else {
        attachment.name.clone()
    };

    // Build metadata
    let mut metadata_parts = vec![attachment.name.clone()];
    if let Some(ref mime_type) = attachment.mime_type {
        metadata_parts.push(mime_type.clone());
    }

    let title = format!("Attachment: {} (on {})", attachment.name, ticket_title);

    IndexDocument::new(EntityType::Attachment, attachment.id as i64, title, content)
        .metadata(metadata_parts.join(" "))
        .url(format!("/tickets/{}", ticket_id))
        .preview(preview)
        .updated_at(chrono::Utc::now().timestamp())
}

/// Create an index document from a device
pub fn index_document_from_device(device: &models::Asset) -> IndexDocument {
    // The IT-flavoured fields (hostname, OS, etc.) used to live
    // as top-level columns; Pass B moved them into the
    // attributes JSONB. Read through the typed accessors so the
    // search surface stays consistent across IT and non-IT
    // kinds without re-reaching into Value indexing here.
    use crate::services::assets::it_attrs;

    // Asset has `name` field (not optional)
    let hostname_attr = it_attrs::hostname(&device.attributes);
    let title = if !device.name.is_empty() {
        device.name.clone()
    } else if let Some(hostname) = hostname_attr {
        hostname.to_string()
    } else {
        format!("Asset #{}", device.id)
    };

    // Build comprehensive metadata for device search
    let mut metadata_parts = Vec::new();
    metadata_parts.push(device.name.clone());
    if let Some(hostname) = hostname_attr {
        metadata_parts.push(hostname.to_string());
    }
    if let Some(ref serial) = device.serial_number {
        metadata_parts.push(serial.clone());
    }
    if let Some(ref manufacturer) = device.manufacturer {
        metadata_parts.push(manufacturer.clone());
    }
    if let Some(ref model) = device.model {
        metadata_parts.push(model.clone());
    }
    if let Some(os) = it_attrs::operating_system(&device.attributes) {
        metadata_parts.push(os.to_string());
    }
    if let Some(os_version) = it_attrs::os_version(&device.attributes) {
        metadata_parts.push(os_version.to_string());
    }
    if let Some(ref asset_tag) = device.asset_tag {
        metadata_parts.push(asset_tag.clone());
    }
    // Discriminator slug. Indexing the slug rather than the
    // registry label avoids a per-row lookup; admins searching
    // for "license" or "vehicle" find the matching rows even
    // when the kind is non-IT.
    metadata_parts.push(device.kind.clone());
    // Quantity + unit as a single token ("50 m") so bulk
    // materials are searchable by their measured amount.
    if let Some(ref qty) = device.quantity {
        let pretty = match device.unit.as_deref() {
            Some(unit) if !unit.is_empty() => format!("{qty} {unit}"),
            _ => qty.to_string(),
        };
        metadata_parts.push(pretty);
    } else if let Some(ref unit) = device.unit {
        metadata_parts.push(unit.clone());
    }
    // Attribute values. Stringified scalars are appended so a
    // search for "22mm copper" hits a material whose attributes
    // contain those values. Object / array values are skipped
    // because they need per-kind interpretation that the search
    // layer shouldn't bake in.
    if let Some(obj) = device.attributes.as_object() {
        for v in obj.values() {
            if let Some(s) = v.as_str() {
                if !s.is_empty() {
                    metadata_parts.push(s.to_string());
                }
            } else if v.is_number() || v.is_boolean() {
                metadata_parts.push(v.to_string());
            }
        }
    }

    let preview = metadata_parts
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(" | ");

    IndexDocument::new(EntityType::Asset, device.id as i64, title, "")
        .metadata(metadata_parts.join(" "))
        // Canonical URL now lives at /assets/:id; the /devices
        // path resolves via a frontend redirect but the new
        // path is what we want bookmarks and search clicks to
        // land on.
        .url(format!("/assets/{}", device.id))
        .preview(preview)
        .updated_at(device.updated_at.and_utc().timestamp())
}

/// Create an index document from a user
/// Note: User doesn't have email/department/title directly - email is in user_emails table
pub fn index_document_from_user(user: &models::User, primary_email: Option<&str>) -> IndexDocument {
    // Build metadata from available user fields
    let mut metadata_parts = Vec::new();
    if let Some(email) = primary_email {
        metadata_parts.push(email.to_string());
    }

    let preview = primary_email.unwrap_or("").to_string();

    IndexDocument::with_uuid(EntityType::User, &user.uuid.to_string(), &user.name)
        .metadata(metadata_parts.join(" "))
        .url(format!("/users/{}", user.uuid))
        .preview(preview)
        .updated_at(user.updated_at.and_utc().timestamp())
}

/// Create an index document from a project. Title = name, content +
/// preview = description, metadata = lifecycle status.
pub fn index_document_from_project(project: &models::Project) -> IndexDocument {
    let status = match project.status {
        models::ProjectStatus::Active => "active",
        models::ProjectStatus::Completed => "completed",
        models::ProjectStatus::Archived => "archived",
    };
    let description = project.description.clone().unwrap_or_default();

    IndexDocument::new(
        EntityType::Project,
        project.id as i64,
        &project.name,
        description.clone(),
    )
    .metadata(status)
    .url(format!("/projects/{}", project.id))
    .preview(description)
    .updated_at(project.updated_at.and_utc().timestamp())
}

/// Add a document to the index
pub fn add_document_to_index(
    writer: &IndexWriter,
    schema: &SearchSchema,
    doc: &IndexDocument,
) -> tantivy::Result<()> {
    // First, delete any existing document with the same ID
    let id_term = Term::from_field_text(schema.id, &doc.id);
    writer.delete_term(id_term);

    // Add the new document
    writer.add_document(doc!(
        schema.id => doc.id.clone(),
        schema.entity_type => doc.entity_type.as_str(),
        schema.entity_id => doc.entity_id,
        schema.title => doc.title.clone(),
        schema.content => doc.content.clone(),
        schema.metadata => doc.metadata.clone(),
        schema.url => doc.url.clone(),
        schema.preview => doc.preview.clone(),
        schema.updated_at => doc.updated_at,
        schema.is_internal => if doc.is_internal { 1i64 } else { 0i64 },
    ))?;

    Ok(())
}

/// Delete a document from the index by its composite ID key (e.g. "ticket-123" or "user-uuid")
pub fn delete_document_from_index(
    writer: &IndexWriter,
    schema: &SearchSchema,
    entity_type: EntityType,
    key: &str,
) -> tantivy::Result<()> {
    let id = format!("{}-{}", entity_type.as_str(), key);
    let id_term = Term::from_field_text(schema.id, &id);
    writer.delete_term(id_term);
    Ok(())
}

/// Rebuild the entire index from the database
pub fn rebuild_index(
    conn: &mut DbConnection,
    writer: &IndexWriter,
    schema: &SearchSchema,
) -> Result<IndexStats, Box<dyn std::error::Error + Send + Sync>> {
    use crate::schema::{
        article_contents, assets, attachments, comments, documentation_pages, projects, tickets,
        user_emails, users,
    };

    info!("Starting full index rebuild");

    let mut stats = IndexStats {
        tickets: 0,
        comments: 0,
        documentation: 0,
        attachments: 0,
        devices: 0,
        users: 0,
        projects: 0,
    };

    // Index all tickets with their article contents
    let all_tickets: Vec<models::Ticket> = tickets::table.load(conn)?;
    let all_article_contents: Vec<models::ArticleContent> = article_contents::table.load(conn)?;

    // Build a map of ticket_id to article_content
    let article_content_map: std::collections::HashMap<i32, &models::ArticleContent> =
        all_article_contents
            .iter()
            .filter_map(|ac| ac.ticket_id.map(|tid| (tid, ac)))
            .collect();

    info!(count = all_tickets.len(), "Indexing tickets");
    for ticket in &all_tickets {
        let article_content = article_content_map.get(&ticket.id).copied();
        let doc = index_document_from_ticket(ticket, article_content);
        if let Err(e) = add_document_to_index(writer, schema, &doc) {
            warn!(ticket_id = ticket.id, error = ?e, "Failed to index ticket");
        } else {
            stats.tickets += 1;
        }
    }

    // Build a map of ticket IDs to titles for comments
    let ticket_titles: std::collections::HashMap<i32, String> = all_tickets
        .iter()
        .map(|t| (t.id, t.title.clone()))
        .collect();

    // Index all comments
    let all_comments: Vec<models::Comment> = comments::table.load(conn)?;
    info!(count = all_comments.len(), "Indexing comments");
    for comment in &all_comments {
        let ticket_title = ticket_titles
            .get(&comment.ticket_id)
            .map(|s| s.as_str())
            .unwrap_or("Unknown Ticket");
        let doc = index_document_from_comment(comment, ticket_title);
        if let Err(e) = add_document_to_index(writer, schema, &doc) {
            warn!(comment_id = comment.id, error = ?e, "Failed to index comment");
        } else {
            stats.comments += 1;
        }
    }

    // Index all attachments with transcriptions
    let all_attachments: Vec<models::Attachment> = attachments::table
        .filter(attachments::transcription.is_not_null())
        .load(conn)?;
    info!(
        count = all_attachments.len(),
        "Indexing attachments with transcriptions"
    );
    for attachment in &all_attachments {
        if let Some(comment_id) = attachment.comment_id {
            // Get the ticket_id from the comment
            if let Ok(comment) = comments::table
                .find(comment_id)
                .first::<models::Comment>(conn)
            {
                let ticket_title = ticket_titles
                    .get(&comment.ticket_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown Ticket");
                let doc =
                    index_document_from_attachment(attachment, comment.ticket_id, ticket_title);
                if let Err(e) = add_document_to_index(writer, schema, &doc) {
                    warn!(attachment_id = attachment.id, error = ?e, "Failed to index attachment");
                } else {
                    stats.attachments += 1;
                }
            }
        }
    }

    // Index all documentation pages
    let all_docs: Vec<models::DocumentationPage> = documentation_pages::table.load(conn)?;
    info!(count = all_docs.len(), "Indexing documentation pages");
    for doc_page in &all_docs {
        let doc = index_document_from_documentation(doc_page);
        if let Err(e) = add_document_to_index(writer, schema, &doc) {
            warn!(doc_id = doc_page.id, error = ?e, "Failed to index documentation");
        } else {
            stats.documentation += 1;
        }
    }

    // Index all devices
    let all_devices: Vec<models::Asset> = assets::table.load(conn)?;
    info!(count = all_devices.len(), "Indexing devices");
    for device in &all_devices {
        let doc = index_document_from_device(device);
        if let Err(e) = add_document_to_index(writer, schema, &doc) {
            warn!(device_id = device.id, error = ?e, "Failed to index device");
        } else {
            stats.devices += 1;
        }
    }

    // Index all users with their primary emails
    let all_users: Vec<models::User> = users::table.load(conn)?;

    // Get primary emails for all users
    let primary_emails: Vec<models::UserEmail> = user_emails::table
        .filter(user_emails::is_primary.eq(true))
        .load(conn)?;

    let email_map: std::collections::HashMap<uuid::Uuid, String> = primary_emails
        .into_iter()
        .map(|ue| (ue.user_uuid, ue.email))
        .collect();

    info!(count = all_users.len(), "Indexing users");
    for user in &all_users {
        let primary_email = email_map.get(&user.uuid).map(|s| s.as_str());
        let doc = index_document_from_user(user, primary_email);
        if let Err(e) = add_document_to_index(writer, schema, &doc) {
            warn!(user_uuid = %user.uuid, error = ?e, "Failed to index user");
        } else {
            stats.users += 1;
        }
    }

    // Index all projects
    let all_projects: Vec<models::Project> = projects::table.load(conn)?;
    info!(count = all_projects.len(), "Indexing projects");
    for project in &all_projects {
        let doc = index_document_from_project(project);
        if let Err(e) = add_document_to_index(writer, schema, &doc) {
            warn!(project_id = project.id, error = ?e, "Failed to index project");
        } else {
            stats.projects += 1;
        }
    }

    // Note: Caller is responsible for committing the writer

    info!(
        tickets = stats.tickets,
        comments = stats.comments,
        documentation = stats.documentation,
        attachments = stats.attachments,
        devices = stats.devices,
        users = stats.users,
        projects = stats.projects,
        "Index rebuild complete"
    );

    Ok(stats)
}

/// Statistics from an index rebuild operation
#[derive(Debug, Default)]
pub struct IndexStats {
    pub tickets: usize,
    pub comments: usize,
    pub documentation: usize,
    pub attachments: usize,
    pub devices: usize,
    pub users: usize,
    pub projects: usize,
}

impl IndexStats {
    pub fn total(&self) -> usize {
        self.tickets
            + self.comments
            + self.documentation
            + self.attachments
            + self.devices
            + self.users
            + self.projects
    }
}
