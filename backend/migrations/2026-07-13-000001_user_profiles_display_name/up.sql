-- Per-workspace display-name override (identity orchestration O7). A user has
-- ONE global display name (control-plane-owned `users.name`, projected in), but
-- may present a different name per workspace — e.g. a different persona/title in
-- each workspace they belong to. `user_profiles` is already workspace-scoped
-- (PK (workspace_id, user_uuid), RLS on app.workspace_id), so the override
-- naturally lives here alongside job_title/organization/department.
--
-- Additive + nullable: NULL means "no override" → render the global name. No
-- backfill. The manual profile-edit surface owns this column (not directory
-- sync), same as the other standard columns.
ALTER TABLE public.user_profiles
    ADD COLUMN display_name varchar(255);
