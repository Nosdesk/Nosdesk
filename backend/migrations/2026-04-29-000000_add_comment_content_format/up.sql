-- Track the format of `comments.content` so the outbound dispatcher
-- knows whether to treat it as HTML, plaintext, or markdown when
-- composing a reply for a transport that needs a specific shape.
--
-- Default 'html' matches what the ProseMirror editor produces, which
-- is the format most existing rows are already in. The inbound IMAP
-- pipeline writes 'plaintext' explicitly for new email-sourced
-- comments.
ALTER TABLE comments
    ADD COLUMN content_format VARCHAR(16) NOT NULL DEFAULT 'html';
