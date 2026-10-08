-- Titles and payloads are not restored: the page's name was the point of
-- removing them.
DROP INDEX IF EXISTS public.idx_knowledge_gaps_subject_page_id;
ALTER TABLE public.knowledge_gaps DROP CONSTRAINT IF EXISTS knowledge_gaps_subject_page_id_fkey;
ALTER TABLE public.knowledge_gaps DROP COLUMN IF EXISTS subject_page_id;
