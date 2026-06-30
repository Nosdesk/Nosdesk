-- Squashed migration: collapses the 18 post-v1.0.11 migrations into one.
-- Net effect is identical to applying them in sequence; verified by a
-- zero schema-diff against main. Originals retained in git history.
-- A v1.0.11 database applies this single migration to reach v1.1.0.

-- ===================================================================
-- from 2026-06-16-120000_workspace_email_settings
-- ===================================================================
-- Per-workspace outbound email identity.
--
-- Outbound SMTP was instance-global (one EmailService::from_env shared by
-- every workspace), while inbound IMAP is already per-channel. Hosted
-- tenants could not send from their own address or relay. This table holds
-- a per-workspace sending identity (From + SMTP transport); the global env
-- config stays the fallback, so single-tenant self-host is unchanged.
--
-- New, empty table: no backfill, so the audit trigger only ever fires on
-- live workspace-pinned writes and there is no NDX01 actor-context trap.
-- The SMTP password is stored KEK-encrypted (the same framed AES-256-GCM
-- blob + kek_id sidecar as channel_credentials.encrypted_value) and is
-- redacted from the audit log via the trigger's exclude list.

CREATE TABLE public.workspace_email_settings (
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    enabled boolean DEFAULT false NOT NULL,
    from_name character varying(255) DEFAULT ''::character varying NOT NULL,
    from_email character varying(320) DEFAULT ''::character varying NOT NULL,
    smtp_host character varying(255) DEFAULT ''::character varying NOT NULL,
    smtp_port integer DEFAULT 587 NOT NULL,
    smtp_security character varying(16) DEFAULT 'starttls'::character varying NOT NULL,
    smtp_username character varying(255) DEFAULT ''::character varying NOT NULL,
    encrypted_smtp_password bytea,
    encrypted_kek_id smallint,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT workspace_email_settings_pkey PRIMARY KEY (workspace_id),
    CONSTRAINT workspace_email_settings_smtp_security_check
        CHECK (smtp_security::text = ANY (ARRAY['tls'::text, 'starttls'::text, 'plaintext'::text])),
    CONSTRAINT workspace_email_settings_smtp_port_check
        CHECK (smtp_port > 0 AND smtp_port <= 65535)
);

ALTER TABLE ONLY public.workspace_email_settings FORCE ROW LEVEL SECURITY;
ALTER TABLE public.workspace_email_settings OWNER TO nosdesk_admin;

ALTER TABLE ONLY public.workspace_email_settings
    ADD CONSTRAINT workspace_email_settings_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);

ALTER TABLE public.workspace_email_settings ENABLE ROW LEVEL SECURITY;

CREATE POLICY workspace_email_settings_workspace_isolation ON public.workspace_email_settings
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.workspace_email_settings TO nosdesk_app;

-- pk = workspace_id; encrypted_smtp_password is redacted (logged only as
-- encrypted_smtp_password_changed: bool).
CREATE TRIGGER tr_audit_workspace_email_settings
    AFTER INSERT OR DELETE OR UPDATE ON public.workspace_email_settings
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('workspace_id', 'encrypted_smtp_password');

-- ===================================================================
-- from 2026-06-16-130000_outbound_emails_sender_identity
-- ===================================================================
-- Outbound mail identity routing.
--
-- The queue worker resolves a per-workspace SMTP identity at send time, but
-- auth/platform mail (password reset, invitation) must keep sending from the
-- instance identity, not a tenant's relay (a phishing + deliverability
-- guard). Notification and conversation mail use the workspace identity.
-- These classes are otherwise indistinguishable in the queue (notifications
-- carry no ticket/channel/comment id either), so record the policy
-- explicitly at enqueue and let the worker route on it.
--
-- ADD COLUMN ... DEFAULT is metadata-only in PG11+ (no row rewrite, no
-- per-row audit-trigger fire). The 'workspace' default is safe at deploy:
-- no workspace identity exists until an admin opts in, so every row falls
-- back to the platform identity in the meantime regardless of this value.

ALTER TABLE public.outbound_emails
    ADD COLUMN sender_identity character varying(16) NOT NULL DEFAULT 'workspace';

ALTER TABLE public.outbound_emails
    ADD CONSTRAINT outbound_emails_sender_identity_check
    CHECK (sender_identity::text = ANY (ARRAY['workspace'::text, 'platform'::text]));

-- ===================================================================
-- from 2026-06-17-000000_workspace_email_settings_dkim
-- ===================================================================
-- Verified-domain (self-managed DKIM) sending mode for workspace_email_settings.
--
-- The hosted model: a workspace sends from its verified domain through the
-- instance relay (SES), DKIM-signed d=domain, so DMARC passes on DKIM
-- alignment alone. These columns hold the per-domain signing material and the
-- verification state. `sending_mode` discriminates how a workspace sends:
--   * fallback        - the instance/env identity (no per-workspace sending)
--   * verified_domain - DKIM-signed via the instance relay (hosted model)
--   * smtp_relay      - the workspace's own relay (the existing smtp_* columns)
--
-- Column-add on an empty table (the feature isn't live yet), so no backfill and
-- no audit-trigger trap. The DKIM private key is KEK-encrypted like the SMTP
-- password, with its own kek_id sidecar, and is redacted from the audit log
-- (the trigger is recreated below to add it to the exclude list).

ALTER TABLE public.workspace_email_settings
    ADD COLUMN sending_mode character varying(16) NOT NULL DEFAULT 'fallback',
    ADD COLUMN sending_domain character varying(255),
    ADD COLUMN dkim_selector character varying(63),
    ADD COLUMN dkim_algorithm character varying(16),
    ADD COLUMN encrypted_dkim_private_key bytea,
    ADD COLUMN dkim_kek_id smallint,
    ADD COLUMN verification_status character varying(16) NOT NULL DEFAULT 'unverified',
    ADD COLUMN verified_at timestamp with time zone;

ALTER TABLE public.workspace_email_settings
    ADD CONSTRAINT workspace_email_settings_sending_mode_check
    CHECK (sending_mode::text = ANY (ARRAY['fallback'::text, 'verified_domain'::text, 'smtp_relay'::text]));

ALTER TABLE public.workspace_email_settings
    ADD CONSTRAINT workspace_email_settings_verification_status_check
    CHECK (verification_status::text = ANY (ARRAY['unverified'::text, 'pending'::text, 'verified'::text, 'failed'::text]));

ALTER TABLE public.workspace_email_settings
    ADD CONSTRAINT workspace_email_settings_dkim_algorithm_check
    CHECK (dkim_algorithm IS NULL OR dkim_algorithm::text = ANY (ARRAY['rsa'::text, 'ed25519'::text]));

-- Recreate the audit trigger so the DKIM private key joins the SMTP password
-- in the redaction list (logged only as <col>_changed: bool).
DROP TRIGGER tr_audit_workspace_email_settings ON public.workspace_email_settings;
CREATE TRIGGER tr_audit_workspace_email_settings
    AFTER INSERT OR UPDATE OR DELETE ON public.workspace_email_settings
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('workspace_id', 'encrypted_smtp_password', 'encrypted_dkim_private_key');

-- ===================================================================
-- from 2026-06-17-010000_drop_outbound_emails_delivered_at
-- ===================================================================
-- `outbound_emails.delivered_at` is dead weight: SMTP gives no delivery
-- confirmation, and the only integration that ever wrote it (the provider
-- delivery webhook) was removed, so the column has only ever been NULL. Drop it
-- rather than ship a column that implies delivery tracking we don't have;
-- `status = 'sent'` (relay accepted handoff) remains the strongest send signal.
--
-- Pure DDL: no row writes, so the outbound_emails audit trigger doesn't fire and
-- needs no disabling.
ALTER TABLE outbound_emails DROP COLUMN delivered_at;

-- ===================================================================
-- from 2026-06-18-120000_outbound_emails_mail_class
-- ===================================================================
-- Outbound mail class: notification vs transactional.
--
-- Deliverability features branch on this. List-Unsubscribe + One-Click (B2)
-- goes only on notification mail (ticket-update notifications, the opt-out-able
-- class), never on transactional mail (password reset, invitation, the agent's
-- reply, auto-ack). It is a distinct axis from sender_identity: a conversation
-- reply is workspace-identity but transactional; a notification is
-- workspace-identity but notification. The two classes are otherwise
-- indistinguishable in the queue (notifications carry no ticket/channel/comment
-- id either), so record the policy explicitly at enqueue and let the worker and
-- the B-items branch on it rather than re-deriving it.
--
-- ADD COLUMN ... DEFAULT is metadata-only in PG11+ (no row rewrite, no per-row
-- audit-trigger fire). 'transactional' is the safe default: it omits
-- List-Unsubscribe, so an unclassified row is never treated as opt-out-able.

ALTER TABLE public.outbound_emails
    ADD COLUMN mail_class character varying(16) NOT NULL DEFAULT 'transactional';

ALTER TABLE public.outbound_emails
    ADD CONSTRAINT outbound_emails_mail_class_check
    CHECK (mail_class::text = ANY (ARRAY['transactional'::text, 'notification'::text]));

-- ===================================================================
-- from 2026-06-20-000000_inbound_forwarding
-- ===================================================================
-- Inbound forwarding addresses: the opaque-token router for forwarding-based
-- email ingestion (the hosted inbound path).
--
-- A customer forwards their support mailbox to <token>@inbound.<domain>; SES
-- receives it and the webhook resolves <token> to the owning workspace +
-- channel, then runs the existing channels parse pipeline. The token is an
-- unguessable capability rather than the workspace slug: a guessable address
-- would let anyone inject mail into a known workspace's queue. One row per
-- forwarding address; a channel can own more than one (per-inbox split) later
-- without a schema change. `status` carries 'active'/'retired' so a rotated
-- address is invalidated while staying on record.
--
-- Resolving a token is a pre-tenant, cross-workspace lookup (the webhook has
-- no workspace context until the token resolves), so the webhook reads this
-- table on a system/background connection; the token's unguessability is the
-- access control and RLS is the defence-in-depth backstop for app-path reads.
--
-- New, empty table: no backfill, so no audit-trigger backfill trap.

CREATE TABLE public.inbound_addresses (
    id integer NOT NULL,
    token character varying(64) NOT NULL,
    channel_id integer NOT NULL,
    status character varying(16) DEFAULT 'active'::character varying NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT inbound_addresses_status_check
        CHECK (status::text = ANY (ARRAY['active'::text, 'retired'::text]))
);

CREATE SEQUENCE public.inbound_addresses_id_seq
    AS integer START WITH 1 INCREMENT BY 1 NO MINVALUE NO MAXVALUE CACHE 1;
ALTER SEQUENCE public.inbound_addresses_id_seq OWNED BY public.inbound_addresses.id;
ALTER TABLE ONLY public.inbound_addresses
    ALTER COLUMN id SET DEFAULT nextval('public.inbound_addresses_id_seq'::regclass);

ALTER TABLE ONLY public.inbound_addresses
    ADD CONSTRAINT inbound_addresses_pkey PRIMARY KEY (id);
ALTER TABLE ONLY public.inbound_addresses
    ADD CONSTRAINT inbound_addresses_token_key UNIQUE (token);

ALTER TABLE ONLY public.inbound_addresses
    ADD CONSTRAINT inbound_addresses_channel_id_fkey
    FOREIGN KEY (channel_id) REFERENCES public.channels(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.inbound_addresses
    ADD CONSTRAINT inbound_addresses_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);

CREATE INDEX idx_inbound_addresses_channel ON public.inbound_addresses USING btree (channel_id);

ALTER TABLE ONLY public.inbound_addresses FORCE ROW LEVEL SECURITY;
ALTER TABLE public.inbound_addresses OWNER TO nosdesk_admin;
ALTER SEQUENCE public.inbound_addresses_id_seq OWNER TO nosdesk_admin;
ALTER TABLE public.inbound_addresses ENABLE ROW LEVEL SECURITY;

CREATE POLICY inbound_addresses_workspace_isolation ON public.inbound_addresses
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.inbound_addresses TO nosdesk_app;
GRANT ALL ON SEQUENCE public.inbound_addresses_id_seq TO nosdesk_app;

-- ===================================================================
-- from 2026-06-20-010000_inbound_dead_letters
-- ===================================================================
-- Unrouted inbound mail: the dead-letter log for the hosted inbound path.
--
-- Mail forwarded to an unknown <token>@inbound.<domain> can't be attributed to
-- any workspace (a mistyped forward target, a forward set up before the
-- channel was saved, or a rotated-out token), so this table is platform-level,
-- NOT workspace-scoped: there is deliberately no workspace_id and no RLS. It is
-- a diagnostic, not a quarantine, so a misconfigured forward is visible to the
-- operator instead of vanishing silently. Spam/virus-failing unknown mail is
-- dropped without a row; only scans-passing unknown mail lands here. The S3
-- lifecycle expires the referenced object on its own, so each row points at a
-- body that self-deletes.

CREATE TABLE public.inbound_dead_letters (
    id bigint NOT NULL,
    envelope_recipient character varying(320) NOT NULL,
    from_address character varying(320),
    subject text,
    s3_key text NOT NULL,
    reason character varying(32) NOT NULL,
    received_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE SEQUENCE public.inbound_dead_letters_id_seq
    AS bigint START WITH 1 INCREMENT BY 1 NO MINVALUE NO MAXVALUE CACHE 1;
ALTER SEQUENCE public.inbound_dead_letters_id_seq OWNED BY public.inbound_dead_letters.id;
ALTER TABLE ONLY public.inbound_dead_letters
    ALTER COLUMN id SET DEFAULT nextval('public.inbound_dead_letters_id_seq'::regclass);

ALTER TABLE ONLY public.inbound_dead_letters
    ADD CONSTRAINT inbound_dead_letters_pkey PRIMARY KEY (id);

CREATE INDEX idx_inbound_dead_letters_received_at
    ON public.inbound_dead_letters USING btree (received_at DESC);

ALTER TABLE public.inbound_dead_letters OWNER TO nosdesk_admin;
ALTER SEQUENCE public.inbound_dead_letters_id_seq OWNER TO nosdesk_admin;

GRANT SELECT, INSERT, DELETE ON TABLE public.inbound_dead_letters TO nosdesk_app;
GRANT ALL ON SEQUENCE public.inbound_dead_letters_id_seq TO nosdesk_app;

-- ===================================================================
-- from 2026-06-20-020000_ticket_merges_satellite
-- ===================================================================
-- Normalize ticket-merge metadata out of the wide `tickets` table.
--
-- A merge is a sparse, one-time event: `merged_into_ticket_id` / `merged_at` /
-- `merged_by_user_uuid` / `merge_reason` are NULL on ~99% of tickets and
-- describe a relationship ("this ticket was merged into that one"), not a core
-- attribute of a live ticket. Four cold columns on the hot tickets row is the
-- wrong shape, so they move to a 1:1 satellite keyed by the source ticket.
--
-- Not audited separately: the merge action already emits a `ticket.merged`
-- sync event and the source ticket's workflow-state change is audited on
-- `tickets`, so the satellite carries no new audit surface (and the backfill
-- below stays trigger-free).

CREATE TABLE public.ticket_merges (
    ticket_id integer NOT NULL,
    merged_into_ticket_id integer NOT NULL,
    merged_at timestamp with time zone NOT NULL,
    merged_by_user_uuid uuid,
    merge_reason text,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT ticket_merges_pkey PRIMARY KEY (ticket_id)
);

-- Backfill BEFORE enabling RLS so the cross-workspace copy isn't filtered by
-- the per-row policy. (The migrator bypasses RLS anyway, but this is robust to
-- a non-superuser migrator.)
INSERT INTO public.ticket_merges
    (ticket_id, merged_into_ticket_id, merged_at, merged_by_user_uuid, merge_reason, workspace_id)
SELECT id, merged_into_ticket_id, merged_at, merged_by_user_uuid, merge_reason, workspace_id
FROM public.tickets
WHERE merged_into_ticket_id IS NOT NULL;

ALTER TABLE ONLY public.ticket_merges
    ADD CONSTRAINT ticket_merges_ticket_id_fkey
    FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.ticket_merges
    ADD CONSTRAINT ticket_merges_merged_into_ticket_id_fkey
    FOREIGN KEY (merged_into_ticket_id) REFERENCES public.tickets(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.ticket_merges
    ADD CONSTRAINT ticket_merges_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);

CREATE INDEX idx_ticket_merges_into ON public.ticket_merges USING btree (merged_into_ticket_id);

ALTER TABLE ONLY public.ticket_merges FORCE ROW LEVEL SECURITY;
ALTER TABLE public.ticket_merges OWNER TO nosdesk_admin;
ALTER TABLE public.ticket_merges ENABLE ROW LEVEL SECURITY;

CREATE POLICY ticket_merges_workspace_isolation ON public.ticket_merges
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.ticket_merges TO nosdesk_app;

-- Drop the columns + their all-or-nothing check from the hot table.
ALTER TABLE public.tickets DROP CONSTRAINT tickets_merge_complete;
ALTER TABLE public.tickets
    DROP COLUMN merged_into_ticket_id,
    DROP COLUMN merged_at,
    DROP COLUMN merged_by_user_uuid,
    DROP COLUMN merge_reason;

-- ===================================================================
-- from 2026-06-20-030000_tickets_spam_suspected
-- ===================================================================
-- Ticket-level spam-suspected flag (hot: read on every queue row for the badge).
--
-- Set true when a ticket opens from inbound mail the provider flagged as spam
-- (SES X-SES-Spam-Verdict on the forwarding path). We never drop the mail; the
-- ticket opens flagged + low-priority so agents can triage it from the queue.
-- Clearing the flag ("not spam") is a normal ticket update.
--
-- Room for this came from normalizing the cold merge columns out into
-- ticket_merges in the previous migration. ADD COLUMN ... DEFAULT is
-- metadata-only in PG11+ (no row rewrite, no per-row audit-trigger fire); the
-- audit trigger captures the row generically, so no trigger recreation needed.

ALTER TABLE public.tickets
    ADD COLUMN spam_suspected boolean NOT NULL DEFAULT false;

-- ===================================================================
-- from 2026-06-21-000000_asset_loans
-- ===================================================================
-- Device loaning: a first-class loan ledger.
--
-- A loan is a span: an asset is in a borrower's custody from `loaned_at` until
-- `returned_at`, optionally with a `due_back` date, optionally against the
-- ticket that prompted it. The ledger is the source of truth for "who has what,
-- until when, returned or not"; `assets.status = 'on_loan'` is a denormalised
-- cache kept in step by the issue/return flow, and `asset_lifecycle_events`
-- stays the unified status timeline (the issue/return transitions reference the
-- loan via metadata.loan_id). At most one active (unreturned) loan per asset.
--
-- New, empty table: the audit trigger only ever fires on real loan writes, so
-- there is no backfill to disable it for.

-- Register the loan aggregate on the sync_actions enum so loan events can be
-- recorded on the pool. Safe inside the migration transaction (PG12+): the
-- value is only added here, never used until a later runtime emit. (The down
-- migration can't remove it; Postgres doesn't drop enum values, which is the
-- standard, harmless limitation.)
ALTER TYPE public.sync_aggregate ADD VALUE IF NOT EXISTS 'asset_loan';

CREATE TABLE public.asset_loans (
    id integer NOT NULL,
    asset_id integer NOT NULL,
    borrower_user_uuid uuid NOT NULL,
    loaned_at timestamp with time zone DEFAULT now() NOT NULL,
    due_back date,
    returned_at timestamp with time zone,
    ticket_id integer,
    status_before character varying(32) NOT NULL,
    notes text,
    actor_uuid uuid,
    returned_by_uuid uuid,
    due_soon_notified_at timestamp with time zone,
    overdue_notified_at timestamp with time zone,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT asset_loans_pkey PRIMARY KEY (id)
);

CREATE SEQUENCE public.asset_loans_id_seq
    AS integer START WITH 1 INCREMENT BY 1 NO MINVALUE NO MAXVALUE CACHE 1;
ALTER SEQUENCE public.asset_loans_id_seq OWNED BY public.asset_loans.id;
ALTER TABLE ONLY public.asset_loans
    ALTER COLUMN id SET DEFAULT nextval('public.asset_loans_id_seq'::regclass);

ALTER TABLE ONLY public.asset_loans
    ADD CONSTRAINT asset_loans_asset_id_fkey
    FOREIGN KEY (asset_id) REFERENCES public.assets(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.asset_loans
    ADD CONSTRAINT asset_loans_borrower_user_uuid_fkey
    FOREIGN KEY (borrower_user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;
ALTER TABLE ONLY public.asset_loans
    ADD CONSTRAINT asset_loans_ticket_id_fkey
    FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE SET NULL;
ALTER TABLE ONLY public.asset_loans
    ADD CONSTRAINT asset_loans_actor_uuid_fkey
    FOREIGN KEY (actor_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;
ALTER TABLE ONLY public.asset_loans
    ADD CONSTRAINT asset_loans_returned_by_uuid_fkey
    FOREIGN KEY (returned_by_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;
ALTER TABLE ONLY public.asset_loans
    ADD CONSTRAINT asset_loans_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);

-- At most one active loan per asset (a device can't be in two hands at once).
-- workspace_id leads the key to satisfy the tenant-table unique-index lint; it
-- is redundant since asset_id is globally unique, but keeps the invariant clean
-- without an allowlist entry.
CREATE UNIQUE INDEX uq_asset_loans_active_per_asset
    ON public.asset_loans USING btree (workspace_id, asset_id) WHERE (returned_at IS NULL);

CREATE INDEX idx_asset_loans_asset ON public.asset_loans USING btree (asset_id);
CREATE INDEX idx_asset_loans_borrower ON public.asset_loans USING btree (borrower_user_uuid);
CREATE INDEX idx_asset_loans_ticket ON public.asset_loans USING btree (ticket_id);
-- Reminder scan: open loans that carry a due date.
CREATE INDEX idx_asset_loans_due_open ON public.asset_loans USING btree (due_back)
    WHERE (returned_at IS NULL AND due_back IS NOT NULL);

ALTER TABLE ONLY public.asset_loans FORCE ROW LEVEL SECURITY;
ALTER TABLE public.asset_loans OWNER TO nosdesk_admin;
ALTER SEQUENCE public.asset_loans_id_seq OWNER TO nosdesk_admin;
ALTER TABLE public.asset_loans ENABLE ROW LEVEL SECURITY;

CREATE POLICY asset_loans_workspace_isolation ON public.asset_loans
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.asset_loans TO nosdesk_app;
GRANT ALL ON SEQUENCE public.asset_loans_id_seq TO nosdesk_app;

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.asset_loans
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
CREATE TRIGGER tr_audit_asset_loans AFTER INSERT OR DELETE OR UPDATE ON public.asset_loans
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');

-- ===================================================================
-- from 2026-06-21-010000_loan_notification_types
-- ===================================================================
-- Notification types for the device-loan due-back reminders (Phase 3).
--
-- `notification_types` is a non-tenant system catalog (no workspace_id, no
-- RLS), so this is a plain seed. The scheduler's loan-reminder job dispatches
-- these to the borrower via NotificationService (in-app + email), per the
-- default_channels here and each recipient's preferences.
INSERT INTO public.notification_types (id, code, name, description, category, default_channels) VALUES
  (9, 'loan_due_soon', 'Loan Due Soon', 'When a device you have on loan is due back soon', 'asset', '["in_app", "email"]'),
  (10, 'loan_overdue', 'Loan Overdue', 'When a device you have on loan is overdue', 'asset', '["in_app", "email"]');

SELECT pg_catalog.setval('public.notification_types_id_seq', 10, true);

-- ===================================================================
-- from 2026-06-21-120000_asset_model_catalog
-- ===================================================================
-- Asset model catalog (NetBox-style Manufacturer -> Model -> Asset).
--
-- A `manufacturer` is a make (Apple, Dell). An `asset_model` is a real
-- make+model ("MacBook Pro 16 2023") that captures the kind and a set of
-- default attributes once; an asset references one via `assets.model_id`
-- and is stamped with the manufacturer/model/kind/specs at assignment
-- time (copy-at-assignment, like NetBox: edits to a model do not rewrite
-- existing assets). Model-less assets keep working with hand-typed
-- manufacturer/model text and a directly-set kind.
--
-- All new, empty tables; the audit trigger only fires on real writes, so
-- there is no backfill to disable it for. `assets.model_id` is added
-- nullable with no backfill (DDL doesn't fire the row trigger).

-- The catalog is workspace config (like asset_kinds), not a sync
-- aggregate: pickers re-fetch via the frontend cache, and coverage lives
-- in the audit_log triggers below. No sync_aggregate enum value needed.

-- ---------------------------------------------------------------------
-- manufacturers
-- ---------------------------------------------------------------------
CREATE TABLE public.manufacturers (
    id integer NOT NULL,
    name character varying(255) NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    CONSTRAINT manufacturers_pkey PRIMARY KEY (id)
);

CREATE SEQUENCE public.manufacturers_id_seq
    AS integer START WITH 1 INCREMENT BY 1 NO MINVALUE NO MAXVALUE CACHE 1;
ALTER SEQUENCE public.manufacturers_id_seq OWNED BY public.manufacturers.id;
ALTER TABLE ONLY public.manufacturers
    ALTER COLUMN id SET DEFAULT nextval('public.manufacturers_id_seq'::regclass);

ALTER TABLE ONLY public.manufacturers
    ADD CONSTRAINT manufacturers_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);
ALTER TABLE ONLY public.manufacturers
    ADD CONSTRAINT manufacturers_created_by_fkey
    FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;

-- One name per workspace. workspace_id leads to satisfy the tenant-table
-- unique-index lint.
CREATE UNIQUE INDEX uq_manufacturers_workspace_name
    ON public.manufacturers USING btree (workspace_id, name);

ALTER TABLE ONLY public.manufacturers FORCE ROW LEVEL SECURITY;
ALTER TABLE public.manufacturers OWNER TO nosdesk_admin;
ALTER SEQUENCE public.manufacturers_id_seq OWNER TO nosdesk_admin;
ALTER TABLE public.manufacturers ENABLE ROW LEVEL SECURITY;

CREATE POLICY manufacturers_workspace_isolation ON public.manufacturers
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.manufacturers TO nosdesk_app;
GRANT ALL ON SEQUENCE public.manufacturers_id_seq TO nosdesk_app;

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.manufacturers
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
CREATE TRIGGER tr_audit_manufacturers AFTER INSERT OR DELETE OR UPDATE ON public.manufacturers
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');

-- ---------------------------------------------------------------------
-- asset_models (the "device type")
-- ---------------------------------------------------------------------
CREATE TABLE public.asset_models (
    id integer NOT NULL,
    manufacturer_id integer NOT NULL,
    name character varying(255) NOT NULL,
    -- The model's category/role. References asset_kinds.slug, validated in
    -- the app layer like assets.kind (no DB FK, slug is per-workspace).
    kind character varying(64) NOT NULL,
    part_number character varying(255),
    -- Specs stamped onto new assets of this model. Validated against the
    -- kind's user-owned attribute schema.
    default_attributes jsonb DEFAULT '{}'::jsonb NOT NULL,
    notes text,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    CONSTRAINT asset_models_pkey PRIMARY KEY (id)
);

CREATE SEQUENCE public.asset_models_id_seq
    AS integer START WITH 1 INCREMENT BY 1 NO MINVALUE NO MAXVALUE CACHE 1;
ALTER SEQUENCE public.asset_models_id_seq OWNED BY public.asset_models.id;
ALTER TABLE ONLY public.asset_models
    ALTER COLUMN id SET DEFAULT nextval('public.asset_models_id_seq'::regclass);

-- RESTRICT: a manufacturer with models must have them cleared first, so a
-- delete can't silently orphan a catalog.
ALTER TABLE ONLY public.asset_models
    ADD CONSTRAINT asset_models_manufacturer_id_fkey
    FOREIGN KEY (manufacturer_id) REFERENCES public.manufacturers(id) ON DELETE RESTRICT;
ALTER TABLE ONLY public.asset_models
    ADD CONSTRAINT asset_models_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);
ALTER TABLE ONLY public.asset_models
    ADD CONSTRAINT asset_models_created_by_fkey
    FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;

CREATE UNIQUE INDEX uq_asset_models_workspace_mfr_name
    ON public.asset_models USING btree (workspace_id, manufacturer_id, name);
CREATE INDEX idx_asset_models_manufacturer ON public.asset_models USING btree (manufacturer_id);
CREATE INDEX idx_asset_models_kind ON public.asset_models USING btree (workspace_id, kind);

ALTER TABLE ONLY public.asset_models FORCE ROW LEVEL SECURITY;
ALTER TABLE public.asset_models OWNER TO nosdesk_admin;
ALTER SEQUENCE public.asset_models_id_seq OWNER TO nosdesk_admin;
ALTER TABLE public.asset_models ENABLE ROW LEVEL SECURITY;

CREATE POLICY asset_models_workspace_isolation ON public.asset_models
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.asset_models TO nosdesk_app;
GRANT ALL ON SEQUENCE public.asset_models_id_seq TO nosdesk_app;

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.asset_models
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
CREATE TRIGGER tr_audit_asset_models AFTER INSERT OR DELETE OR UPDATE ON public.asset_models
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');

-- ---------------------------------------------------------------------
-- assets.model_id
-- ---------------------------------------------------------------------
ALTER TABLE public.assets ADD COLUMN model_id integer;
ALTER TABLE ONLY public.assets
    ADD CONSTRAINT assets_model_id_fkey
    FOREIGN KEY (model_id) REFERENCES public.asset_models(id) ON DELETE SET NULL;
CREATE INDEX idx_assets_model ON public.assets USING btree (model_id);

-- ===================================================================
-- from 2026-06-23-000000_native_asset_groups
-- ===================================================================
-- Native asset groups: workspace-local, user-managed classification of assets
-- (e.g. "Loaner pool", "Exec laptops", "Warehouse scanners"). Tag-style UX
-- (multi-assign, assigned from the asset, surfaced as a list filter) over an
-- entity-shaped schema, so future depth (smart membership, a detail page)
-- stays additive rather than a concept migration.
--
-- Distinct from the directory-group memberships Intune/Entra sync owns: that
-- junction is renamed here from `asset_groups` to `asset_directory_memberships`
-- so this native entity can own the `asset_groups` name and the schema reads
-- unambiguously. The rename is a pure relation rename, data and the sync path
-- are untouched. (Constraint names already carry a legacy `device_groups_*`
-- prefix from an earlier rename; following that precedent we rename the table,
-- its policy and its indexes, and leave the constraint names alone.)

-- 0. Free the `asset_groups` name. The existing junction links assets to
--    directory `groups`, so name it for what it is.
ALTER TABLE public.asset_groups RENAME TO asset_directory_memberships;
ALTER POLICY asset_groups_workspace_isolation
    ON public.asset_directory_memberships RENAME TO asset_directory_memberships_workspace_isolation;
ALTER INDEX idx_asset_groups_asset RENAME TO idx_asset_directory_memberships_asset;
ALTER INDEX idx_asset_groups_group RENAME TO idx_asset_directory_memberships_group;
ALTER INDEX idx_asset_groups_external RENAME TO idx_asset_directory_memberships_external;

-- 1. Native classification entity. Mirrors ticket_categories (uuid, color,
--    display_order, audited) plus the tags soft-archive (`archived_at`).
CREATE TABLE public.asset_groups (
    id integer NOT NULL,
    uuid uuid DEFAULT gen_random_uuid() NOT NULL,
    name character varying(255) NOT NULL,
    description text,
    color character varying(7),
    display_order integer DEFAULT 0 NOT NULL,
    archived_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT asset_groups_pkey PRIMARY KEY (id)
);

CREATE SEQUENCE public.asset_groups_id_seq
    AS integer START WITH 1 INCREMENT BY 1 NO MINVALUE NO MAXVALUE CACHE 1;
ALTER SEQUENCE public.asset_groups_id_seq OWNED BY public.asset_groups.id;
ALTER TABLE ONLY public.asset_groups
    ALTER COLUMN id SET DEFAULT nextval('public.asset_groups_id_seq'::regclass);

ALTER TABLE ONLY public.asset_groups
    ADD CONSTRAINT asset_groups_created_by_fkey
    FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;
ALTER TABLE ONLY public.asset_groups
    ADD CONSTRAINT asset_groups_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);

-- One active group per name per workspace. workspace_id leads the key to
-- satisfy the tenant-table unique-index lint; archived rows are excluded so a
-- name frees up once archived.
CREATE UNIQUE INDEX uq_asset_groups_name_active
    ON public.asset_groups USING btree (workspace_id, lower((name)::text))
    WHERE (archived_at IS NULL);
CREATE INDEX idx_asset_groups_workspace ON public.asset_groups USING btree (workspace_id);

ALTER TABLE ONLY public.asset_groups FORCE ROW LEVEL SECURITY;
ALTER TABLE public.asset_groups OWNER TO nosdesk_admin;
ALTER SEQUENCE public.asset_groups_id_seq OWNER TO nosdesk_admin;
ALTER TABLE public.asset_groups ENABLE ROW LEVEL SECURITY;

CREATE POLICY asset_groups_workspace_isolation ON public.asset_groups
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.asset_groups TO nosdesk_app;
GRANT ALL ON SEQUENCE public.asset_groups_id_seq TO nosdesk_app;

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.asset_groups
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
CREATE TRIGGER tr_audit_asset_groups AFTER INSERT OR DELETE OR UPDATE ON public.asset_groups
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');

-- 2. Membership junction (tag-style, assigned from the asset side). High-churn,
--    so it follows ticket_tags: no audit trigger, no updated_at.
CREATE TABLE public.asset_group_assignments (
    group_id integer NOT NULL,
    asset_id integer NOT NULL,
    added_at timestamp with time zone DEFAULT now() NOT NULL,
    added_by uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT asset_group_assignments_pkey PRIMARY KEY (group_id, asset_id)
);

ALTER TABLE ONLY public.asset_group_assignments
    ADD CONSTRAINT asset_group_assignments_group_id_fkey
    FOREIGN KEY (group_id) REFERENCES public.asset_groups(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.asset_group_assignments
    ADD CONSTRAINT asset_group_assignments_asset_id_fkey
    FOREIGN KEY (asset_id) REFERENCES public.assets(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.asset_group_assignments
    ADD CONSTRAINT asset_group_assignments_added_by_fkey
    FOREIGN KEY (added_by) REFERENCES public.users(uuid) ON DELETE SET NULL;
ALTER TABLE ONLY public.asset_group_assignments
    ADD CONSTRAINT asset_group_assignments_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);

-- pkey covers (group_id, asset_id) for group->assets; add the reverse for the
-- per-asset enrichment lookup.
CREATE INDEX idx_asset_group_assignments_asset ON public.asset_group_assignments USING btree (asset_id);

ALTER TABLE ONLY public.asset_group_assignments FORCE ROW LEVEL SECURITY;
ALTER TABLE public.asset_group_assignments OWNER TO nosdesk_admin;
ALTER TABLE public.asset_group_assignments ENABLE ROW LEVEL SECURITY;

CREATE POLICY asset_group_assignments_workspace_isolation ON public.asset_group_assignments
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.asset_group_assignments TO nosdesk_app;

-- ===================================================================
-- from 2026-06-24-000000_user_contact_fields
-- ===================================================================
-- User contact fields, phase A.
--
-- `user_field_schema`: the workspace's custom-field definitions for users (a
-- JSON-Schema subset, validated by services::custom_fields). Override-only: a
-- row exists once an admin customises it; otherwise reads fall back to a code
-- default, so no per-workspace seed/backfill is needed here.
--
-- `user_profiles`: a per-(user × workspace) contact record holding the SCIM-
-- Enterprise standard columns (job_title, organization, department) plus the
-- custom-field values JSONB. `directory_synced` marks the standard columns as
-- Graph-owned (read-only) for that user. Multi-valued phones/addresses land in
-- phase B. Both tables are workspace-scoped.
--
-- New empty tables: the audit triggers only fire on real runtime writes (which
-- carry app.workspace_id via TenantConn), so there is no backfill to disable.

CREATE TABLE public.user_field_schema (
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    schema jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    CONSTRAINT user_field_schema_pkey PRIMARY KEY (workspace_id)
);

ALTER TABLE ONLY public.user_field_schema
    ADD CONSTRAINT user_field_schema_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.user_field_schema
    ADD CONSTRAINT user_field_schema_created_by_fkey
    FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;

ALTER TABLE ONLY public.user_field_schema FORCE ROW LEVEL SECURITY;
ALTER TABLE public.user_field_schema OWNER TO nosdesk_admin;
ALTER TABLE public.user_field_schema ENABLE ROW LEVEL SECURITY;
CREATE POLICY user_field_schema_workspace_isolation ON public.user_field_schema
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.user_field_schema TO nosdesk_app;
CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.user_field_schema
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
CREATE TRIGGER tr_audit_user_field_schema AFTER INSERT OR DELETE OR UPDATE ON public.user_field_schema
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('workspace_id');


CREATE TABLE public.user_profiles (
    user_uuid uuid NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    job_title character varying(255),
    organization character varying(255),
    department character varying(255),
    custom_fields jsonb DEFAULT '{}'::jsonb NOT NULL,
    directory_synced boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    -- workspace_id leads the composite key to satisfy the tenant-table
    -- unique-index lint (the row is unique per user per workspace either way).
    CONSTRAINT user_profiles_pkey PRIMARY KEY (workspace_id, user_uuid)
);

ALTER TABLE ONLY public.user_profiles
    ADD CONSTRAINT user_profiles_user_uuid_fkey
    FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;
ALTER TABLE ONLY public.user_profiles
    ADD CONSTRAINT user_profiles_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.user_profiles
    ADD CONSTRAINT user_profiles_created_by_fkey
    FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;

CREATE INDEX idx_user_profiles_user ON public.user_profiles USING btree (user_uuid);

ALTER TABLE ONLY public.user_profiles FORCE ROW LEVEL SECURITY;
ALTER TABLE public.user_profiles OWNER TO nosdesk_admin;
ALTER TABLE public.user_profiles ENABLE ROW LEVEL SECURITY;
CREATE POLICY user_profiles_workspace_isolation ON public.user_profiles
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.user_profiles TO nosdesk_app;
CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.user_profiles
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
CREATE TRIGGER tr_audit_user_profiles AFTER INSERT OR DELETE OR UPDATE ON public.user_profiles
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('user_uuid');

-- ===================================================================
-- from 2026-06-24-010000_user_contact_satellites
-- ===================================================================
-- User contact fields, phase B: multi-valued typed phones + addresses.
--
-- vCard TEL / ADR realised as satellite tables like user_emails, but
-- workspace-scoped (the extended contact card is per workspace; email stays the
-- global identity anchor). `source` mirrors user_emails: NULL = manual, a
-- provider name (e.g. 'microsoft') = sync-owned/read-only. One primary per
-- (workspace, user) per table, enforced by a partial unique index.

CREATE TABLE public.user_phone_numbers (
    id integer NOT NULL,
    user_uuid uuid NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    phone character varying(64) NOT NULL,
    phone_type character varying(16) DEFAULT 'work'::character varying NOT NULL,
    is_primary boolean DEFAULT false NOT NULL,
    source character varying(32),
    label character varying(100),
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    CONSTRAINT user_phone_numbers_pkey PRIMARY KEY (id),
    CONSTRAINT user_phone_numbers_type_check CHECK (((phone_type)::text = ANY ((ARRAY['work', 'mobile', 'other'])::text[])))
);
CREATE SEQUENCE public.user_phone_numbers_id_seq
    AS integer START WITH 1 INCREMENT BY 1 NO MINVALUE NO MAXVALUE CACHE 1;
ALTER SEQUENCE public.user_phone_numbers_id_seq OWNED BY public.user_phone_numbers.id;
ALTER TABLE ONLY public.user_phone_numbers
    ALTER COLUMN id SET DEFAULT nextval('public.user_phone_numbers_id_seq'::regclass);
ALTER TABLE ONLY public.user_phone_numbers
    ADD CONSTRAINT user_phone_numbers_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;
ALTER TABLE ONLY public.user_phone_numbers
    ADD CONSTRAINT user_phone_numbers_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.user_phone_numbers
    ADD CONSTRAINT user_phone_numbers_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;
CREATE INDEX idx_user_phone_numbers_user ON public.user_phone_numbers USING btree (workspace_id, user_uuid);
CREATE UNIQUE INDEX uq_user_phone_numbers_primary ON public.user_phone_numbers USING btree (workspace_id, user_uuid) WHERE is_primary;
ALTER TABLE ONLY public.user_phone_numbers FORCE ROW LEVEL SECURITY;
ALTER TABLE public.user_phone_numbers OWNER TO nosdesk_admin;
ALTER SEQUENCE public.user_phone_numbers_id_seq OWNER TO nosdesk_admin;
ALTER TABLE public.user_phone_numbers ENABLE ROW LEVEL SECURITY;
CREATE POLICY user_phone_numbers_workspace_isolation ON public.user_phone_numbers
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.user_phone_numbers TO nosdesk_app;
GRANT ALL ON SEQUENCE public.user_phone_numbers_id_seq TO nosdesk_app;
CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.user_phone_numbers
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
CREATE TRIGGER tr_audit_user_phone_numbers AFTER INSERT OR DELETE OR UPDATE ON public.user_phone_numbers
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


CREATE TABLE public.user_addresses (
    id integer NOT NULL,
    user_uuid uuid NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    address_type character varying(16) DEFAULT 'work'::character varying NOT NULL,
    is_primary boolean DEFAULT false NOT NULL,
    street character varying(255),
    city character varying(128),
    region character varying(128),
    postal_code character varying(32),
    country character varying(128),
    source character varying(32),
    label character varying(100),
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    CONSTRAINT user_addresses_pkey PRIMARY KEY (id),
    CONSTRAINT user_addresses_type_check CHECK (((address_type)::text = ANY ((ARRAY['work', 'home', 'other'])::text[])))
);
CREATE SEQUENCE public.user_addresses_id_seq
    AS integer START WITH 1 INCREMENT BY 1 NO MINVALUE NO MAXVALUE CACHE 1;
ALTER SEQUENCE public.user_addresses_id_seq OWNED BY public.user_addresses.id;
ALTER TABLE ONLY public.user_addresses
    ALTER COLUMN id SET DEFAULT nextval('public.user_addresses_id_seq'::regclass);
ALTER TABLE ONLY public.user_addresses
    ADD CONSTRAINT user_addresses_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;
ALTER TABLE ONLY public.user_addresses
    ADD CONSTRAINT user_addresses_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.user_addresses
    ADD CONSTRAINT user_addresses_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;
CREATE INDEX idx_user_addresses_user ON public.user_addresses USING btree (workspace_id, user_uuid);
CREATE UNIQUE INDEX uq_user_addresses_primary ON public.user_addresses USING btree (workspace_id, user_uuid) WHERE is_primary;
ALTER TABLE ONLY public.user_addresses FORCE ROW LEVEL SECURITY;
ALTER TABLE public.user_addresses OWNER TO nosdesk_admin;
ALTER SEQUENCE public.user_addresses_id_seq OWNER TO nosdesk_admin;
ALTER TABLE public.user_addresses ENABLE ROW LEVEL SECURITY;
CREATE POLICY user_addresses_workspace_isolation ON public.user_addresses
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.user_addresses TO nosdesk_app;
GRANT ALL ON SEQUENCE public.user_addresses_id_seq TO nosdesk_app;
CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.user_addresses
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
CREATE TRIGGER tr_audit_user_addresses AFTER INSERT OR DELETE OR UPDATE ON public.user_addresses
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');

-- ===================================================================
-- from 2026-06-24-020000_scope_user_auth_identities
-- ===================================================================
-- Pre-P0 (LDAP/directory integration): scope auth identities by workspace so the
-- same directory external_id (entryUUID/objectGUID, SCIM externalId) can exist in
-- more than one workspace. Global login identities (local, microsoft, oidc) keep
-- workspace_id NULL and stay unique on (provider_type, external_id); directory
-- identities (ldap, scim) set workspace_id and are unique within their workspace.
--
-- The table has no audit trigger and no RLS, so existing rows simply default to
-- NULL (global) with no backfill needed; login/sync lookups are unchanged.

ALTER TABLE public.user_auth_identities
    ADD COLUMN workspace_id integer;
ALTER TABLE public.user_auth_identities
    ADD CONSTRAINT user_auth_identities_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;

-- Replace the single global unique with two partial uniques.
ALTER TABLE public.user_auth_identities
    DROP CONSTRAINT user_auth_identities_provider_type_external_id_key;
CREATE UNIQUE INDEX user_auth_identities_global_uq
    ON public.user_auth_identities (provider_type, external_id)
    WHERE workspace_id IS NULL;
CREATE UNIQUE INDEX user_auth_identities_scoped_uq
    ON public.user_auth_identities (workspace_id, provider_type, external_id)
    WHERE workspace_id IS NOT NULL;
CREATE INDEX idx_user_auth_identities_workspace_id
    ON public.user_auth_identities (workspace_id);

-- ===================================================================
-- from 2026-06-24-030000_workspace_ldap_settings
-- ===================================================================
-- P1 (LDAP/directory integration): one provider-agnostic LDAP config row per
-- workspace, modeled on workspace_email_settings. The bind password is stored
-- KEK-encrypted (framed AES-256-GCM blob + kek_id sidecar, workspace_id bound
-- into the AAD) and redacted from the audit log via the trigger exclude list.
-- New, empty table: no backfill, so the audit trigger only fires on live writes.
--
-- Flexible/many-valued config (attribute mappings, group model, provisioning
-- policy) lives in JSONB so the 8 provider dialects need no schema branches;
-- the connection/bind/search essentials are typed columns. The mutable sync
-- cursor is a separate table added in P3.

CREATE TABLE public.workspace_ldap_settings (
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    enabled boolean DEFAULT false NOT NULL,
    -- Connection
    host character varying(255) DEFAULT ''::character varying NOT NULL,
    port integer DEFAULT 636 NOT NULL,
    tls_mode character varying(16) DEFAULT 'ldaps'::character varying NOT NULL,
    verify_certs boolean DEFAULT true NOT NULL,
    ca_cert_pem text,
    follow_referrals boolean DEFAULT false NOT NULL,
    connect_timeout_secs integer DEFAULT 5 NOT NULL,
    -- Bind / auth (service account)
    auth_mode character varying(16) DEFAULT 'simple_bind'::character varying NOT NULL,
    bind_dn character varying(512) DEFAULT ''::character varying NOT NULL,
    encrypted_bind_password bytea,
    encrypted_kek_id smallint,
    -- User search
    user_base_dn character varying(512) DEFAULT ''::character varying NOT NULL,
    username_attribute character varying(64) DEFAULT 'sAMAccountName'::character varying NOT NULL,
    user_filter text DEFAULT ''::text NOT NULL,
    page_size integer DEFAULT 500 NOT NULL,
    -- Mappings + group model + provisioning policy (provider-agnostic JSONB)
    attribute_map jsonb DEFAULT '{}'::jsonb NOT NULL,
    group_config jsonb DEFAULT '{}'::jsonb NOT NULL,
    provisioning jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT workspace_ldap_settings_pkey PRIMARY KEY (workspace_id),
    CONSTRAINT workspace_ldap_settings_tls_mode_check
        CHECK (((tls_mode)::text = ANY ((ARRAY['ldaps', 'starttls', 'plain'])::text[]))),
    CONSTRAINT workspace_ldap_settings_auth_mode_check
        CHECK (((auth_mode)::text = ANY ((ARRAY['simple_bind', 'mtls'])::text[])))
);
ALTER TABLE ONLY public.workspace_ldap_settings
    ADD CONSTRAINT workspace_ldap_settings_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.workspace_ldap_settings FORCE ROW LEVEL SECURITY;
ALTER TABLE public.workspace_ldap_settings OWNER TO nosdesk_admin;
ALTER TABLE public.workspace_ldap_settings ENABLE ROW LEVEL SECURITY;
CREATE POLICY workspace_ldap_settings_workspace_isolation ON public.workspace_ldap_settings
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.workspace_ldap_settings TO nosdesk_app;
CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.workspace_ldap_settings
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
CREATE TRIGGER tr_audit_workspace_ldap_settings
    AFTER INSERT OR DELETE OR UPDATE ON public.workspace_ldap_settings
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('workspace_id', 'encrypted_bind_password');

-- ===================================================================
-- from 2026-06-25-000000_ldap_tls_mode_drop_plain
-- ===================================================================
-- Security (review must-fix): never allow a cleartext "plain" LDAP bind -- it
-- ships the service-account + end-user passwords over an unencrypted socket
-- (RFC 4513 §5.1.3). Tighten the tls_mode check to LDAPS / StartTLS only. The
-- app layer (connector + admin validator) already rejects "plain"; this closes
-- it at the DB too. New feature, so no existing row carries "plain".
ALTER TABLE public.workspace_ldap_settings
    DROP CONSTRAINT workspace_ldap_settings_tls_mode_check;
ALTER TABLE public.workspace_ldap_settings
    ADD CONSTRAINT workspace_ldap_settings_tls_mode_check
    CHECK (((tls_mode)::text = ANY ((ARRAY['ldaps', 'starttls'])::text[])));

-- ===================================================================
-- from 2026-06-25-010000_workspace_ldap_sync_state
-- ===================================================================
-- DirSync cursor state, one row per workspace. The opaque DirSync cookie is
-- client-held (unlike Graph's server-held deltaLink), so it MUST survive
-- restarts. Operational state, not audited; RLS-isolated like the config row.
CREATE TABLE public.workspace_ldap_sync_state (
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    -- Which incremental mechanism the cookie belongs to (v1: dirsync).
    mechanism character varying(16) DEFAULT 'dirsync'::character varying NOT NULL,
    -- The opaque cursor (DirSync cookie). NULL = no cursor yet -> next run is a
    -- full sync (an empty-cookie DirSync returns everything + a fresh cookie).
    cookie bytea,
    -- When the last full reconcile completed (for the nightly safety pass).
    last_full_reconcile_at timestamp with time zone,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT workspace_ldap_sync_state_pkey PRIMARY KEY (workspace_id)
);
ALTER TABLE ONLY public.workspace_ldap_sync_state
    ADD CONSTRAINT workspace_ldap_sync_state_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.workspace_ldap_sync_state FORCE ROW LEVEL SECURITY;
ALTER TABLE public.workspace_ldap_sync_state OWNER TO nosdesk_admin;
ALTER TABLE public.workspace_ldap_sync_state ENABLE ROW LEVEL SECURITY;
CREATE POLICY workspace_ldap_sync_state_workspace_isolation ON public.workspace_ldap_sync_state
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.workspace_ldap_sync_state TO nosdesk_app;
CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.workspace_ldap_sync_state
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();

