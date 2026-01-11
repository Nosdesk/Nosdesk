-- Remove unused description column from tickets table
-- Ticket content is stored in article_content table (Yjs collaborative editor)
ALTER TABLE tickets DROP COLUMN description;
