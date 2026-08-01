-- A No-SLA policy: when it is the most-specific match for a ticket, the
-- ticket gets no SLA (compute_pill returns None). Lets an admin scope SLAs
-- away from a class of tickets (e.g. requests) by adding a more-specific
-- policy that beats the catch-all default, reusing the existing precedence.
ALTER TABLE sla_policies ADD COLUMN no_sla BOOLEAN NOT NULL DEFAULT false;
