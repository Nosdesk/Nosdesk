-- Item C / W5a (D4): add the AuditReviewer role.
--
-- A standalone role distinct from admin: read-only access to the
-- audit surface, no business-entity writes, no admin panel beyond the
-- audit view. The Rust UserRole enum gains a matching `AuditReviewer`
-- variant in the same change.
--
-- ALTER TYPE ... ADD VALUE is allowed inside Diesel's per-migration
-- transaction on PG 12+ as long as the new value isn't *used* in the
-- same transaction; this migration only adds it. IF NOT EXISTS keeps
-- it idempotent.

ALTER TYPE user_role ADD VALUE IF NOT EXISTS 'audit_reviewer';
