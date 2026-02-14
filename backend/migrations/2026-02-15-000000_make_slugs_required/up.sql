-- Temporarily drop the existing unique constraint so we can fix duplicates/empties
ALTER TABLE documentation_pages DROP CONSTRAINT IF EXISTS documentation_pages_slug_key;

-- Backfill null slugs from title (lowercase, spaces to hyphens, strip non-alnum)
UPDATE documentation_pages
SET slug = TRIM(BOTH '-' FROM REGEXP_REPLACE(REGEXP_REPLACE(LOWER(title), '[^a-z0-9\s-]', '', 'g'), '[\s-]+', '-', 'g'))
WHERE slug IS NULL OR slug = '';

-- Handle any remaining nulls or empty slugs with id-based fallback
UPDATE documentation_pages SET slug = 'page-' || id WHERE slug IS NULL OR slug = '';

-- Handle purely numeric slugs (would be ambiguous with ID routing)
UPDATE documentation_pages SET slug = 'page-' || slug
WHERE slug ~ '^[0-9]+$';

-- Handle duplicates by appending -<id> to the later entries
UPDATE documentation_pages p1
SET slug = p1.slug || '-' || p1.id
WHERE EXISTS (
  SELECT 1 FROM documentation_pages p2
  WHERE p2.slug = p1.slug AND p2.id < p1.id
);

-- Add NOT NULL and re-add unique constraint
ALTER TABLE documentation_pages ALTER COLUMN slug SET NOT NULL;
ALTER TABLE documentation_pages ADD CONSTRAINT documentation_pages_slug_key UNIQUE (slug);
