-- Asset records' sync aggregate (docs: asset records, R2). Alone in its
-- migration: the value is only added here and first used by a runtime emit.
ALTER TYPE public.sync_aggregate ADD VALUE IF NOT EXISTS 'asset_record';
