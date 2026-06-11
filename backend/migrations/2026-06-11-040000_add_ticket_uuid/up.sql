-- Give tickets a stable, never-recycled identity. Collaborative
-- documents are keyed by this UUID (ws-{workspaceUuid}_ticket-{uuid})
-- so a wiped+recreated ticket that reuses an integer id gets a fresh
-- doc identity and can't inherit a previous generation's cached notes.
-- Docs/collections already carry a uuid; this brings tickets in line.
-- A non-constant default forces a per-row rewrite, so existing rows
-- each get a distinct uuid.
ALTER TABLE tickets ADD COLUMN uuid uuid NOT NULL DEFAULT uuid_generate_v7();
ALTER TABLE tickets ADD CONSTRAINT tickets_uuid_key UNIQUE (uuid);
