ALTER TABLE comments
    DROP COLUMN body_text,
    DROP COLUMN body_html,
    DROP COLUMN new_content,
    DROP COLUMN quoted_content,
    DROP COLUMN raw_source_uri;
