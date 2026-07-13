-- Global username handle, projected from the control-plane IdP (identity
-- orchestration O4). Additive + nullable: existing rows read as NULL (no
-- handle) with no backfill, so there's no audit-trigger backfill hazard.
--
-- The control plane is authoritative and validates the handle before it ever
-- reaches here (3-30 chars, [a-z0-9_-], lowercase-canonical, reserved-word
-- denylist), so the product simply stores what it projects — no product-side
-- format check, and a plain unique index suffices (handles are lowercase, so
-- there are no case collisions to fold). The partial index keeps the many
-- NULL rows unconstrained while preventing two members sharing a handle.
ALTER TABLE users
    ADD COLUMN username varchar(30);

CREATE UNIQUE INDEX users_username_key
    ON users (username)
    WHERE username IS NOT NULL;
