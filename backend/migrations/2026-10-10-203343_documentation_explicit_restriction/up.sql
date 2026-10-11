-- Who may open a restricted page or collection was "anyone, unless it has
-- grants". Anything that emptied the grants (deleting the only group or
-- person a record was shared with) opened it to everyone. Restriction is
-- now explicit: a restricted record is open only to its grants, and with
-- none left, only to admins.
--
-- documentation_collections.restricted: open only to its grants.
-- documentation_pages.restricted: the page has its own rules (open only to
-- its grants) instead of following its collection.
ALTER TABLE public.documentation_collections
    ADD COLUMN restricted boolean NOT NULL DEFAULT false;
ALTER TABLE public.documentation_pages
    ADD COLUMN restricted boolean NOT NULL DEFAULT false;

-- Backfill: a record with any grant today is restricted. Neither table is
-- audited; the page table's updated_at trigger is held off so the backfill
-- doesn't look like an edit.
UPDATE public.documentation_collections c
SET restricted = true
WHERE EXISTS (
    SELECT 1 FROM public.documentation_collection_visibility v
    WHERE v.collection_id = c.id
);

ALTER TABLE public.documentation_pages DISABLE TRIGGER set_updated_at;
UPDATE public.documentation_pages p
SET restricted = true
WHERE EXISTS (
    SELECT 1 FROM public.documentation_page_visibility v
    WHERE v.page_id = p.id
);
ALTER TABLE public.documentation_pages ENABLE TRIGGER set_updated_at;
