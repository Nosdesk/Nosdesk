-- Per-ticket resolution notes. Captured separately from the
-- comment thread so the answer to "what fixed this?" lives in a
-- dedicated, structured field rather than buried somewhere in a
-- conversation that may include back-and-forth, internal notes,
-- and channel relays. Other helpdesks (Zendesk, Jira SM, Plain)
-- all separate the resolution from the discussion for the same
-- reason.
--
-- TEXT (no length cap) because admins paste runbook excerpts and
-- step-by-step diagnoses here. Empty string and NULL both mean
-- "no notes" — handlers should normalise empty → NULL on write
-- so the UI can use a single null-check rather than two.

ALTER TABLE tickets
    ADD COLUMN resolution_notes TEXT;
