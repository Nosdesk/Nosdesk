-- The Microsoft Teams personal tab (docs/plans/requester-phase3.md, Teams MVP).
-- It signs requesters in with the workspace's own Entra app, so it lives on the
-- requester sign-in provider row. `teams_app_id` is the Teams app's id in the
-- package we generate: stable, so a re-downloaded package updates the same app.
ALTER TABLE public.workspace_identity_providers
    ADD COLUMN teams_enabled boolean NOT NULL DEFAULT false,
    ADD COLUMN teams_app_id uuid NOT NULL DEFAULT gen_random_uuid();
