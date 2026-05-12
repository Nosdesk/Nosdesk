-- Render-ready sanitised HTML for email-derived comments.
--
-- Pass 2 of the email rendering plan moves the HTML sanitisation
-- chokepoint from client-side DOMPurify-at-render to backend
-- ammonia-at-ingest. The browser still runs DOMPurify as
-- defence-in-depth before the iframe srcdoc, but the canonical
-- safe HTML lives here.
--
-- The pipeline runs the Outlook pre-strip → ammonia → (eventual)
-- lightningcss CSS pass on `body_html` and stores the result.
-- Re-sanitisation on policy change is a backfill job that
-- re-reads `raw_source_uri` (added in Pass 1) and re-runs the
-- pipeline — no upstream re-fetch needed.
--
-- NULL when the comment has no HTML body (plaintext-only inbound,
-- UI-authored comments, chat-relayed). Renderer treats NULL as a
-- signal to render from `new_content` / `quoted_content`
-- (plaintext path) or fall back to client-side DOMPurify on
-- `content` for pre-Pass-2 history.

ALTER TABLE comments
    ADD COLUMN sanitised_html TEXT;
