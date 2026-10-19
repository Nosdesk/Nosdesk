-- Client cache fence. Replacing a workspace's data (an archive import, a
-- reset) gives every row new ids, so a client holding that workspace's
-- cached rows must drop them and bootstrap again.
--
-- data_epoch: a new uuid each time the data is replaced. Clients compare it
-- with the one they cached under (`/api/sync/schema`, every delta) and wipe
-- their caches when it changes. NULL until the first replacement.
--
-- resync_floor: a sync_id drawn from sync_actions_sync_id_seq when the data
-- was replaced, so it is above every cursor issued before. A delta from a
-- cursor below it returns resync_required, and a bootstrap never hands out a
-- cursor below it. NULL until the first replacement.
--
-- Both are set by repository::workspaces::bump_data_epoch. The table-level
-- SELECT grant to nosdesk_app already covers new columns.
ALTER TABLE public.workspaces
    ADD COLUMN data_epoch uuid,
    ADD COLUMN resync_floor bigint;
