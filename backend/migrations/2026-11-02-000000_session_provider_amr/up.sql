-- How the identity provider checked who signed in (its id_token's `amr`,
-- RFC 8176) on a session opened through the provider. An empty array is a
-- provider that didn't say; NULL is any other sign-in.
ALTER TABLE public.active_sessions ADD COLUMN provider_amr text[];
