-- A stale-doc gap names its page by id. It used to carry the page's title in
-- its own title ("Doc may be stale: {title}") and the page's title and slug in
-- its signal's payload, so the queue named a page to anyone who can read gaps,
-- whether or not they can open the page. The client now names the page from
-- the documentation it can already see.
--
-- Every statement is safe to run again. knowledge_gaps and
-- knowledge_gap_signals carry no audit trigger.

ALTER TABLE public.knowledge_gaps ADD COLUMN IF NOT EXISTS subject_page_id INTEGER;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'knowledge_gaps_subject_page_id_fkey'
    ) THEN
        ALTER TABLE public.knowledge_gaps
            ADD CONSTRAINT knowledge_gaps_subject_page_id_fkey
            FOREIGN KEY (workspace_id, subject_page_id)
            REFERENCES public.documentation_pages (workspace_id, id)
            ON DELETE SET NULL (subject_page_id);
    END IF;
END
$$;

CREATE INDEX IF NOT EXISTS idx_knowledge_gaps_subject_page_id
    ON public.knowledge_gaps (subject_page_id) WHERE subject_page_id IS NOT NULL;

-- Existing stale-doc gaps: the page from their signal, when it still exists.
UPDATE public.knowledge_gaps g
SET subject_page_id = p.id
FROM public.knowledge_gap_signals s
JOIN public.documentation_pages p
  ON p.workspace_id = s.workspace_id AND p.id::text = s.source_ref
WHERE s.gap_id = g.id
  AND s.workspace_id = g.workspace_id
  AND s.signal_type = 'stale_doc'
  AND s.source_kind = 'page'
  AND g.subject_page_id IS NULL;

UPDATE public.knowledge_gaps
SET title = 'Doc may be stale'
WHERE title LIKE 'Doc may be stale: %';

UPDATE public.knowledge_gap_signals
SET payload = payload - 'page_title' - 'page_slug'
WHERE signal_type = 'stale_doc'
  AND (payload ? 'page_title' OR payload ? 'page_slug');
