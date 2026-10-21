-- Hosted billing projection: the control plane pushes the workspace's
-- subscription state here so the owner can see their trial and cancel dates.
-- Cloud owns it; the workspace export leaves these out and an import never
-- sets them.
--
-- billing_status: Stripe's subscription status, stored verbatim; NULL when
-- the account has no subscription.
-- billing_trial_end, billing_cancel_at: from the subscription; NULL when unset.
-- billing_as_of: when the control plane read the subscription. A push applies
-- only when its as_of is newer, so a late or repeated one never overwrites
-- newer state.
--
-- Written by repository::workspaces::set_billing_state. The table-level
-- SELECT grant to nosdesk_app already covers new columns.
ALTER TABLE public.workspaces
    ADD COLUMN billing_status text,
    ADD COLUMN billing_trial_end timestamptz,
    ADD COLUMN billing_cancel_at timestamptz,
    ADD COLUMN billing_as_of timestamptz;
