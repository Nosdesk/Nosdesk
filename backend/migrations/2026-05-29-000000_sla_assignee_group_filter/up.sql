-- Add an optional group filter to SLA policies so an admin can scope a
-- policy to tickets whose assignee belongs to a specific group (e.g.
-- "Tier 2 engineering gets a 2h response target"). Mirrors the existing
-- priority_filter and category_id_filter columns: NULL means "any".
-- The matcher (services::sla::pick_policy) treats unassigned tickets as
-- non-matching for any policy that sets this filter, which is the right
-- semantic: a group-scoped policy can only apply once routing has
-- assigned an owner.
ALTER TABLE sla_policies
    ADD COLUMN assignee_group_id_filter INTEGER
        REFERENCES groups(id) ON DELETE SET NULL;

-- The existing (priority_filter, category_id_filter) index already
-- helps the policy-scan path; group_id is low-selectivity (most
-- policies will leave it NULL) so a dedicated index isn't worth the
-- write cost.
