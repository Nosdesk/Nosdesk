-- Email-derived comments gain a richer body shape than the single
-- `content` column gives us. Today the inbound pipeline picks one
-- (HTML if present, else plaintext) and stuffs it into `content`,
-- which means:
--   - we can't tell what was originally text vs HTML,
--   - we can't re-render with different heuristics later,
--   - quote-extraction happens client-side per render against
--     `content`, and any quoted-prior-thread that travelled inside
--     the reply ships to the browser on every page load.
--
-- Five new nullable columns. All optional because non-email
-- comments (UI-authored, Slack relay, Discord relay) don't fill
-- them. `content` + `content_format` stay during the transition
-- so existing read paths keep working; deprecation comes after
-- the frontend migrates.
--
--   body_text       Raw text/plain MIME part (or the body for
--                   plaintext-only messages).
--   body_html       Raw text/html MIME part. Pre-sanitisation;
--                   Pass 2 will add a separate `sanitised_html`
--                   column for the render-ready form.
--   new_content     Extracted "just this reply" — output of the
--                   quote splitter run at ingest. Plain text or
--                   HTML depending on which path the parser took;
--                   the renderer picks by `content_format`.
--   quoted_content  Extracted prior-thread quoted block. NULL
--                   when nothing detected. Same format as
--                   `new_content`.
--   raw_source_uri  Storage path to the persisted .eml. Powers
--                   "Show original message" and lets us re-run
--                   the splitter on policy change without a
--                   re-fetch. References utils::storage paths,
--                   not URLs, so the backend resolves them
--                   through whichever Storage backend is active.

ALTER TABLE comments
    ADD COLUMN body_text      TEXT,
    ADD COLUMN body_html      TEXT,
    ADD COLUMN new_content    TEXT,
    ADD COLUMN quoted_content TEXT,
    ADD COLUMN raw_source_uri TEXT;
