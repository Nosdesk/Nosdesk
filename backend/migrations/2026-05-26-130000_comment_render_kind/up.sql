-- Item J / native-first rendering (Slice 1): per-comment render tier.
--
-- The inbound pipeline classifies each email comment into a render
-- tier at ingest so the frontend can render the common case natively
-- (a text bubble or a reduced semantic-HTML subset) and reserve the
-- sandboxed iframe for genuinely rich mail (newsletters, layout
-- tables, Word-soup).
--
--   text   - plaintext / format=flowed: escape + linkify + pre-wrap
--   simple - human HTML reduced to a semantic inline subset
--   rich   - keep the full sanitised HTML for the iframe
--
-- NULL = not classified: non-email comments (agent markdown) and email
-- comments ingested before this column existed. The frontend falls
-- back to its existing per-format rendering for NULL.

ALTER TABLE comments ADD COLUMN render_kind VARCHAR(16);
