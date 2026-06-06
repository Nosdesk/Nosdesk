-- Squashed initial schema + seed data.
--
-- Generated from a fully-migrated database via `pg_dump` (schema-only
-- + data-only --inserts --disable-triggers), collapsing ~165
-- incremental migrations into one baseline. Single source of truth for
-- a fresh database; existing databases keep their history and aren't
-- re-run.
--
-- pg_dump emits valid FKs and whole `CREATE TYPE ... AS ENUM`, so the
-- PG18-only forms that broke fresh applies on the Postgres 17 pin
-- (`ALTER TYPE ADD VALUE`, `NOT VALID` FK on a partitioned table) are
-- gone. Seed data loads with per-table `DISABLE TRIGGER ALL` so it
-- doesn't fire audit/sync triggers.
--
-- Roles + the membership grant are recreated here (pg_dump
-- --schema-only omits global role objects + memberships), ahead of the
-- OWNER TO / GRANT statements the dump body relies on.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'nosdesk_app') THEN
        CREATE ROLE nosdesk_app NOLOGIN NOBYPASSRLS NOINHERIT;
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'nosdesk_admin') THEN
        CREATE ROLE nosdesk_admin NOLOGIN BYPASSRLS;
    END IF;
END
$$;

GRANT nosdesk_admin TO nosdesk_app WITH INHERIT FALSE, SET TRUE;

-- ============ SCHEMA ============
--
-- PostgreSQL database dump
--


-- Dumped from database version 18.4 (Debian 18.4-1.pgdg12+1)
-- Dumped by pg_dump version 18.4 (Debian 18.4-1.pgdg12+1)


--
-- Name: pgcrypto; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public;


--
-- Name: uuid-ossp; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS "uuid-ossp" WITH SCHEMA public;


--
-- Name: assignment_method; Type: TYPE; Schema: public; Owner: nosdesk
--

CREATE TYPE public.assignment_method AS ENUM (
    'direct_user',
    'group_round_robin',
    'group_random',
    'group_queue'
);


ALTER TYPE public.assignment_method OWNER TO nosdesk;

--
-- Name: documentation_status; Type: TYPE; Schema: public; Owner: nosdesk
--

CREATE TYPE public.documentation_status AS ENUM (
    'draft',
    'published',
    'archived',
    'deleted'
);


ALTER TYPE public.documentation_status OWNER TO nosdesk;

--
-- Name: project_status; Type: TYPE; Schema: public; Owner: nosdesk
--

CREATE TYPE public.project_status AS ENUM (
    'active',
    'completed',
    'archived'
);


ALTER TYPE public.project_status OWNER TO nosdesk;

--
-- Name: rule_application_status; Type: TYPE; Schema: public; Owner: nosdesk
--

CREATE TYPE public.rule_application_status AS ENUM (
    'succeeded',
    'dry_run',
    'skipped_preflight',
    'skipped_condition_unmet',
    'suppressed_recursion_budget',
    'suppressed_loop_guard',
    'failed'
);


ALTER TYPE public.rule_application_status OWNER TO nosdesk;

--
-- Name: rule_state; Type: TYPE; Schema: public; Owner: nosdesk
--

CREATE TYPE public.rule_state AS ENUM (
    'draft',
    'dry_run',
    'live',
    'archived'
);


ALTER TYPE public.rule_state OWNER TO nosdesk;

--
-- Name: rule_trigger_kind; Type: TYPE; Schema: public; Owner: nosdesk
--

CREATE TYPE public.rule_trigger_kind AS ENUM (
    'manual',
    'ticket_created',
    'ticket_updated',
    'ticket_replied',
    'time_elapsed'
);


ALTER TYPE public.rule_trigger_kind OWNER TO nosdesk;

--
-- Name: sync_aggregate; Type: TYPE; Schema: public; Owner: nosdesk
--

CREATE TYPE public.sync_aggregate AS ENUM (
    'ticket',
    'project',
    'project_ticket',
    'workflow_state',
    'comment',
    'attachment',
    'assignment',
    'group_membership',
    'plugin',
    'cycle',
    'cycle_ticket',
    'user',
    'asset',
    'webhook',
    'channel',
    'knowledge_gap',
    'documentation_page',
    'documentation_collection',
    'data',
    'notification'
);


ALTER TYPE public.sync_aggregate OWNER TO nosdesk;

--
-- Name: sync_op; Type: TYPE; Schema: public; Owner: nosdesk
--

CREATE TYPE public.sync_op AS ENUM (
    'I',
    'U',
    'D',
    'A'
);


ALTER TYPE public.sync_op OWNER TO nosdesk;

--
-- Name: ticket_priority; Type: TYPE; Schema: public; Owner: nosdesk
--

CREATE TYPE public.ticket_priority AS ENUM (
    'low',
    'medium',
    'high'
);


ALTER TYPE public.ticket_priority OWNER TO nosdesk;

--
-- Name: user_role; Type: TYPE; Schema: public; Owner: nosdesk
--

CREATE TYPE public.user_role AS ENUM (
    'admin',
    'technician',
    'user',
    'audit_reviewer'
);


ALTER TYPE public.user_role OWNER TO nosdesk;

--
-- Name: workflow_state_category; Type: TYPE; Schema: public; Owner: nosdesk
--

CREATE TYPE public.workflow_state_category AS ENUM (
    'triage',
    'backlog',
    'active',
    'in_review',
    'done',
    'cancelled',
    'merged'
);


ALTER TYPE public.workflow_state_category OWNER TO nosdesk;

--
-- Name: audit_log_trigger(); Type: FUNCTION; Schema: public; Owner: nosdesk
--

CREATE FUNCTION public.audit_log_trigger() RETURNS trigger
    LANGUAGE plpgsql
    AS $_$
DECLARE
    actor    UUID := NULLIF(current_setting('app.actor_uuid', true), '')::UUID;
    corr     UUID := NULLIF(current_setting('app.correlation_id', true), '')::UUID;
    pk       TEXT;
    excluded TEXT[] := ARRAY[]::TEXT[];
    col      TEXT;
    i        INT;
    old_j    JSONB;
    new_j    JSONB;
    before_j JSONB;
    after_j  JSONB;
BEGIN
    -- D5: suppress audit capture for writes made inside an audit-read
    -- transaction. Reads de-duplicate to the single data.audit.read
    -- event the handler emits via emit::record (a different table).
    IF current_setting('nosdesk.in_audit_read', true) = 'true' THEN
        RETURN COALESCE(NEW, OLD);
    END IF;

    -- Layer 2: refuse to write an audit row with no workspace context.
    -- audit_log.workspace_id defaults from this GUC and is NOT NULL, so
    -- an unset GUC would otherwise surface as an opaque NOT NULL
    -- violation. Name the remedy in the message.
    IF NULLIF(current_setting('app.workspace_id', true), '') IS NULL THEN
        RAISE EXCEPTION
            'audit context missing for %.%: wrap the write in with_actor_context() or with_actor_bypass_context()',
            TG_TABLE_NAME, TG_OP
            USING ERRCODE = 'NDX01';
    END IF;

    -- Collect the excluded-column names (TG_ARGV is 0-based; index 0 is
    -- the PK column, 1..N-1 are the columns to redact).
    IF TG_NARGS > 1 THEN
        FOR i IN 1 .. (TG_NARGS - 1) LOOP
            excluded := array_append(excluded, TG_ARGV[i]);
        END LOOP;
    END IF;

    IF TG_OP = 'INSERT' THEN
        EXECUTE format('SELECT ($1).%I::TEXT', TG_ARGV[0]) INTO pk USING NEW;
        new_j := to_jsonb(NEW);
        after_j := new_j - excluded;
        FOREACH col IN ARRAY excluded LOOP
            -- `->>` yields SQL NULL for a JSON-null / absent value
            -- (unlike `->`, which returns the JSONB value 'null'), so
            -- this reads as "a non-null value was provided".
            after_j := after_j
                || jsonb_build_object(col || '_changed', (new_j ->> col) IS NOT NULL);
        END LOOP;
        INSERT INTO audit_log (table_name, pk_text, op, after_jsonb, actor_uuid, correlation_id)
        VALUES (TG_TABLE_NAME, pk, 'I', after_j, actor, corr);
        RETURN NEW;

    ELSIF TG_OP = 'UPDATE' THEN
        EXECUTE format('SELECT ($1).%I::TEXT', TG_ARGV[0]) INTO pk USING NEW;
        old_j := to_jsonb(OLD);
        new_j := to_jsonb(NEW);
        before_j := old_j - excluded;
        after_j := new_j - excluded;
        FOREACH col IN ARRAY excluded LOOP
            after_j := after_j
                || jsonb_build_object(col || '_changed', (old_j -> col) IS DISTINCT FROM (new_j -> col));
        END LOOP;
        INSERT INTO audit_log (table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id)
        VALUES (
            TG_TABLE_NAME,
            pk,
            'U',
            before_j,
            after_j,
            -- changed_cols is computed from the full rows so an excluded
            -- column that changed is still named here (just not valued).
            ARRAY(
                SELECT k FROM jsonb_each(new_j) e1(k, v1)
                WHERE new_j -> e1.k IS DISTINCT FROM old_j -> e1.k
            ),
            actor,
            corr
        );
        RETURN NEW;

    ELSE
        EXECUTE format('SELECT ($1).%I::TEXT', TG_ARGV[0]) INTO pk USING OLD;
        old_j := to_jsonb(OLD);
        before_j := old_j - excluded;
        FOREACH col IN ARRAY excluded LOOP
            -- `->>` (not `->`) so a JSON-null column reads as "had no
            -- value", matching the INSERT branch.
            before_j := before_j
                || jsonb_build_object(col || '_changed', (old_j ->> col) IS NOT NULL);
        END LOOP;
        INSERT INTO audit_log (table_name, pk_text, op, before_jsonb, actor_uuid, correlation_id)
        VALUES (TG_TABLE_NAME, pk, 'D', before_j, actor, corr);
        RETURN OLD;
    END IF;
END;
$_$;


ALTER FUNCTION public.audit_log_trigger() OWNER TO nosdesk;

--
-- Name: auto_create_user_preferences(); Type: FUNCTION; Schema: public; Owner: nosdesk
--

CREATE FUNCTION public.auto_create_user_preferences() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    INSERT INTO user_preferences (user_uuid)
    VALUES (NEW.uuid)
    ON CONFLICT (user_uuid) DO NOTHING;
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.auto_create_user_preferences() OWNER TO nosdesk;

--
-- Name: diesel_manage_updated_at(regclass); Type: FUNCTION; Schema: public; Owner: nosdesk
--

CREATE FUNCTION public.diesel_manage_updated_at(_tbl regclass) RETURNS void
    LANGUAGE plpgsql
    AS $$
BEGIN
    EXECUTE format('CREATE TRIGGER set_updated_at BEFORE UPDATE ON %s
                    FOR EACH ROW EXECUTE PROCEDURE diesel_set_updated_at()', _tbl);
END;
$$;


ALTER FUNCTION public.diesel_manage_updated_at(_tbl regclass) OWNER TO nosdesk;

--
-- Name: diesel_set_updated_at(); Type: FUNCTION; Schema: public; Owner: nosdesk
--

CREATE FUNCTION public.diesel_set_updated_at() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF (
        NEW IS DISTINCT FROM OLD AND
        NEW.updated_at IS NOT DISTINCT FROM OLD.updated_at
    ) THEN
        NEW.updated_at := current_timestamp;
    END IF;
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.diesel_set_updated_at() OWNER TO nosdesk;

--
-- Name: outbound_emails_notify_trigger(); Type: FUNCTION; Schema: public; Owner: nosdesk
--

CREATE FUNCTION public.outbound_emails_notify_trigger() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    PERFORM pg_notify('outbound_emails_new', '');
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.outbound_emails_notify_trigger() OWNER TO nosdesk;

--
-- Name: rules_write_initial_version(); Type: FUNCTION; Schema: public; Owner: nosdesk_admin
--

CREATE FUNCTION public.rules_write_initial_version() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    actor UUID := NULLIF(current_setting('app.actor_uuid', true), '')::UUID;
BEGIN
    INSERT INTO rule_versions (
        rule_id, workspace_id, version, name, description, trigger_kind,
        trigger_config, conditions, actions, state, priority,
        saved_by, saved_at
    )
    VALUES (
        NEW.id, NEW.workspace_id, 1, NEW.name, NEW.description, NEW.trigger_kind,
        NEW.trigger_config, NEW.conditions, NEW.actions, NEW.state, NEW.priority,
        COALESCE(actor, NEW.created_by), NEW.created_at
    );
    RETURN NULL;
END;
$$;


ALTER FUNCTION public.rules_write_initial_version() OWNER TO nosdesk_admin;

--
-- Name: rules_write_update_version(); Type: FUNCTION; Schema: public; Owner: nosdesk_admin
--

CREATE FUNCTION public.rules_write_update_version() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    next_version INTEGER;
    actor        UUID := NULLIF(current_setting('app.actor_uuid', true), '')::UUID;
BEGIN
    SELECT COALESCE(MAX(version), 0) + 1
    INTO next_version
    FROM rule_versions
    WHERE rule_id = NEW.id;

    INSERT INTO rule_versions (
        rule_id, workspace_id, version, name, description, trigger_kind,
        trigger_config, conditions, actions, state, priority,
        saved_by, saved_at
    )
    VALUES (
        NEW.id, NEW.workspace_id, next_version, NEW.name, NEW.description, NEW.trigger_kind,
        NEW.trigger_config, NEW.conditions, NEW.actions, NEW.state, NEW.priority,
        COALESCE(actor, NEW.created_by), NOW()
    );

    NEW.updated_at := NOW();
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.rules_write_update_version() OWNER TO nosdesk_admin;

--
-- Name: sync_actions_notify(); Type: FUNCTION; Schema: public; Owner: nosdesk
--

CREATE FUNCTION public.sync_actions_notify() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    PERFORM pg_notify('sync_actions_new', '');
    RETURN NULL;
END;
$$;


ALTER FUNCTION public.sync_actions_notify() OWNER TO nosdesk;

--
-- Name: uuid_generate_v7(); Type: FUNCTION; Schema: public; Owner: nosdesk
--

CREATE FUNCTION public.uuid_generate_v7() RETURNS uuid
    LANGUAGE plpgsql
    AS $$
DECLARE
    unix_ts_ms BIGINT;
    rand_bytes BYTEA;
    ts_hex TEXT;
BEGIN
    -- Get current timestamp in milliseconds since Unix epoch
    unix_ts_ms := (EXTRACT(EPOCH FROM clock_timestamp()) * 1000)::BIGINT;

    -- Build 6-byte timestamp as hex (12 hex chars)
    ts_hex := LPAD(TO_HEX(unix_ts_ms), 12, '0');

    -- Generate 10 random bytes for version, variant, and random portions
    rand_bytes := gen_random_bytes(10);

    -- Set version (4 bits = 0x7) in first random byte
    rand_bytes := SET_BYTE(rand_bytes, 0, (GET_BYTE(rand_bytes, 0) & 15) | 112);

    -- Set variant (2 bits = 0b10) in third random byte (position 8 in UUID)
    rand_bytes := SET_BYTE(rand_bytes, 2, (GET_BYTE(rand_bytes, 2) & 63) | 128);

    RETURN CAST(ts_hex || ENCODE(rand_bytes, 'hex') AS UUID);
END;
$$;


ALTER FUNCTION public.uuid_generate_v7() OWNER TO nosdesk;

--
-- Name: webhook_outbox_enqueue(); Type: FUNCTION; Schema: public; Owner: nosdesk_admin
--

CREATE FUNCTION public.webhook_outbox_enqueue() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    AS $$
BEGIN
    INSERT INTO webhook_outbox (sync_id) VALUES (NEW.sync_id);
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.webhook_outbox_enqueue() OWNER TO nosdesk_admin;

SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: active_sessions; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.active_sessions (
    id integer NOT NULL,
    user_uuid uuid NOT NULL,
    device_name character varying(255),
    ip_address inet,
    user_agent text,
    location character varying(255),
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    last_active timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    is_current boolean DEFAULT false NOT NULL,
    session_id uuid DEFAULT gen_random_uuid() NOT NULL,
    CONSTRAINT session_times_valid CHECK ((expires_at > created_at))
);


ALTER TABLE public.active_sessions OWNER TO nosdesk;

--
-- Name: active_sessions_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk
--

CREATE SEQUENCE public.active_sessions_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.active_sessions_id_seq OWNER TO nosdesk;

--
-- Name: active_sessions_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk
--

ALTER SEQUENCE public.active_sessions_id_seq OWNED BY public.active_sessions.id;


--
-- Name: api_tokens; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.api_tokens (
    id integer NOT NULL,
    uuid uuid DEFAULT public.uuid_generate_v7() NOT NULL,
    token_hash character varying(64) NOT NULL,
    token_prefix character varying(8) NOT NULL,
    user_uuid uuid NOT NULL,
    name character varying(255) NOT NULL,
    scopes text[] DEFAULT ARRAY['full'::text],
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid NOT NULL,
    expires_at timestamp with time zone,
    revoked_at timestamp with time zone,
    last_used_at timestamp with time zone,
    last_used_ip inet,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    is_platform_scoped boolean DEFAULT false NOT NULL
);

ALTER TABLE ONLY public.api_tokens FORCE ROW LEVEL SECURITY;


ALTER TABLE public.api_tokens OWNER TO nosdesk_admin;

--
-- Name: api_tokens_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.api_tokens_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.api_tokens_id_seq OWNER TO nosdesk_admin;

--
-- Name: api_tokens_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.api_tokens_id_seq OWNED BY public.api_tokens.id;


--
-- Name: article_content_revisions; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.article_content_revisions (
    id integer NOT NULL,
    article_content_id integer NOT NULL,
    revision_number integer NOT NULL,
    yjs_state_vector bytea NOT NULL,
    yjs_document_content bytea NOT NULL,
    contributed_by uuid[] DEFAULT '{}'::uuid[] NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.article_content_revisions FORCE ROW LEVEL SECURITY;


ALTER TABLE public.article_content_revisions OWNER TO nosdesk_admin;

--
-- Name: article_content_revisions_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.article_content_revisions_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.article_content_revisions_id_seq OWNER TO nosdesk_admin;

--
-- Name: article_content_revisions_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.article_content_revisions_id_seq OWNED BY public.article_content_revisions.id;


--
-- Name: article_contents; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.article_contents (
    id integer NOT NULL,
    ticket_id integer,
    current_revision_number integer DEFAULT 1 NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_by uuid,
    yjs_state_vector bytea,
    yjs_document bytea,
    yjs_client_id bigint,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.article_contents FORCE ROW LEVEL SECURITY;


ALTER TABLE public.article_contents OWNER TO nosdesk_admin;

--
-- Name: article_contents_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.article_contents_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.article_contents_id_seq OWNER TO nosdesk_admin;

--
-- Name: article_contents_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.article_contents_id_seq OWNED BY public.article_contents.id;


--
-- Name: asset_audits; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.asset_audits (
    id bigint NOT NULL,
    asset_id integer NOT NULL,
    counted_quantity numeric(12,3) NOT NULL,
    previous_quantity numeric(12,3) NOT NULL,
    delta numeric(12,3) NOT NULL,
    notes text,
    recorded_by uuid,
    recorded_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT asset_audits_counted_quantity_check CHECK ((counted_quantity >= (0)::numeric))
);

ALTER TABLE ONLY public.asset_audits FORCE ROW LEVEL SECURITY;


ALTER TABLE public.asset_audits OWNER TO nosdesk_admin;

--
-- Name: asset_audits_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.asset_audits_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.asset_audits_id_seq OWNER TO nosdesk_admin;

--
-- Name: asset_audits_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.asset_audits_id_seq OWNED BY public.asset_audits.id;


--
-- Name: asset_groups; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.asset_groups (
    asset_id integer CONSTRAINT device_groups_device_id_not_null NOT NULL,
    group_id integer CONSTRAINT device_groups_group_id_not_null NOT NULL,
    created_at timestamp with time zone DEFAULT now() CONSTRAINT device_groups_created_at_not_null NOT NULL,
    created_by uuid,
    external_source character varying(50),
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.asset_groups FORCE ROW LEVEL SECURITY;


ALTER TABLE public.asset_groups OWNER TO nosdesk_admin;

--
-- Name: asset_kinds; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.asset_kinds (
    id integer NOT NULL,
    slug character varying(64) NOT NULL,
    label character varying(255) NOT NULL,
    description text,
    icon character varying(64),
    attribute_schema jsonb DEFAULT '{"type": "object", "properties": {}}'::jsonb NOT NULL,
    sort_order integer DEFAULT 100 NOT NULL,
    is_builtin boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    category character varying(16) DEFAULT 'generic'::character varying NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT asset_kinds_category_check CHECK (((category)::text = ANY ((ARRAY['it'::character varying, 'logical'::character varying, 'physical'::character varying, 'bulk'::character varying, 'generic'::character varying])::text[])))
);

ALTER TABLE ONLY public.asset_kinds FORCE ROW LEVEL SECURITY;


ALTER TABLE public.asset_kinds OWNER TO nosdesk_admin;

--
-- Name: asset_kinds_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.asset_kinds_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.asset_kinds_id_seq OWNER TO nosdesk_admin;

--
-- Name: asset_kinds_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.asset_kinds_id_seq OWNED BY public.asset_kinds.id;


--
-- Name: asset_usage_log; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.asset_usage_log (
    id bigint NOT NULL,
    asset_id integer NOT NULL,
    ticket_id integer,
    quantity_used numeric(12,3) NOT NULL,
    unit character varying(32) NOT NULL,
    recorded_by uuid,
    recorded_at timestamp with time zone DEFAULT now() NOT NULL,
    notes text,
    event_kind character varying(16) NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT asset_usage_log_event_kind_check CHECK (((event_kind)::text = ANY ((ARRAY['usage'::character varying, 'restock'::character varying])::text[]))),
    CONSTRAINT asset_usage_log_quantity_used_check CHECK ((quantity_used > (0)::numeric))
);

ALTER TABLE ONLY public.asset_usage_log FORCE ROW LEVEL SECURITY;


ALTER TABLE public.asset_usage_log OWNER TO nosdesk_admin;

--
-- Name: asset_usage_log_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.asset_usage_log_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.asset_usage_log_id_seq OWNER TO nosdesk_admin;

--
-- Name: asset_usage_log_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.asset_usage_log_id_seq OWNED BY public.asset_usage_log.id;


--
-- Name: assets; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.assets (
    id integer CONSTRAINT devices_id_not_null NOT NULL,
    name character varying(255) CONSTRAINT devices_name_not_null NOT NULL,
    serial_number character varying(255),
    manufacturer character varying(255),
    model character varying(255),
    location character varying(255),
    created_at timestamp with time zone DEFAULT now() CONSTRAINT devices_created_at_not_null NOT NULL,
    updated_at timestamp with time zone DEFAULT now() CONSTRAINT devices_updated_at_not_null NOT NULL,
    created_by uuid,
    notes text,
    primary_user_uuid uuid,
    purchase_date date,
    asset_tag character varying(255),
    kind character varying(64) DEFAULT 'generic'::character varying NOT NULL,
    attributes jsonb DEFAULT '{}'::jsonb NOT NULL,
    quantity numeric(12,3),
    unit character varying(32),
    external_sync_source character varying(32),
    low_stock_threshold numeric(12,3),
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT assets_low_stock_threshold_nonneg CHECK (((low_stock_threshold IS NULL) OR (low_stock_threshold >= (0)::numeric)))
);

ALTER TABLE ONLY public.assets FORCE ROW LEVEL SECURITY;


ALTER TABLE public.assets OWNER TO nosdesk_admin;

--
-- Name: assignment_log; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.assignment_log (
    id integer NOT NULL,
    ticket_id integer NOT NULL,
    rule_id integer,
    trigger_type character varying(50) NOT NULL,
    previous_assignee_uuid uuid,
    new_assignee_uuid uuid,
    method public.assignment_method NOT NULL,
    context jsonb DEFAULT '{}'::jsonb,
    assigned_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.assignment_log FORCE ROW LEVEL SECURITY;


ALTER TABLE public.assignment_log OWNER TO nosdesk_admin;

--
-- Name: assignment_log_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.assignment_log_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.assignment_log_id_seq OWNER TO nosdesk_admin;

--
-- Name: assignment_log_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.assignment_log_id_seq OWNED BY public.assignment_log.id;


--
-- Name: assignment_rule_state; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.assignment_rule_state (
    rule_id integer NOT NULL,
    last_assigned_index integer DEFAULT 0 NOT NULL,
    total_assignments integer DEFAULT 0 NOT NULL,
    last_assigned_at timestamp with time zone,
    last_assigned_user_uuid uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.assignment_rule_state FORCE ROW LEVEL SECURITY;


ALTER TABLE public.assignment_rule_state OWNER TO nosdesk_admin;

--
-- Name: assignment_rules; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.assignment_rules (
    id integer NOT NULL,
    uuid uuid DEFAULT gen_random_uuid() NOT NULL,
    name character varying(255) NOT NULL,
    description text,
    priority integer DEFAULT 100 NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    method public.assignment_method NOT NULL,
    target_user_uuid uuid,
    target_group_id integer,
    trigger_on_create boolean DEFAULT true NOT NULL,
    trigger_on_category_change boolean DEFAULT true NOT NULL,
    category_id integer,
    conditions jsonb DEFAULT '{}'::jsonb,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.assignment_rules FORCE ROW LEVEL SECURITY;


ALTER TABLE public.assignment_rules OWNER TO nosdesk_admin;

--
-- Name: assignment_rules_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.assignment_rules_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.assignment_rules_id_seq OWNER TO nosdesk_admin;

--
-- Name: assignment_rules_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.assignment_rules_id_seq OWNED BY public.assignment_rules.id;


--
-- Name: attachments; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.attachments (
    id integer NOT NULL,
    url character varying(2048) NOT NULL,
    name character varying(255) NOT NULL,
    file_size bigint,
    mime_type character varying(100),
    checksum character varying(64),
    comment_id integer,
    uploaded_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    transcription text,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.attachments FORCE ROW LEVEL SECURITY;


ALTER TABLE public.attachments OWNER TO nosdesk_admin;

--
-- Name: attachments_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.attachments_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.attachments_id_seq OWNER TO nosdesk_admin;

--
-- Name: attachments_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.attachments_id_seq OWNED BY public.attachments.id;


--
-- Name: audit_log; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.audit_log (
    id bigint NOT NULL,
    table_name text NOT NULL,
    pk_text text NOT NULL,
    op character(1) NOT NULL,
    before_jsonb jsonb,
    after_jsonb jsonb,
    changed_cols text[],
    actor_uuid uuid,
    correlation_id uuid,
    occurred_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT audit_log_op_check CHECK ((op = ANY (ARRAY['I'::bpchar, 'U'::bpchar, 'D'::bpchar])))
)
PARTITION BY RANGE (occurred_at);

ALTER TABLE ONLY public.audit_log FORCE ROW LEVEL SECURITY;


ALTER TABLE public.audit_log OWNER TO nosdesk_admin;

--
-- Name: audit_log_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.audit_log_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.audit_log_id_seq OWNER TO nosdesk_admin;

--
-- Name: audit_log_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.audit_log_id_seq OWNED BY public.audit_log.id;


--
-- Name: audit_log_2026_05; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.audit_log_2026_05 (
    id bigint DEFAULT nextval('public.audit_log_id_seq'::regclass) CONSTRAINT audit_log_id_not_null NOT NULL,
    table_name text CONSTRAINT audit_log_table_name_not_null NOT NULL,
    pk_text text CONSTRAINT audit_log_pk_text_not_null NOT NULL,
    op character(1) CONSTRAINT audit_log_op_not_null NOT NULL,
    before_jsonb jsonb,
    after_jsonb jsonb,
    changed_cols text[],
    actor_uuid uuid,
    correlation_id uuid,
    occurred_at timestamp with time zone DEFAULT clock_timestamp() CONSTRAINT audit_log_occurred_at_not_null NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer CONSTRAINT audit_log_workspace_id_not_null NOT NULL,
    CONSTRAINT audit_log_op_check CHECK ((op = ANY (ARRAY['I'::bpchar, 'U'::bpchar, 'D'::bpchar])))
);

ALTER TABLE ONLY public.audit_log_2026_05 FORCE ROW LEVEL SECURITY;


ALTER TABLE public.audit_log_2026_05 OWNER TO nosdesk_admin;

--
-- Name: audit_log_2026_06; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.audit_log_2026_06 (
    id bigint DEFAULT nextval('public.audit_log_id_seq'::regclass) CONSTRAINT audit_log_id_not_null NOT NULL,
    table_name text CONSTRAINT audit_log_table_name_not_null NOT NULL,
    pk_text text CONSTRAINT audit_log_pk_text_not_null NOT NULL,
    op character(1) CONSTRAINT audit_log_op_not_null NOT NULL,
    before_jsonb jsonb,
    after_jsonb jsonb,
    changed_cols text[],
    actor_uuid uuid,
    correlation_id uuid,
    occurred_at timestamp with time zone DEFAULT clock_timestamp() CONSTRAINT audit_log_occurred_at_not_null NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer CONSTRAINT audit_log_workspace_id_not_null NOT NULL,
    CONSTRAINT audit_log_op_check CHECK ((op = ANY (ARRAY['I'::bpchar, 'U'::bpchar, 'D'::bpchar])))
);

ALTER TABLE ONLY public.audit_log_2026_06 FORCE ROW LEVEL SECURITY;


ALTER TABLE public.audit_log_2026_06 OWNER TO nosdesk_admin;

--
-- Name: audit_log_2026_07; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.audit_log_2026_07 (
    id bigint DEFAULT nextval('public.audit_log_id_seq'::regclass) CONSTRAINT audit_log_id_not_null NOT NULL,
    table_name text CONSTRAINT audit_log_table_name_not_null NOT NULL,
    pk_text text CONSTRAINT audit_log_pk_text_not_null NOT NULL,
    op character(1) CONSTRAINT audit_log_op_not_null NOT NULL,
    before_jsonb jsonb,
    after_jsonb jsonb,
    changed_cols text[],
    actor_uuid uuid,
    correlation_id uuid,
    occurred_at timestamp with time zone DEFAULT clock_timestamp() CONSTRAINT audit_log_occurred_at_not_null NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer CONSTRAINT audit_log_workspace_id_not_null NOT NULL,
    CONSTRAINT audit_log_op_check CHECK ((op = ANY (ARRAY['I'::bpchar, 'U'::bpchar, 'D'::bpchar])))
);

ALTER TABLE ONLY public.audit_log_2026_07 FORCE ROW LEVEL SECURITY;


ALTER TABLE public.audit_log_2026_07 OWNER TO nosdesk_admin;

--
-- Name: audit_log_2026_08; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.audit_log_2026_08 (
    id bigint DEFAULT nextval('public.audit_log_id_seq'::regclass) CONSTRAINT audit_log_id_not_null NOT NULL,
    table_name text CONSTRAINT audit_log_table_name_not_null NOT NULL,
    pk_text text CONSTRAINT audit_log_pk_text_not_null NOT NULL,
    op character(1) CONSTRAINT audit_log_op_not_null NOT NULL,
    before_jsonb jsonb,
    after_jsonb jsonb,
    changed_cols text[],
    actor_uuid uuid,
    correlation_id uuid,
    occurred_at timestamp with time zone DEFAULT clock_timestamp() CONSTRAINT audit_log_occurred_at_not_null NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer CONSTRAINT audit_log_workspace_id_not_null NOT NULL,
    CONSTRAINT audit_log_op_check CHECK ((op = ANY (ARRAY['I'::bpchar, 'U'::bpchar, 'D'::bpchar])))
);

ALTER TABLE ONLY public.audit_log_2026_08 FORCE ROW LEVEL SECURITY;


ALTER TABLE public.audit_log_2026_08 OWNER TO nosdesk_admin;

--
-- Name: audit_log_default; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.audit_log_default (
    id bigint DEFAULT nextval('public.audit_log_id_seq'::regclass) CONSTRAINT audit_log_id_not_null NOT NULL,
    table_name text CONSTRAINT audit_log_table_name_not_null NOT NULL,
    pk_text text CONSTRAINT audit_log_pk_text_not_null NOT NULL,
    op character(1) CONSTRAINT audit_log_op_not_null NOT NULL,
    before_jsonb jsonb,
    after_jsonb jsonb,
    changed_cols text[],
    actor_uuid uuid,
    correlation_id uuid,
    occurred_at timestamp with time zone DEFAULT clock_timestamp() CONSTRAINT audit_log_occurred_at_not_null NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer CONSTRAINT audit_log_workspace_id_not_null NOT NULL,
    CONSTRAINT audit_log_op_check CHECK ((op = ANY (ARRAY['I'::bpchar, 'U'::bpchar, 'D'::bpchar])))
);

ALTER TABLE ONLY public.audit_log_default FORCE ROW LEVEL SECURITY;


ALTER TABLE public.audit_log_default OWNER TO nosdesk_admin;

--
-- Name: backup_jobs; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.backup_jobs (
    id uuid DEFAULT public.uuid_generate_v7() NOT NULL,
    job_type character varying(20) NOT NULL,
    status character varying(20) DEFAULT 'pending'::character varying NOT NULL,
    include_sensitive boolean DEFAULT false NOT NULL,
    file_path text,
    file_size bigint,
    error_message text,
    created_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    completed_at timestamp with time zone,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.backup_jobs FORCE ROW LEVEL SECURITY;


ALTER TABLE public.backup_jobs OWNER TO nosdesk_admin;

--
-- Name: bug_reports; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.bug_reports (
    id bigint NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    user_uuid uuid,
    session_id uuid NOT NULL,
    description text NOT NULL,
    url text NOT NULL,
    breadcrumbs jsonb DEFAULT '[]'::jsonb NOT NULL,
    build_sha character varying(64) NOT NULL,
    user_agent text,
    viewport jsonb,
    occurred_at timestamp with time zone NOT NULL,
    received_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT bug_reports_breadcrumbs_size CHECK ((octet_length((breadcrumbs)::text) < 16384)),
    CONSTRAINT bug_reports_description_size CHECK ((octet_length(description) < 4096))
);

ALTER TABLE ONLY public.bug_reports FORCE ROW LEVEL SECURITY;


ALTER TABLE public.bug_reports OWNER TO nosdesk_admin;

--
-- Name: bug_reports_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.bug_reports_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.bug_reports_id_seq OWNER TO nosdesk_admin;

--
-- Name: bug_reports_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.bug_reports_id_seq OWNED BY public.bug_reports.id;


--
-- Name: canned_response_insertions; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.canned_response_insertions (
    id bigint NOT NULL,
    canned_response_id integer NOT NULL,
    user_uuid uuid,
    ticket_id integer,
    inserted_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer NOT NULL
);

ALTER TABLE ONLY public.canned_response_insertions FORCE ROW LEVEL SECURITY;


ALTER TABLE public.canned_response_insertions OWNER TO nosdesk_admin;

--
-- Name: canned_response_insertions_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.canned_response_insertions_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.canned_response_insertions_id_seq OWNER TO nosdesk_admin;

--
-- Name: canned_response_insertions_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.canned_response_insertions_id_seq OWNED BY public.canned_response_insertions.id;


--
-- Name: canned_responses; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.canned_responses (
    id integer NOT NULL,
    title character varying(255) NOT NULL,
    body text NOT NULL,
    created_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.canned_responses FORCE ROW LEVEL SECURITY;


ALTER TABLE public.canned_responses OWNER TO nosdesk_admin;

--
-- Name: canned_responses_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.canned_responses_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.canned_responses_id_seq OWNER TO nosdesk_admin;

--
-- Name: canned_responses_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.canned_responses_id_seq OWNED BY public.canned_responses.id;


--
-- Name: category_group_visibility; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.category_group_visibility (
    category_id integer NOT NULL,
    group_id integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.category_group_visibility FORCE ROW LEVEL SECURITY;


ALTER TABLE public.category_group_visibility OWNER TO nosdesk_admin;

--
-- Name: channel_credentials; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.channel_credentials (
    id integer NOT NULL,
    channel_id integer NOT NULL,
    credential_type character varying(64) NOT NULL,
    expires_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    encrypted_value bytea NOT NULL,
    encrypted_kek_id smallint NOT NULL
);

ALTER TABLE ONLY public.channel_credentials FORCE ROW LEVEL SECURITY;


ALTER TABLE public.channel_credentials OWNER TO nosdesk_admin;

--
-- Name: channel_credentials_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.channel_credentials_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.channel_credentials_id_seq OWNER TO nosdesk_admin;

--
-- Name: channel_credentials_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.channel_credentials_id_seq OWNED BY public.channel_credentials.id;


--
-- Name: channel_messages; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.channel_messages (
    id bigint NOT NULL,
    channel_id integer NOT NULL,
    external_id character varying(998) NOT NULL,
    direction character varying(16) NOT NULL,
    ticket_id integer,
    comment_id integer,
    in_reply_to character varying(998),
    from_address character varying(320),
    author_user_uuid uuid,
    raw_metadata jsonb,
    received_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT channel_messages_direction_check CHECK (((direction)::text = ANY ((ARRAY['inbound'::character varying, 'outbound'::character varying])::text[])))
);

ALTER TABLE ONLY public.channel_messages FORCE ROW LEVEL SECURITY;


ALTER TABLE public.channel_messages OWNER TO nosdesk_admin;

--
-- Name: channel_messages_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.channel_messages_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.channel_messages_id_seq OWNER TO nosdesk_admin;

--
-- Name: channel_messages_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.channel_messages_id_seq OWNED BY public.channel_messages.id;


--
-- Name: channels; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.channels (
    id integer NOT NULL,
    provider character varying(64) NOT NULL,
    name character varying(255) NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    config jsonb NOT NULL,
    runtime_state jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    last_polled_at timestamp with time zone,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.channels FORCE ROW LEVEL SECURITY;


ALTER TABLE public.channels OWNER TO nosdesk_admin;

--
-- Name: channels_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.channels_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.channels_id_seq OWNER TO nosdesk_admin;

--
-- Name: channels_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.channels_id_seq OWNED BY public.channels.id;


--
-- Name: comments; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.comments (
    id integer NOT NULL,
    content text NOT NULL,
    ticket_id integer NOT NULL,
    user_uuid uuid NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    is_edited boolean DEFAULT false NOT NULL,
    edit_count integer DEFAULT 0 NOT NULL,
    channel_metadata jsonb,
    is_internal boolean DEFAULT false NOT NULL,
    deleted_at timestamp with time zone,
    content_format character varying(16) DEFAULT 'html'::character varying NOT NULL,
    body_text text,
    body_html text,
    new_content text,
    quoted_content text,
    raw_source_uri text,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    render_kind character varying(16)
);

ALTER TABLE ONLY public.comments FORCE ROW LEVEL SECURITY;


ALTER TABLE public.comments OWNER TO nosdesk_admin;

--
-- Name: comments_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.comments_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.comments_id_seq OWNER TO nosdesk_admin;

--
-- Name: comments_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.comments_id_seq OWNED BY public.comments.id;


--
-- Name: csp_reports; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.csp_reports (
    id bigint NOT NULL,
    dedup_hash character(64) NOT NULL,
    effective_directive character varying(64) NOT NULL,
    blocked_uri text,
    source_file text,
    line_number integer,
    column_number integer,
    document_uri text NOT NULL,
    referrer text,
    violated_directive character varying(64),
    original_policy text,
    disposition character varying(16) NOT NULL,
    user_agent text,
    user_uuid uuid,
    occurrence_count integer DEFAULT 1 NOT NULL,
    first_seen_at timestamp with time zone DEFAULT now() NOT NULL,
    last_seen_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.csp_reports FORCE ROW LEVEL SECURITY;


ALTER TABLE public.csp_reports OWNER TO nosdesk_admin;

--
-- Name: csp_reports_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.csp_reports_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.csp_reports_id_seq OWNER TO nosdesk_admin;

--
-- Name: csp_reports_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.csp_reports_id_seq OWNED BY public.csp_reports.id;


--
-- Name: cycle_tickets; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.cycle_tickets (
    cycle_id integer NOT NULL,
    ticket_id integer NOT NULL,
    added_at timestamp with time zone DEFAULT now() NOT NULL,
    added_by uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.cycle_tickets FORCE ROW LEVEL SECURITY;


ALTER TABLE public.cycle_tickets OWNER TO nosdesk_admin;

--
-- Name: cycles; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.cycles (
    id integer NOT NULL,
    uuid uuid DEFAULT public.uuid_generate_v7() NOT NULL,
    project_id integer NOT NULL,
    name character varying(120) NOT NULL,
    start_at timestamp with time zone,
    end_at timestamp with time zone,
    state character varying(20) DEFAULT 'planned'::character varying NOT NULL,
    completion_snapshot jsonb,
    completed_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    archived_at timestamp with time zone,
    created_by uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT cycles_completed_snapshot CHECK (((((state)::text = 'completed'::text) AND (completion_snapshot IS NOT NULL) AND (completed_at IS NOT NULL)) OR (((state)::text <> 'completed'::text) AND (completed_at IS NULL)))),
    CONSTRAINT cycles_state_check CHECK (((state)::text = ANY ((ARRAY['planned'::character varying, 'active'::character varying, 'completed'::character varying])::text[])))
);

ALTER TABLE ONLY public.cycles FORCE ROW LEVEL SECURITY;


ALTER TABLE public.cycles OWNER TO nosdesk_admin;

--
-- Name: cycles_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.cycles_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.cycles_id_seq OWNER TO nosdesk_admin;

--
-- Name: cycles_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.cycles_id_seq OWNED BY public.cycles.id;


--
-- Name: devices_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.devices_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.devices_id_seq OWNER TO nosdesk_admin;

--
-- Name: devices_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.devices_id_seq OWNED BY public.assets.id;


--
-- Name: documentation_collection_pages; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.documentation_collection_pages (
    collection_id integer NOT NULL,
    page_id integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.documentation_collection_pages FORCE ROW LEVEL SECURITY;


ALTER TABLE public.documentation_collection_pages OWNER TO nosdesk_admin;

--
-- Name: documentation_collection_visibility; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.documentation_collection_visibility (
    collection_id integer NOT NULL,
    group_id integer,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    id integer NOT NULL,
    user_uuid uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT chk_collection_vis_one_principal CHECK ((((group_id IS NOT NULL) AND (user_uuid IS NULL)) OR ((group_id IS NULL) AND (user_uuid IS NOT NULL))))
);

ALTER TABLE ONLY public.documentation_collection_visibility FORCE ROW LEVEL SECURITY;


ALTER TABLE public.documentation_collection_visibility OWNER TO nosdesk_admin;

--
-- Name: documentation_collection_visibility_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.documentation_collection_visibility_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.documentation_collection_visibility_id_seq OWNER TO nosdesk_admin;

--
-- Name: documentation_collection_visibility_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.documentation_collection_visibility_id_seq OWNED BY public.documentation_collection_visibility.id;


--
-- Name: documentation_collections; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.documentation_collections (
    id integer NOT NULL,
    uuid uuid DEFAULT gen_random_uuid() NOT NULL,
    name character varying(255) NOT NULL,
    slug character varying(255) NOT NULL,
    description text,
    icon character varying(50),
    color character varying(7),
    is_system boolean DEFAULT false NOT NULL,
    created_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    display_order integer DEFAULT 0 NOT NULL,
    description_yjs bytea,
    description_state_vector bytea,
    description_text text,
    hide_titles_from_non_members boolean DEFAULT false NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.documentation_collections FORCE ROW LEVEL SECURITY;


ALTER TABLE public.documentation_collections OWNER TO nosdesk_admin;

--
-- Name: documentation_collections_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.documentation_collections_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.documentation_collections_id_seq OWNER TO nosdesk_admin;

--
-- Name: documentation_collections_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.documentation_collections_id_seq OWNED BY public.documentation_collections.id;


--
-- Name: documentation_page_embeddings; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.documentation_page_embeddings (
    source_page_id integer NOT NULL,
    target_page_id integer NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.documentation_page_embeddings FORCE ROW LEVEL SECURITY;


ALTER TABLE public.documentation_page_embeddings OWNER TO nosdesk_admin;

--
-- Name: documentation_page_tickets; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.documentation_page_tickets (
    page_id integer NOT NULL,
    ticket_id integer NOT NULL,
    link_type character varying(32) DEFAULT 'references'::character varying NOT NULL,
    created_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT documentation_page_tickets_link_type_check CHECK (((link_type)::text = ANY ((ARRAY['resolves'::character varying, 'references'::character varying])::text[])))
);

ALTER TABLE ONLY public.documentation_page_tickets FORCE ROW LEVEL SECURITY;


ALTER TABLE public.documentation_page_tickets OWNER TO nosdesk_admin;

--
-- Name: documentation_page_visibility; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.documentation_page_visibility (
    page_id integer NOT NULL,
    group_id integer,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    id integer NOT NULL,
    user_uuid uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT chk_page_vis_one_principal CHECK ((((group_id IS NOT NULL) AND (user_uuid IS NULL)) OR ((group_id IS NULL) AND (user_uuid IS NOT NULL))))
);

ALTER TABLE ONLY public.documentation_page_visibility FORCE ROW LEVEL SECURITY;


ALTER TABLE public.documentation_page_visibility OWNER TO nosdesk_admin;

--
-- Name: documentation_page_visibility_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.documentation_page_visibility_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.documentation_page_visibility_id_seq OWNER TO nosdesk_admin;

--
-- Name: documentation_page_visibility_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.documentation_page_visibility_id_seq OWNED BY public.documentation_page_visibility.id;


--
-- Name: documentation_pages; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.documentation_pages (
    id integer NOT NULL,
    uuid uuid DEFAULT public.uuid_generate_v7() NOT NULL,
    title character varying(255) NOT NULL,
    slug character varying(255) NOT NULL,
    icon character varying(50),
    cover_image character varying(2048),
    status public.documentation_status DEFAULT 'draft'::public.documentation_status NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid NOT NULL,
    last_edited_by uuid NOT NULL,
    parent_id integer,
    display_order integer DEFAULT 0,
    is_public boolean DEFAULT false NOT NULL,
    is_template boolean DEFAULT false NOT NULL,
    archived_at timestamp with time zone,
    yjs_state_vector bytea,
    yjs_document bytea,
    yjs_client_id bigint,
    has_unsaved_changes boolean DEFAULT false NOT NULL,
    deleted_at timestamp with time zone,
    verified_by uuid,
    verified_at timestamp with time zone,
    verify_interval_days integer,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.documentation_pages FORCE ROW LEVEL SECURITY;


ALTER TABLE public.documentation_pages OWNER TO nosdesk_admin;

--
-- Name: documentation_pages_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.documentation_pages_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.documentation_pages_id_seq OWNER TO nosdesk_admin;

--
-- Name: documentation_pages_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.documentation_pages_id_seq OWNED BY public.documentation_pages.id;


--
-- Name: documentation_revisions; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.documentation_revisions (
    id integer NOT NULL,
    page_id integer NOT NULL,
    revision_number integer NOT NULL,
    title character varying(255) NOT NULL,
    yjs_document_snapshot bytea NOT NULL,
    yjs_state_vector bytea NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid NOT NULL,
    change_summary text,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.documentation_revisions FORCE ROW LEVEL SECURITY;


ALTER TABLE public.documentation_revisions OWNER TO nosdesk_admin;

--
-- Name: documentation_revisions_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.documentation_revisions_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.documentation_revisions_id_seq OWNER TO nosdesk_admin;

--
-- Name: documentation_revisions_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.documentation_revisions_id_seq OWNED BY public.documentation_revisions.id;


--
-- Name: documentation_starred_pages; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.documentation_starred_pages (
    id integer NOT NULL,
    user_uuid uuid NOT NULL,
    page_id integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.documentation_starred_pages FORCE ROW LEVEL SECURITY;


ALTER TABLE public.documentation_starred_pages OWNER TO nosdesk_admin;

--
-- Name: documentation_starred_pages_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.documentation_starred_pages_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.documentation_starred_pages_id_seq OWNER TO nosdesk_admin;

--
-- Name: documentation_starred_pages_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.documentation_starred_pages_id_seq OWNED BY public.documentation_starred_pages.id;


--
-- Name: documentation_subscriptions; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.documentation_subscriptions (
    id integer NOT NULL,
    user_uuid uuid NOT NULL,
    page_id integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.documentation_subscriptions FORCE ROW LEVEL SECURITY;


ALTER TABLE public.documentation_subscriptions OWNER TO nosdesk_admin;

--
-- Name: documentation_subscriptions_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.documentation_subscriptions_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.documentation_subscriptions_id_seq OWNER TO nosdesk_admin;

--
-- Name: documentation_subscriptions_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.documentation_subscriptions_id_seq OWNED BY public.documentation_subscriptions.id;


--
-- Name: email_suppressions; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.email_suppressions (
    email text NOT NULL,
    reason text NOT NULL,
    bounce_diagnostic text,
    bounce_count integer DEFAULT 1 NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    last_seen_at timestamp with time zone DEFAULT now() NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL
);


ALTER TABLE public.email_suppressions OWNER TO nosdesk;

--
-- Name: group_includes; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.group_includes (
    parent_group_id integer NOT NULL,
    child_group_id integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT group_includes_check CHECK ((parent_group_id <> child_group_id))
);

ALTER TABLE ONLY public.group_includes FORCE ROW LEVEL SECURITY;


ALTER TABLE public.group_includes OWNER TO nosdesk_admin;

--
-- Name: groups; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.groups (
    id integer NOT NULL,
    uuid uuid DEFAULT gen_random_uuid() NOT NULL,
    name character varying(255) NOT NULL,
    description text,
    color character varying(7),
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    external_id character varying(255),
    external_source character varying(50),
    group_type character varying(50),
    mail_enabled boolean DEFAULT false NOT NULL,
    security_enabled boolean DEFAULT false NOT NULL,
    last_synced_at timestamp with time zone,
    sync_enabled boolean DEFAULT true NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.groups FORCE ROW LEVEL SECURITY;


ALTER TABLE public.groups OWNER TO nosdesk_admin;

--
-- Name: groups_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.groups_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.groups_id_seq OWNER TO nosdesk_admin;

--
-- Name: groups_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.groups_id_seq OWNED BY public.groups.id;


--
-- Name: idempotency_keys; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.idempotency_keys (
    key text NOT NULL,
    response_body jsonb NOT NULL,
    response_status smallint NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.idempotency_keys OWNER TO nosdesk;

--
-- Name: import_jobs; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.import_jobs (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    job_type character varying(32) NOT NULL,
    status character varying(32) DEFAULT 'parsed'::character varying NOT NULL,
    filename character varying(255) NOT NULL,
    file_path text NOT NULL,
    created_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    completed_at timestamp with time zone,
    summary jsonb,
    records_committed integer,
    error_message text,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT import_jobs_job_type_check CHECK (((job_type)::text = ANY ((ARRAY['assets'::character varying, 'users'::character varying, 'tickets'::character varying])::text[]))),
    CONSTRAINT import_jobs_status_check CHECK (((status)::text = ANY ((ARRAY['parsed'::character varying, 'dry_run_done'::character varying, 'committing'::character varying, 'done'::character varying, 'failed'::character varying])::text[])))
);

ALTER TABLE ONLY public.import_jobs FORCE ROW LEVEL SECURITY;


ALTER TABLE public.import_jobs OWNER TO nosdesk_admin;

--
-- Name: knowledge_gap_signals; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.knowledge_gap_signals (
    id bigint NOT NULL,
    gap_id bigint NOT NULL,
    signal_type character varying(32) NOT NULL,
    source_kind character varying(32) NOT NULL,
    source_ref text NOT NULL,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    confidence integer DEFAULT 50 NOT NULL,
    detected_by uuid,
    detected_at timestamp with time zone DEFAULT now() NOT NULL,
    dismissed_at timestamp with time zone,
    dismissed_by uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT knowledge_gap_signals_confidence_check CHECK (((confidence >= 0) AND (confidence <= 100))),
    CONSTRAINT knowledge_gap_signals_signal_type_check CHECK (((signal_type)::text = ANY ((ARRAY['manual_flag'::character varying, 'ticket_cluster'::character varying, 'failed_search'::character varying, 'stale_doc'::character varying, 'ai_suggested'::character varying])::text[])))
);

ALTER TABLE ONLY public.knowledge_gap_signals FORCE ROW LEVEL SECURITY;


ALTER TABLE public.knowledge_gap_signals OWNER TO nosdesk_admin;

--
-- Name: knowledge_gap_signals_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.knowledge_gap_signals_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.knowledge_gap_signals_id_seq OWNER TO nosdesk_admin;

--
-- Name: knowledge_gap_signals_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.knowledge_gap_signals_id_seq OWNED BY public.knowledge_gap_signals.id;


--
-- Name: knowledge_gaps; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.knowledge_gaps (
    id bigint NOT NULL,
    title text NOT NULL,
    description text,
    status character varying(32) DEFAULT 'open'::character varying NOT NULL,
    assignee_uuid uuid,
    resolved_page_id integer,
    evidence_count integer DEFAULT 0 NOT NULL,
    last_evidence_at timestamp with time zone,
    impact_score integer DEFAULT 0 NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    dismissed_at timestamp with time zone,
    dismissed_by uuid,
    resolved_at timestamp with time zone,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT knowledge_gaps_status_check CHECK (((status)::text = ANY ((ARRAY['open'::character varying, 'drafting'::character varying, 'resolved'::character varying, 'dismissed'::character varying])::text[])))
);

ALTER TABLE ONLY public.knowledge_gaps FORCE ROW LEVEL SECURITY;


ALTER TABLE public.knowledge_gaps OWNER TO nosdesk_admin;

--
-- Name: knowledge_gaps_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.knowledge_gaps_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.knowledge_gaps_id_seq OWNER TO nosdesk_admin;

--
-- Name: knowledge_gaps_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.knowledge_gaps_id_seq OWNED BY public.knowledge_gaps.id;


--
-- Name: linked_tickets; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.linked_tickets (
    ticket_id integer NOT NULL,
    linked_ticket_id integer NOT NULL,
    relation_type character varying(50) DEFAULT 'related'::character varying CONSTRAINT linked_tickets_link_type_not_null NOT NULL,
    description text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT linked_tickets_relation_type_check CHECK (((relation_type)::text = ANY ((ARRAY['blocks'::character varying, 'blocked_by'::character varying, 'related'::character varying, 'duplicate_of'::character varying])::text[]))),
    CONSTRAINT no_self_link CHECK ((ticket_id <> linked_ticket_id))
);

ALTER TABLE ONLY public.linked_tickets FORCE ROW LEVEL SECURITY;


ALTER TABLE public.linked_tickets OWNER TO nosdesk_admin;

--
-- Name: notification_preferences; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.notification_preferences (
    id integer NOT NULL,
    user_uuid uuid NOT NULL,
    notification_type_id integer NOT NULL,
    channel character varying(20) NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.notification_preferences FORCE ROW LEVEL SECURITY;


ALTER TABLE public.notification_preferences OWNER TO nosdesk_admin;

--
-- Name: notification_preferences_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.notification_preferences_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.notification_preferences_id_seq OWNER TO nosdesk_admin;

--
-- Name: notification_preferences_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.notification_preferences_id_seq OWNED BY public.notification_preferences.id;


--
-- Name: notification_rate_limits; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.notification_rate_limits (
    id integer NOT NULL,
    user_uuid uuid NOT NULL,
    notification_type_id integer NOT NULL,
    entity_type character varying(50) NOT NULL,
    entity_id integer NOT NULL,
    last_notified_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.notification_rate_limits OWNER TO nosdesk;

--
-- Name: notification_rate_limits_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk
--

CREATE SEQUENCE public.notification_rate_limits_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.notification_rate_limits_id_seq OWNER TO nosdesk;

--
-- Name: notification_rate_limits_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk
--

ALTER SEQUENCE public.notification_rate_limits_id_seq OWNED BY public.notification_rate_limits.id;


--
-- Name: notification_types; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.notification_types (
    id integer NOT NULL,
    code character varying(50) NOT NULL,
    name character varying(100) NOT NULL,
    description text,
    category character varying(50) NOT NULL,
    default_channels jsonb DEFAULT '["in_app"]'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.notification_types OWNER TO nosdesk;

--
-- Name: notification_types_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk
--

CREATE SEQUENCE public.notification_types_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.notification_types_id_seq OWNER TO nosdesk;

--
-- Name: notification_types_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk
--

ALTER SEQUENCE public.notification_types_id_seq OWNED BY public.notification_types.id;


--
-- Name: notifications; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.notifications (
    id integer NOT NULL,
    uuid uuid DEFAULT gen_random_uuid() NOT NULL,
    user_uuid uuid NOT NULL,
    notification_type_id integer NOT NULL,
    entity_type character varying(50) NOT NULL,
    entity_id integer NOT NULL,
    title character varying(255) NOT NULL,
    body text,
    metadata jsonb,
    channels_delivered jsonb DEFAULT '[]'::jsonb NOT NULL,
    is_read boolean DEFAULT false NOT NULL,
    read_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.notifications FORCE ROW LEVEL SECURITY;


ALTER TABLE public.notifications OWNER TO nosdesk_admin;

--
-- Name: notifications_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.notifications_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.notifications_id_seq OWNER TO nosdesk_admin;

--
-- Name: notifications_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.notifications_id_seq OWNED BY public.notifications.id;


--
-- Name: outbound_emails; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.outbound_emails (
    id bigint NOT NULL,
    channel_id integer,
    ticket_id integer,
    comment_id integer,
    recipient text NOT NULL,
    subject text NOT NULL,
    body_text text NOT NULL,
    body_html text,
    message_id text NOT NULL,
    in_reply_to text,
    references_list text[] DEFAULT '{}'::text[] NOT NULL,
    headers_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    attempts integer DEFAULT 0 NOT NULL,
    last_error text,
    last_smtp_code integer,
    next_attempt_at timestamp with time zone DEFAULT now() NOT NULL,
    lease_token uuid,
    lease_expires_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    sent_at timestamp with time zone,
    failed_at timestamp with time zone,
    correlation_id uuid,
    bounced_at timestamp with time zone,
    bounce_recipient text,
    bounce_diagnostic text,
    idempotency_key text,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT outbound_emails_status_chk CHECK ((status = ANY (ARRAY['pending'::text, 'sending'::text, 'sent'::text, 'failed'::text, 'dead'::text, 'suppressed'::text])))
);

ALTER TABLE ONLY public.outbound_emails FORCE ROW LEVEL SECURITY;


ALTER TABLE public.outbound_emails OWNER TO nosdesk_admin;

--
-- Name: outbound_emails_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.outbound_emails_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.outbound_emails_id_seq OWNER TO nosdesk_admin;

--
-- Name: outbound_emails_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.outbound_emails_id_seq OWNED BY public.outbound_emails.id;


--
-- Name: passkey_credentials; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.passkey_credentials (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_uuid uuid NOT NULL,
    credential_id text NOT NULL,
    name character varying(100) NOT NULL,
    credential jsonb NOT NULL,
    transports text[] DEFAULT '{}'::text[] NOT NULL,
    backup_eligible boolean DEFAULT false NOT NULL,
    backup_state boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    last_used_at timestamp with time zone,
    sign_count bigint DEFAULT 0 NOT NULL,
    backup_state_changed_at timestamp with time zone
);


ALTER TABLE public.passkey_credentials OWNER TO nosdesk;

--
-- Name: plugin_activity; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.plugin_activity (
    id integer NOT NULL,
    uuid uuid DEFAULT public.uuid_generate_v7() NOT NULL,
    plugin_id integer NOT NULL,
    action character varying(100) NOT NULL,
    details jsonb,
    user_uuid uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.plugin_activity FORCE ROW LEVEL SECURITY;


ALTER TABLE public.plugin_activity OWNER TO nosdesk_admin;

--
-- Name: plugin_activity_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.plugin_activity_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.plugin_activity_id_seq OWNER TO nosdesk_admin;

--
-- Name: plugin_activity_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.plugin_activity_id_seq OWNED BY public.plugin_activity.id;


--
-- Name: plugin_collection_rows; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.plugin_collection_rows (
    id integer NOT NULL,
    uuid uuid DEFAULT public.uuid_generate_v7() NOT NULL,
    plugin_id integer NOT NULL,
    schema_id integer NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.plugin_collection_rows FORCE ROW LEVEL SECURITY;


ALTER TABLE public.plugin_collection_rows OWNER TO nosdesk_admin;

--
-- Name: plugin_collection_rows_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.plugin_collection_rows_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.plugin_collection_rows_id_seq OWNER TO nosdesk_admin;

--
-- Name: plugin_collection_rows_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.plugin_collection_rows_id_seq OWNED BY public.plugin_collection_rows.id;


--
-- Name: plugin_collection_schemas; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.plugin_collection_schemas (
    id integer NOT NULL,
    uuid uuid DEFAULT public.uuid_generate_v7() NOT NULL,
    plugin_id integer NOT NULL,
    collection_name character varying(100) NOT NULL,
    schema jsonb NOT NULL,
    version integer DEFAULT 1 NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.plugin_collection_schemas FORCE ROW LEVEL SECURITY;


ALTER TABLE public.plugin_collection_schemas OWNER TO nosdesk_admin;

--
-- Name: plugin_collection_schemas_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.plugin_collection_schemas_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.plugin_collection_schemas_id_seq OWNER TO nosdesk_admin;

--
-- Name: plugin_collection_schemas_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.plugin_collection_schemas_id_seq OWNED BY public.plugin_collection_schemas.id;


--
-- Name: plugin_data; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.plugin_data (
    id integer NOT NULL,
    uuid uuid DEFAULT public.uuid_generate_v7() NOT NULL,
    plugin_id integer NOT NULL,
    data_type character varying(20) NOT NULL,
    key character varying(255) NOT NULL,
    value jsonb,
    is_secret boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT plugin_data_data_type_check CHECK (((data_type)::text = ANY ((ARRAY['setting'::character varying, 'storage'::character varying])::text[])))
);

ALTER TABLE ONLY public.plugin_data FORCE ROW LEVEL SECURITY;


ALTER TABLE public.plugin_data OWNER TO nosdesk_admin;

--
-- Name: plugin_data_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.plugin_data_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.plugin_data_id_seq OWNER TO nosdesk_admin;

--
-- Name: plugin_data_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.plugin_data_id_seq OWNED BY public.plugin_data.id;


--
-- Name: plugin_local_signing_key; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.plugin_local_signing_key (
    id integer NOT NULL,
    pubkey text NOT NULL,
    encrypted_sk bytea NOT NULL,
    fingerprint character varying(64) NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    encrypted_sk_kek_id smallint NOT NULL,
    CONSTRAINT plugin_local_signing_key_id_check CHECK ((id = 1))
);


ALTER TABLE public.plugin_local_signing_key OWNER TO nosdesk;

--
-- Name: plugin_registry_state; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.plugin_registry_state (
    id integer NOT NULL,
    publishers_version bigint DEFAULT 0 NOT NULL,
    index_version bigint DEFAULT 0 NOT NULL,
    last_fetched_at timestamp with time zone,
    last_fetch_error text,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT plugin_registry_state_id_check CHECK ((id = 1))
);


ALTER TABLE public.plugin_registry_state OWNER TO nosdesk;

--
-- Name: plugin_trusted_publishers; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.plugin_trusted_publishers (
    id integer NOT NULL,
    pubkey text NOT NULL,
    display_name character varying(200) NOT NULL,
    tier character varying(32) NOT NULL,
    website text,
    added_at timestamp with time zone DEFAULT now() NOT NULL,
    revoked_at timestamp with time zone,
    CONSTRAINT plugin_trusted_publishers_tier_check CHECK (((tier)::text = ANY ((ARRAY['verified'::character varying, 'community'::character varying])::text[])))
);


ALTER TABLE public.plugin_trusted_publishers OWNER TO nosdesk;

--
-- Name: plugin_trusted_publishers_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk
--

CREATE SEQUENCE public.plugin_trusted_publishers_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.plugin_trusted_publishers_id_seq OWNER TO nosdesk;

--
-- Name: plugin_trusted_publishers_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk
--

ALTER SEQUENCE public.plugin_trusted_publishers_id_seq OWNED BY public.plugin_trusted_publishers.id;


--
-- Name: plugins; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.plugins (
    id integer NOT NULL,
    uuid uuid DEFAULT public.uuid_generate_v7() NOT NULL,
    name character varying(100) NOT NULL,
    display_name character varying(255) NOT NULL,
    version character varying(50) NOT NULL,
    description text,
    manifest jsonb NOT NULL,
    trust_level character varying(50) DEFAULT 'community'::character varying NOT NULL,
    installed_by uuid,
    installed_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    bundle_hash character varying(64),
    bundle_size integer,
    bundle_uploaded_at timestamp with time zone,
    source character varying(20) DEFAULT 'uploaded'::character varying NOT NULL,
    signer_pubkey text,
    signer_source character varying(32),
    signature_metadata jsonb,
    icon_svg bytea,
    state character varying(32) DEFAULT 'installed'::character varying NOT NULL,
    bundle_js bytea,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT plugins_state_check CHECK (((state)::text = ANY ((ARRAY['installed'::character varying, 'disabled'::character varying, 'quarantined'::character varying, 'uninstalled'::character varying])::text[])))
);

ALTER TABLE ONLY public.plugins FORCE ROW LEVEL SECURITY;


ALTER TABLE public.plugins OWNER TO nosdesk_admin;

--
-- Name: plugins_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.plugins_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.plugins_id_seq OWNER TO nosdesk_admin;

--
-- Name: plugins_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.plugins_id_seq OWNED BY public.plugins.id;


--
-- Name: project_tickets; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.project_tickets (
    project_id integer NOT NULL,
    ticket_id integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    display_order integer DEFAULT 0 NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.project_tickets FORCE ROW LEVEL SECURITY;


ALTER TABLE public.project_tickets OWNER TO nosdesk_admin;

--
-- Name: projects; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.projects (
    id integer NOT NULL,
    name character varying(255) NOT NULL,
    description text,
    status public.project_status DEFAULT 'active'::public.project_status NOT NULL,
    start_date date,
    end_date date,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    owner_uuid uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT projects_dates_valid CHECK (((end_date IS NULL) OR (start_date IS NULL) OR (end_date >= start_date)))
);

ALTER TABLE ONLY public.projects FORCE ROW LEVEL SECURITY;


ALTER TABLE public.projects OWNER TO nosdesk_admin;

--
-- Name: projects_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.projects_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.projects_id_seq OWNER TO nosdesk_admin;

--
-- Name: projects_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.projects_id_seq OWNED BY public.projects.id;


--
-- Name: refresh_tokens; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.refresh_tokens (
    id integer NOT NULL,
    token_hash character varying(64) NOT NULL,
    user_uuid uuid NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    revoked_at timestamp with time zone,
    session_id uuid,
    family_id uuid DEFAULT gen_random_uuid() NOT NULL,
    is_used boolean DEFAULT false NOT NULL,
    used_at timestamp with time zone,
    replaced_by_hash character varying(64),
    grace_expires_at timestamp with time zone
);


ALTER TABLE public.refresh_tokens OWNER TO nosdesk;

--
-- Name: refresh_tokens_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk
--

CREATE SEQUENCE public.refresh_tokens_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.refresh_tokens_id_seq OWNER TO nosdesk;

--
-- Name: refresh_tokens_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk
--

ALTER SEQUENCE public.refresh_tokens_id_seq OWNED BY public.refresh_tokens.id;


--
-- Name: reset_tokens; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.reset_tokens (
    token_hash character varying(64) NOT NULL,
    user_uuid uuid NOT NULL,
    token_type character varying(50) NOT NULL,
    ip_address inet,
    user_agent text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    used_at timestamp with time zone,
    is_used boolean DEFAULT false NOT NULL,
    metadata jsonb
);


ALTER TABLE public.reset_tokens OWNER TO nosdesk;

--
-- Name: rule_applications; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.rule_applications (
    id bigint NOT NULL,
    workspace_id integer NOT NULL,
    rule_id integer NOT NULL,
    rule_version integer NOT NULL,
    ticket_id integer NOT NULL,
    status public.rule_application_status NOT NULL,
    correlation_id uuid,
    actor_uuid uuid,
    actor_kind character varying(16) NOT NULL,
    originating_event_id uuid,
    originating_event_kind character varying(64),
    condition_evaluation jsonb,
    actions_taken jsonb,
    actions_skipped jsonb,
    failure_reason text,
    applied_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT rule_applications_actor_kind_valid CHECK (((actor_kind)::text = ANY ((ARRAY['user'::character varying, 'system'::character varying])::text[])))
);

ALTER TABLE ONLY public.rule_applications FORCE ROW LEVEL SECURITY;


ALTER TABLE public.rule_applications OWNER TO nosdesk_admin;

--
-- Name: rule_applications_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.rule_applications_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.rule_applications_id_seq OWNER TO nosdesk_admin;

--
-- Name: rule_applications_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.rule_applications_id_seq OWNED BY public.rule_applications.id;


--
-- Name: rule_versions; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.rule_versions (
    id integer NOT NULL,
    rule_id integer NOT NULL,
    workspace_id integer NOT NULL,
    version integer NOT NULL,
    name character varying(255) NOT NULL,
    description text,
    trigger_kind public.rule_trigger_kind NOT NULL,
    trigger_config jsonb NOT NULL,
    conditions jsonb NOT NULL,
    actions jsonb NOT NULL,
    state public.rule_state NOT NULL,
    priority integer NOT NULL,
    saved_by uuid,
    saved_at timestamp with time zone DEFAULT now() NOT NULL
);

ALTER TABLE ONLY public.rule_versions FORCE ROW LEVEL SECURITY;


ALTER TABLE public.rule_versions OWNER TO nosdesk_admin;

--
-- Name: rule_versions_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.rule_versions_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.rule_versions_id_seq OWNER TO nosdesk_admin;

--
-- Name: rule_versions_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.rule_versions_id_seq OWNED BY public.rule_versions.id;


--
-- Name: rules; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.rules (
    id integer NOT NULL,
    workspace_id integer NOT NULL,
    name character varying(255) NOT NULL,
    description text,
    trigger_kind public.rule_trigger_kind NOT NULL,
    trigger_config jsonb DEFAULT '{}'::jsonb NOT NULL,
    conditions jsonb DEFAULT '[]'::jsonb NOT NULL,
    actions jsonb NOT NULL,
    reads_set text[] DEFAULT '{}'::text[] NOT NULL,
    writes_set text[] DEFAULT '{}'::text[] NOT NULL,
    state public.rule_state DEFAULT 'draft'::public.rule_state NOT NULL,
    priority integer DEFAULT 100 NOT NULL,
    last_fired_at timestamp with time zone,
    fire_count integer DEFAULT 0 NOT NULL,
    created_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    archived_at timestamp with time zone,
    CONSTRAINT rules_actions_non_empty CHECK (((jsonb_typeof(actions) = 'array'::text) AND (jsonb_array_length(actions) > 0))),
    CONSTRAINT rules_manual_no_conditions CHECK (((trigger_kind <> 'manual'::public.rule_trigger_kind) OR (conditions = '[]'::jsonb)))
);

ALTER TABLE ONLY public.rules FORCE ROW LEVEL SECURITY;


ALTER TABLE public.rules OWNER TO nosdesk_admin;

--
-- Name: rules_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.rules_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.rules_id_seq OWNER TO nosdesk_admin;

--
-- Name: rules_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.rules_id_seq OWNED BY public.rules.id;


--
-- Name: saved_views; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.saved_views (
    id integer NOT NULL,
    uuid uuid DEFAULT public.uuid_generate_v7() NOT NULL,
    scope character varying(20) NOT NULL,
    scope_id text,
    name character varying(120) NOT NULL,
    shape jsonb NOT NULL,
    filter jsonb NOT NULL,
    created_by uuid NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    dataset character varying(20) NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    viz_type character varying(32) DEFAULT 'list'::character varying NOT NULL,
    viz_config jsonb DEFAULT '{}'::jsonb NOT NULL,
    CONSTRAINT saved_views_scope_check CHECK (((scope)::text = ANY ((ARRAY['workspace'::character varying, 'project'::character varying, 'private'::character varying])::text[]))),
    CONSTRAINT saved_views_scope_id_shape CHECK (((((scope)::text = 'workspace'::text) AND (scope_id IS NULL)) OR (((scope)::text = ANY ((ARRAY['project'::character varying, 'private'::character varying])::text[])) AND (scope_id IS NOT NULL)))),
    CONSTRAINT saved_views_viz_type_check CHECK (((viz_type)::text = ANY ((ARRAY['list'::character varying, 'kpi_tile'::character varying, 'line'::character varying, 'horizontal_bar'::character varying, 'heatmap'::character varying, 'leaderboard'::character varying, 'table'::character varying])::text[])))
);

ALTER TABLE ONLY public.saved_views FORCE ROW LEVEL SECURITY;


ALTER TABLE public.saved_views OWNER TO nosdesk_admin;

--
-- Name: saved_views_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.saved_views_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.saved_views_id_seq OWNER TO nosdesk_admin;

--
-- Name: saved_views_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.saved_views_id_seq OWNED BY public.saved_views.id;


--
-- Name: search_index_state; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.search_index_state (
    id integer NOT NULL,
    entity_type character varying(50) NOT NULL,
    last_indexed_at timestamp with time zone,
    index_version integer DEFAULT 1 NOT NULL,
    document_count integer DEFAULT 0 NOT NULL,
    last_error text,
    last_error_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.search_index_state OWNER TO nosdesk;

--
-- Name: search_index_state_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk
--

CREATE SEQUENCE public.search_index_state_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.search_index_state_id_seq OWNER TO nosdesk;

--
-- Name: search_index_state_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk
--

ALTER SEQUENCE public.search_index_state_id_seq OWNED BY public.search_index_state.id;


--
-- Name: search_query_log; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.search_query_log (
    id bigint NOT NULL,
    query_raw text NOT NULL,
    query_norm text NOT NULL,
    result_count integer NOT NULL,
    searched_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.search_query_log FORCE ROW LEVEL SECURITY;


ALTER TABLE public.search_query_log OWNER TO nosdesk_admin;

--
-- Name: search_query_log_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.search_query_log_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.search_query_log_id_seq OWNER TO nosdesk_admin;

--
-- Name: search_query_log_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.search_query_log_id_seq OWNED BY public.search_query_log.id;


--
-- Name: security_events; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.security_events (
    id integer NOT NULL,
    user_uuid uuid,
    event_type character varying(50) NOT NULL,
    ip_address inet,
    user_agent text,
    location character varying(255),
    details jsonb,
    severity character varying(20) DEFAULT 'info'::character varying NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    session_id integer
);


ALTER TABLE public.security_events OWNER TO nosdesk;

--
-- Name: security_events_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk
--

CREATE SEQUENCE public.security_events_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.security_events_id_seq OWNER TO nosdesk;

--
-- Name: security_events_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk
--

ALTER SEQUENCE public.security_events_id_seq OWNED BY public.security_events.id;


--
-- Name: site_settings; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.site_settings (
    id integer DEFAULT 1 NOT NULL,
    app_name character varying(255) DEFAULT 'Nosdesk'::character varying NOT NULL,
    logo_url character varying(2048),
    logo_light_url character varying(2048),
    favicon_url character varying(2048),
    primary_color character varying(7),
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_by uuid,
    guest_tickets_enabled boolean DEFAULT false NOT NULL,
    guest_public_docs_enabled boolean DEFAULT false NOT NULL,
    guest_kb_search_enabled boolean DEFAULT false NOT NULL,
    guest_ticket_lookup_enabled boolean DEFAULT false NOT NULL,
    guest_help_page_enabled boolean DEFAULT false NOT NULL,
    guest_ticket_default_priority character varying(32),
    guest_ticket_rate_limit_per_hour integer DEFAULT 5 NOT NULL,
    guest_ticket_email_verification boolean DEFAULT true NOT NULL,
    guest_ticket_attachments_enabled boolean DEFAULT true NOT NULL,
    guest_ticket_intro_message text,
    channel_auto_ack_enabled boolean DEFAULT true NOT NULL,
    channel_auto_ack_template text,
    feature_flags jsonb DEFAULT '{}'::jsonb NOT NULL,
    default_locale text DEFAULT 'en-US'::text NOT NULL,
    default_timezone text DEFAULT 'UTC'::text NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    signature_default text,
    CONSTRAINT site_settings_id_check CHECK ((id = 1))
);

ALTER TABLE ONLY public.site_settings FORCE ROW LEVEL SECURITY;


ALTER TABLE public.site_settings OWNER TO nosdesk_admin;

--
-- Name: sla_policies; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.sla_policies (
    id integer NOT NULL,
    name character varying(120) NOT NULL,
    target_response_minutes integer,
    target_resolution_minutes integer,
    working_calendar_id integer,
    priority_filter character varying(20),
    category_id_filter integer,
    is_default boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    assignee_group_id_filter integer
);

ALTER TABLE ONLY public.sla_policies FORCE ROW LEVEL SECURITY;


ALTER TABLE public.sla_policies OWNER TO nosdesk_admin;

--
-- Name: sla_policies_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.sla_policies_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.sla_policies_id_seq OWNER TO nosdesk_admin;

--
-- Name: sla_policies_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.sla_policies_id_seq OWNED BY public.sla_policies.id;


--
-- Name: sync_actions; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.sync_actions (
    sync_id bigint NOT NULL,
    event_uuid uuid DEFAULT public.uuid_generate_v7() NOT NULL,
    aggregate public.sync_aggregate NOT NULL,
    aggregate_id text NOT NULL,
    op public.sync_op NOT NULL,
    event_type character varying(64) NOT NULL,
    schema_version smallint DEFAULT 1 NOT NULL,
    data jsonb NOT NULL,
    groups text[] NOT NULL,
    actor_uuid uuid,
    actor_kind character varying(16) DEFAULT 'user'::character varying NOT NULL,
    actor_ref text,
    correlation_id uuid,
    causation_id uuid,
    client_tx_id text,
    occurred_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    recorded_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
)
PARTITION BY RANGE (occurred_at);

ALTER TABLE ONLY public.sync_actions FORCE ROW LEVEL SECURITY;


ALTER TABLE public.sync_actions OWNER TO nosdesk_admin;

--
-- Name: sync_actions_sync_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.sync_actions_sync_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.sync_actions_sync_id_seq OWNER TO nosdesk_admin;

--
-- Name: sync_actions_sync_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.sync_actions_sync_id_seq OWNED BY public.sync_actions.sync_id;


--
-- Name: sync_actions_2026_05; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.sync_actions_2026_05 (
    sync_id bigint DEFAULT nextval('public.sync_actions_sync_id_seq'::regclass) CONSTRAINT sync_actions_sync_id_not_null NOT NULL,
    event_uuid uuid DEFAULT public.uuid_generate_v7() CONSTRAINT sync_actions_event_uuid_not_null NOT NULL,
    aggregate public.sync_aggregate CONSTRAINT sync_actions_aggregate_not_null NOT NULL,
    aggregate_id text CONSTRAINT sync_actions_aggregate_id_not_null NOT NULL,
    op public.sync_op CONSTRAINT sync_actions_op_not_null NOT NULL,
    event_type character varying(64) CONSTRAINT sync_actions_event_type_not_null NOT NULL,
    schema_version smallint DEFAULT 1 CONSTRAINT sync_actions_schema_version_not_null NOT NULL,
    data jsonb CONSTRAINT sync_actions_data_not_null NOT NULL,
    groups text[] CONSTRAINT sync_actions_groups_not_null NOT NULL,
    actor_uuid uuid,
    actor_kind character varying(16) DEFAULT 'user'::character varying CONSTRAINT sync_actions_actor_kind_not_null NOT NULL,
    actor_ref text,
    correlation_id uuid,
    causation_id uuid,
    client_tx_id text,
    occurred_at timestamp with time zone DEFAULT clock_timestamp() CONSTRAINT sync_actions_occurred_at_not_null NOT NULL,
    recorded_at timestamp with time zone DEFAULT clock_timestamp() CONSTRAINT sync_actions_recorded_at_not_null NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer CONSTRAINT sync_actions_workspace_id_not_null NOT NULL
);

ALTER TABLE ONLY public.sync_actions_2026_05 FORCE ROW LEVEL SECURITY;


ALTER TABLE public.sync_actions_2026_05 OWNER TO nosdesk_admin;

--
-- Name: sync_actions_2026_06; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.sync_actions_2026_06 (
    sync_id bigint DEFAULT nextval('public.sync_actions_sync_id_seq'::regclass) CONSTRAINT sync_actions_sync_id_not_null NOT NULL,
    event_uuid uuid DEFAULT public.uuid_generate_v7() CONSTRAINT sync_actions_event_uuid_not_null NOT NULL,
    aggregate public.sync_aggregate CONSTRAINT sync_actions_aggregate_not_null NOT NULL,
    aggregate_id text CONSTRAINT sync_actions_aggregate_id_not_null NOT NULL,
    op public.sync_op CONSTRAINT sync_actions_op_not_null NOT NULL,
    event_type character varying(64) CONSTRAINT sync_actions_event_type_not_null NOT NULL,
    schema_version smallint DEFAULT 1 CONSTRAINT sync_actions_schema_version_not_null NOT NULL,
    data jsonb CONSTRAINT sync_actions_data_not_null NOT NULL,
    groups text[] CONSTRAINT sync_actions_groups_not_null NOT NULL,
    actor_uuid uuid,
    actor_kind character varying(16) DEFAULT 'user'::character varying CONSTRAINT sync_actions_actor_kind_not_null NOT NULL,
    actor_ref text,
    correlation_id uuid,
    causation_id uuid,
    client_tx_id text,
    occurred_at timestamp with time zone DEFAULT clock_timestamp() CONSTRAINT sync_actions_occurred_at_not_null NOT NULL,
    recorded_at timestamp with time zone DEFAULT clock_timestamp() CONSTRAINT sync_actions_recorded_at_not_null NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer CONSTRAINT sync_actions_workspace_id_not_null NOT NULL
);

ALTER TABLE ONLY public.sync_actions_2026_06 FORCE ROW LEVEL SECURITY;


ALTER TABLE public.sync_actions_2026_06 OWNER TO nosdesk_admin;

--
-- Name: sync_actions_2026_07; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.sync_actions_2026_07 (
    sync_id bigint DEFAULT nextval('public.sync_actions_sync_id_seq'::regclass) CONSTRAINT sync_actions_sync_id_not_null NOT NULL,
    event_uuid uuid DEFAULT public.uuid_generate_v7() CONSTRAINT sync_actions_event_uuid_not_null NOT NULL,
    aggregate public.sync_aggregate CONSTRAINT sync_actions_aggregate_not_null NOT NULL,
    aggregate_id text CONSTRAINT sync_actions_aggregate_id_not_null NOT NULL,
    op public.sync_op CONSTRAINT sync_actions_op_not_null NOT NULL,
    event_type character varying(64) CONSTRAINT sync_actions_event_type_not_null NOT NULL,
    schema_version smallint DEFAULT 1 CONSTRAINT sync_actions_schema_version_not_null NOT NULL,
    data jsonb CONSTRAINT sync_actions_data_not_null NOT NULL,
    groups text[] CONSTRAINT sync_actions_groups_not_null NOT NULL,
    actor_uuid uuid,
    actor_kind character varying(16) DEFAULT 'user'::character varying CONSTRAINT sync_actions_actor_kind_not_null NOT NULL,
    actor_ref text,
    correlation_id uuid,
    causation_id uuid,
    client_tx_id text,
    occurred_at timestamp with time zone DEFAULT clock_timestamp() CONSTRAINT sync_actions_occurred_at_not_null NOT NULL,
    recorded_at timestamp with time zone DEFAULT clock_timestamp() CONSTRAINT sync_actions_recorded_at_not_null NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer CONSTRAINT sync_actions_workspace_id_not_null NOT NULL
);

ALTER TABLE ONLY public.sync_actions_2026_07 FORCE ROW LEVEL SECURITY;


ALTER TABLE public.sync_actions_2026_07 OWNER TO nosdesk_admin;

--
-- Name: sync_actions_2026_08; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.sync_actions_2026_08 (
    sync_id bigint DEFAULT nextval('public.sync_actions_sync_id_seq'::regclass) CONSTRAINT sync_actions_sync_id_not_null NOT NULL,
    event_uuid uuid DEFAULT public.uuid_generate_v7() CONSTRAINT sync_actions_event_uuid_not_null NOT NULL,
    aggregate public.sync_aggregate CONSTRAINT sync_actions_aggregate_not_null NOT NULL,
    aggregate_id text CONSTRAINT sync_actions_aggregate_id_not_null NOT NULL,
    op public.sync_op CONSTRAINT sync_actions_op_not_null NOT NULL,
    event_type character varying(64) CONSTRAINT sync_actions_event_type_not_null NOT NULL,
    schema_version smallint DEFAULT 1 CONSTRAINT sync_actions_schema_version_not_null NOT NULL,
    data jsonb CONSTRAINT sync_actions_data_not_null NOT NULL,
    groups text[] CONSTRAINT sync_actions_groups_not_null NOT NULL,
    actor_uuid uuid,
    actor_kind character varying(16) DEFAULT 'user'::character varying CONSTRAINT sync_actions_actor_kind_not_null NOT NULL,
    actor_ref text,
    correlation_id uuid,
    causation_id uuid,
    client_tx_id text,
    occurred_at timestamp with time zone DEFAULT clock_timestamp() CONSTRAINT sync_actions_occurred_at_not_null NOT NULL,
    recorded_at timestamp with time zone DEFAULT clock_timestamp() CONSTRAINT sync_actions_recorded_at_not_null NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer CONSTRAINT sync_actions_workspace_id_not_null NOT NULL
);

ALTER TABLE ONLY public.sync_actions_2026_08 FORCE ROW LEVEL SECURITY;


ALTER TABLE public.sync_actions_2026_08 OWNER TO nosdesk_admin;

--
-- Name: sync_actions_default; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.sync_actions_default (
    sync_id bigint DEFAULT nextval('public.sync_actions_sync_id_seq'::regclass) CONSTRAINT sync_actions_sync_id_not_null NOT NULL,
    event_uuid uuid DEFAULT public.uuid_generate_v7() CONSTRAINT sync_actions_event_uuid_not_null NOT NULL,
    aggregate public.sync_aggregate CONSTRAINT sync_actions_aggregate_not_null NOT NULL,
    aggregate_id text CONSTRAINT sync_actions_aggregate_id_not_null NOT NULL,
    op public.sync_op CONSTRAINT sync_actions_op_not_null NOT NULL,
    event_type character varying(64) CONSTRAINT sync_actions_event_type_not_null NOT NULL,
    schema_version smallint DEFAULT 1 CONSTRAINT sync_actions_schema_version_not_null NOT NULL,
    data jsonb CONSTRAINT sync_actions_data_not_null NOT NULL,
    groups text[] CONSTRAINT sync_actions_groups_not_null NOT NULL,
    actor_uuid uuid,
    actor_kind character varying(16) DEFAULT 'user'::character varying CONSTRAINT sync_actions_actor_kind_not_null NOT NULL,
    actor_ref text,
    correlation_id uuid,
    causation_id uuid,
    client_tx_id text,
    occurred_at timestamp with time zone DEFAULT clock_timestamp() CONSTRAINT sync_actions_occurred_at_not_null NOT NULL,
    recorded_at timestamp with time zone DEFAULT clock_timestamp() CONSTRAINT sync_actions_recorded_at_not_null NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer CONSTRAINT sync_actions_workspace_id_not_null NOT NULL
);

ALTER TABLE ONLY public.sync_actions_default FORCE ROW LEVEL SECURITY;


ALTER TABLE public.sync_actions_default OWNER TO nosdesk_admin;

--
-- Name: sync_delta_tokens; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.sync_delta_tokens (
    id integer NOT NULL,
    provider_type character varying(50) NOT NULL,
    entity_type character varying(50) NOT NULL,
    delta_link text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.sync_delta_tokens FORCE ROW LEVEL SECURITY;


ALTER TABLE public.sync_delta_tokens OWNER TO nosdesk_admin;

--
-- Name: sync_delta_tokens_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.sync_delta_tokens_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.sync_delta_tokens_id_seq OWNER TO nosdesk_admin;

--
-- Name: sync_delta_tokens_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.sync_delta_tokens_id_seq OWNED BY public.sync_delta_tokens.id;


--
-- Name: sync_history; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.sync_history (
    id integer NOT NULL,
    sync_type character varying(100) NOT NULL,
    status character varying(50) NOT NULL,
    started_at timestamp with time zone DEFAULT now() NOT NULL,
    completed_at timestamp with time zone,
    error_message text,
    records_processed integer DEFAULT 0,
    records_created integer DEFAULT 0,
    records_updated integer DEFAULT 0,
    records_failed integer DEFAULT 0,
    tenant_id character varying(255),
    initiated_by uuid,
    is_delta boolean DEFAULT true NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.sync_history FORCE ROW LEVEL SECURITY;


ALTER TABLE public.sync_history OWNER TO nosdesk_admin;

--
-- Name: sync_history_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.sync_history_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.sync_history_id_seq OWNER TO nosdesk_admin;

--
-- Name: sync_history_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.sync_history_id_seq OWNED BY public.sync_history.id;


--
-- Name: system_meta; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.system_meta (
    key text NOT NULL,
    value jsonb NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.system_meta OWNER TO nosdesk;

--
-- Name: tags; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.tags (
    id integer NOT NULL,
    name character varying(64) NOT NULL,
    color character varying(32),
    description text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    archived_at timestamp with time zone,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.tags FORCE ROW LEVEL SECURITY;


ALTER TABLE public.tags OWNER TO nosdesk_admin;

--
-- Name: tags_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.tags_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.tags_id_seq OWNER TO nosdesk_admin;

--
-- Name: tags_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.tags_id_seq OWNED BY public.tags.id;


--
-- Name: ticket_assets; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.ticket_assets (
    ticket_id integer CONSTRAINT ticket_devices_ticket_id_not_null NOT NULL,
    asset_id integer CONSTRAINT ticket_devices_device_id_not_null NOT NULL,
    created_at timestamp with time zone DEFAULT now() CONSTRAINT ticket_devices_created_at_not_null NOT NULL,
    created_by uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.ticket_assets FORCE ROW LEVEL SECURITY;


ALTER TABLE public.ticket_assets OWNER TO nosdesk_admin;

--
-- Name: ticket_categories; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.ticket_categories (
    id integer NOT NULL,
    uuid uuid DEFAULT gen_random_uuid() NOT NULL,
    name character varying(255) NOT NULL,
    description text,
    color character varying(7),
    icon character varying(50),
    display_order integer DEFAULT 0 NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.ticket_categories FORCE ROW LEVEL SECURITY;


ALTER TABLE public.ticket_categories OWNER TO nosdesk_admin;

--
-- Name: ticket_categories_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.ticket_categories_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.ticket_categories_id_seq OWNER TO nosdesk_admin;

--
-- Name: ticket_categories_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.ticket_categories_id_seq OWNED BY public.ticket_categories.id;


--
-- Name: ticket_rule_runs; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.ticket_rule_runs (
    event_id uuid NOT NULL,
    ticket_id integer NOT NULL,
    rule_id integer NOT NULL,
    fired_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.ticket_rule_runs OWNER TO nosdesk_admin;

--
-- Name: ticket_tags; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.ticket_tags (
    ticket_id integer NOT NULL,
    tag_id integer NOT NULL,
    created_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.ticket_tags FORCE ROW LEVEL SECURITY;


ALTER TABLE public.ticket_tags OWNER TO nosdesk_admin;

--
-- Name: ticket_watchers; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.ticket_watchers (
    ticket_id integer NOT NULL,
    user_uuid uuid NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    auto_added boolean DEFAULT false NOT NULL,
    notify_on_internal_notes boolean DEFAULT true NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.ticket_watchers FORCE ROW LEVEL SECURITY;


ALTER TABLE public.ticket_watchers OWNER TO nosdesk_admin;

--
-- Name: tickets; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.tickets (
    id integer NOT NULL,
    title character varying(255) NOT NULL,
    priority public.ticket_priority DEFAULT 'medium'::public.ticket_priority NOT NULL,
    requester_uuid uuid,
    assignee_uuid uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    closed_at timestamp with time zone,
    closed_by uuid,
    category_id integer,
    submitted_via character varying(32),
    guest_lookup_token uuid,
    verification_state character varying(32),
    origin_channel_id integer,
    workflow_state_id integer NOT NULL,
    triage_state character varying(20),
    due_date timestamp with time zone,
    recurrence_rule text,
    recurrence_template_id integer,
    resolution_notes text,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    first_response_at timestamp with time zone,
    sla_response_target_at timestamp with time zone,
    sla_response_breached_at timestamp with time zone,
    sla_resolution_target_at timestamp with time zone,
    sla_resolution_breached_at timestamp with time zone,
    merged_into_ticket_id integer,
    merged_at timestamp with time zone,
    merged_by_user_uuid uuid,
    merge_reason text,
    CONSTRAINT tickets_dates_valid CHECK (((closed_at IS NULL) OR (closed_at >= created_at))),
    CONSTRAINT tickets_merge_complete CHECK ((((merged_into_ticket_id IS NULL) AND (merged_at IS NULL) AND (merged_by_user_uuid IS NULL)) OR ((merged_into_ticket_id IS NOT NULL) AND (merged_at IS NOT NULL) AND (merged_by_user_uuid IS NOT NULL)))),
    CONSTRAINT tickets_triage_state_check CHECK (((triage_state IS NULL) OR ((triage_state)::text = ANY ((ARRAY['untriaged'::character varying, 'triaged'::character varying, 'rejected'::character varying])::text[]))))
);

ALTER TABLE ONLY public.tickets FORCE ROW LEVEL SECURITY;


ALTER TABLE public.tickets OWNER TO nosdesk_admin;

--
-- Name: tickets_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.tickets_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.tickets_id_seq OWNER TO nosdesk_admin;

--
-- Name: tickets_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.tickets_id_seq OWNED BY public.tickets.id;


--
-- Name: user_auth_identities; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.user_auth_identities (
    id integer NOT NULL,
    user_uuid uuid NOT NULL,
    provider_type character varying(50) NOT NULL,
    external_id character varying(255) NOT NULL,
    email character varying(320),
    metadata jsonb,
    password_hash character varying(255),
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid
);


ALTER TABLE public.user_auth_identities OWNER TO nosdesk;

--
-- Name: user_auth_identities_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk
--

CREATE SEQUENCE public.user_auth_identities_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.user_auth_identities_id_seq OWNER TO nosdesk;

--
-- Name: user_auth_identities_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk
--

ALTER SEQUENCE public.user_auth_identities_id_seq OWNED BY public.user_auth_identities.id;


--
-- Name: user_emails; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.user_emails (
    id integer NOT NULL,
    user_uuid uuid NOT NULL,
    email character varying(320) NOT NULL,
    email_type character varying(50) DEFAULT 'personal'::character varying NOT NULL,
    is_primary boolean DEFAULT false NOT NULL,
    is_verified boolean DEFAULT false NOT NULL,
    source character varying(50),
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    CONSTRAINT valid_email_format CHECK (((email)::text ~* '^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$'::text))
);


ALTER TABLE public.user_emails OWNER TO nosdesk;

--
-- Name: user_emails_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk
--

CREATE SEQUENCE public.user_emails_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.user_emails_id_seq OWNER TO nosdesk;

--
-- Name: user_emails_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk
--

ALTER SEQUENCE public.user_emails_id_seq OWNED BY public.user_emails.id;


--
-- Name: user_groups; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.user_groups (
    user_uuid uuid NOT NULL,
    group_id integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.user_groups FORCE ROW LEVEL SECURITY;


ALTER TABLE public.user_groups OWNER TO nosdesk_admin;

--
-- Name: user_preferences; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.user_preferences (
    user_uuid uuid NOT NULL,
    theme character varying(50),
    signature text,
    dashboard_layout jsonb,
    locale text,
    timezone text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.user_preferences OWNER TO nosdesk;

--
-- Name: user_recovery_codes; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.user_recovery_codes (
    id bigint NOT NULL,
    user_uuid uuid NOT NULL,
    code_hash text NOT NULL,
    used_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.user_recovery_codes OWNER TO nosdesk;

--
-- Name: user_recovery_codes_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk
--

CREATE SEQUENCE public.user_recovery_codes_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.user_recovery_codes_id_seq OWNER TO nosdesk;

--
-- Name: user_recovery_codes_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk
--

ALTER SEQUENCE public.user_recovery_codes_id_seq OWNED BY public.user_recovery_codes.id;


--
-- Name: user_ticket_views; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.user_ticket_views (
    id integer NOT NULL,
    user_uuid uuid NOT NULL,
    ticket_id integer NOT NULL,
    first_viewed_at timestamp with time zone DEFAULT now() NOT NULL,
    last_viewed_at timestamp with time zone DEFAULT now() NOT NULL,
    view_count integer DEFAULT 1 NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.user_ticket_views FORCE ROW LEVEL SECURITY;


ALTER TABLE public.user_ticket_views OWNER TO nosdesk_admin;

--
-- Name: user_ticket_views_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.user_ticket_views_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.user_ticket_views_id_seq OWNER TO nosdesk_admin;

--
-- Name: user_ticket_views_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.user_ticket_views_id_seq OWNED BY public.user_ticket_views.id;


--
-- Name: users; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.users (
    uuid uuid DEFAULT public.uuid_generate_v7() NOT NULL,
    name character varying(255) NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    password_changed_at timestamp with time zone,
    pronouns character varying(100),
    avatar_url character varying(2048),
    banner_url character varying(2048),
    avatar_thumb character varying(2048),
    microsoft_uuid uuid,
    mfa_enabled boolean DEFAULT false NOT NULL,
    feature_flag_overrides jsonb DEFAULT '{}'::jsonb NOT NULL,
    deleted_at timestamp with time zone,
    mfa_secret bytea,
    mfa_secret_kek_id smallint,
    platform_role character varying(32) DEFAULT 'user'::character varying NOT NULL,
    CONSTRAINT users_mfa_secret_kek_id_present_iff_secret CHECK (((mfa_secret IS NULL) = (mfa_secret_kek_id IS NULL))),
    CONSTRAINT users_platform_role_check CHECK (((platform_role)::text = ANY ((ARRAY['platform_admin'::character varying, 'audit_reviewer'::character varying, 'user'::character varying])::text[])))
);


ALTER TABLE public.users OWNER TO nosdesk;

--
-- Name: webhook_deliveries; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.webhook_deliveries (
    id integer NOT NULL,
    uuid uuid DEFAULT public.uuid_generate_v7() NOT NULL,
    webhook_id integer NOT NULL,
    event_type character varying(100) NOT NULL,
    payload jsonb NOT NULL,
    request_headers jsonb,
    response_status integer,
    response_body text,
    response_headers jsonb,
    attempt_number integer DEFAULT 1 NOT NULL,
    duration_ms integer,
    error_message text,
    delivered_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    next_retry_at timestamp with time zone,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.webhook_deliveries FORCE ROW LEVEL SECURITY;


ALTER TABLE public.webhook_deliveries OWNER TO nosdesk_admin;

--
-- Name: webhook_deliveries_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.webhook_deliveries_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.webhook_deliveries_id_seq OWNER TO nosdesk_admin;

--
-- Name: webhook_deliveries_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.webhook_deliveries_id_seq OWNED BY public.webhook_deliveries.id;


--
-- Name: webhook_outbox; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.webhook_outbox (
    sync_id bigint NOT NULL,
    enqueued_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.webhook_outbox OWNER TO nosdesk_admin;

--
-- Name: webhooks; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.webhooks (
    id integer NOT NULL,
    uuid uuid DEFAULT public.uuid_generate_v7() NOT NULL,
    name character varying(255) NOT NULL,
    url text NOT NULL,
    secret character varying(255) NOT NULL,
    events text[] DEFAULT '{}'::text[] NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    headers jsonb DEFAULT '{}'::jsonb,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    last_triggered_at timestamp with time zone,
    failure_count integer DEFAULT 0 NOT NULL,
    disabled_reason text,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.webhooks FORCE ROW LEVEL SECURITY;


ALTER TABLE public.webhooks OWNER TO nosdesk_admin;

--
-- Name: webhooks_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.webhooks_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.webhooks_id_seq OWNER TO nosdesk_admin;

--
-- Name: webhooks_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.webhooks_id_seq OWNED BY public.webhooks.id;


--
-- Name: workflow_states; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.workflow_states (
    id integer NOT NULL,
    name character varying(64) NOT NULL,
    category public.workflow_state_category NOT NULL,
    color character varying(20) NOT NULL,
    "position" integer NOT NULL,
    is_default boolean DEFAULT false NOT NULL,
    archived_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    pauses_sla boolean DEFAULT true NOT NULL
);

ALTER TABLE ONLY public.workflow_states FORCE ROW LEVEL SECURITY;


ALTER TABLE public.workflow_states OWNER TO nosdesk_admin;

--
-- Name: workflow_states_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.workflow_states_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.workflow_states_id_seq OWNER TO nosdesk_admin;

--
-- Name: workflow_states_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.workflow_states_id_seq OWNED BY public.workflow_states.id;


--
-- Name: working_calendar_holidays; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.working_calendar_holidays (
    id integer NOT NULL,
    calendar_id integer NOT NULL,
    date date NOT NULL,
    label character varying(120),
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    recurrence character varying(20) DEFAULT 'none'::character varying NOT NULL
);

ALTER TABLE ONLY public.working_calendar_holidays FORCE ROW LEVEL SECURITY;


ALTER TABLE public.working_calendar_holidays OWNER TO nosdesk_admin;

--
-- Name: working_calendar_holidays_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.working_calendar_holidays_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.working_calendar_holidays_id_seq OWNER TO nosdesk_admin;

--
-- Name: working_calendar_holidays_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.working_calendar_holidays_id_seq OWNED BY public.working_calendar_holidays.id;


--
-- Name: working_calendars; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.working_calendars (
    id integer NOT NULL,
    name character varying(120) NOT NULL,
    timezone character varying(64) DEFAULT 'UTC'::character varying NOT NULL,
    schedule jsonb NOT NULL,
    is_default boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.working_calendars FORCE ROW LEVEL SECURITY;


ALTER TABLE public.working_calendars OWNER TO nosdesk_admin;

--
-- Name: working_calendars_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.working_calendars_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.working_calendars_id_seq OWNER TO nosdesk_admin;

--
-- Name: working_calendars_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.working_calendars_id_seq OWNED BY public.working_calendars.id;


--
-- Name: workspace_members; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.workspace_members (
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    user_uuid uuid NOT NULL,
    role character varying(32) NOT NULL,
    invited_at timestamp with time zone DEFAULT now() NOT NULL,
    accepted_at timestamp with time zone,
    CONSTRAINT workspace_members_role_check CHECK (((role)::text = ANY ((ARRAY['owner'::character varying, 'admin'::character varying, 'agent'::character varying, 'member'::character varying])::text[])))
);


ALTER TABLE public.workspace_members OWNER TO nosdesk_admin;

--
-- Name: workspaces; Type: TABLE; Schema: public; Owner: nosdesk
--

CREATE TABLE public.workspaces (
    id integer NOT NULL,
    uuid uuid DEFAULT gen_random_uuid() NOT NULL,
    slug character varying(64) NOT NULL,
    name character varying(200) NOT NULL,
    plan character varying(32) DEFAULT 'free'::character varying NOT NULL,
    settings jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    archived_at timestamp with time zone,
    organisation_id integer,
    custom_domain text,
    CONSTRAINT workspaces_slug_check CHECK (((slug)::text ~ '^[a-z0-9]([a-z0-9-]{0,62}[a-z0-9])?$'::text)),
    CONSTRAINT workspaces_slug_not_reserved CHECK (((slug)::text <> ALL ((ARRAY['about'::character varying, 'access'::character varying, 'account'::character varying, 'accounts'::character varying, 'adm'::character varying, 'admin'::character varying, 'administrator'::character varying, 'administrators'::character varying, 'ads'::character varying, 'alpha'::character varying, 'alumni'::character varying, 'api'::character varying, 'api-v1'::character varying, 'api-v2'::character varying, 'api-v3'::character varying, 'app'::character varying, 'apps'::character varying, 'archive'::character varying, 'assets'::character varying, 'auth'::character varying, 'authenticate'::character varying, 'autoconfig'::character varying, 'autodiscover'::character varying, 'backup'::character varying, 'backups'::character varying, 'bbs'::character varying, 'beta'::character varying, 'billing'::character varying, 'blog'::character varying, 'blogs'::character varying, 'bugs'::character varying, 'cache'::character varying, 'cacti'::character varying, 'calendar'::character varying, 'callback'::character varying, 'callbacks'::character varying, 'cart'::character varying, 'catalog'::character varying, 'cdn'::character varying, 'cert'::character varying, 'certs'::character varying, 'changelog'::character varying, 'chat'::character varying, 'checkout'::character varying, 'citrix'::character varying, 'cloud'::character varying, 'cluster'::character varying, 'clusters'::character varying, 'cms'::character varying, 'community'::character varying, 'conference'::character varying, 'connect'::character varying, 'console'::character varying, 'contact'::character varying, 'contacts'::character varying, 'content'::character varying, 'control'::character varying, 'copyright'::character varying, 'correo'::character varying, 'cpanel'::character varying, 'crm'::character varying, 'crypto'::character varying, 'css'::character varying, 'dashboard'::character varying, 'data'::character varying, 'demo'::character varying, 'dev'::character varying, 'dev2'::character varying, 'devel'::character varying, 'develop'::character varying, 'development'::character varying, 'dialin'::character varying, 'dns'::character varying, 'dns1'::character varying, 'dns2'::character varying, 'dns3'::character varying, 'dns4'::character varying, 'doc'::character varying, 'docs'::character varying, 'documentation'::character varying, 'download'::character varying, 'download-now'::character varying, 'downloads'::character varying, 'edge'::character varying, 'edu'::character varying, 'elearning'::character varying, 'email'::character varying, 'english'::character varying, 'error'::character varying, 'events'::character varying, 'exchange'::character varying, 'extranet'::character varying, 'facebook'::character varying, 'faq'::character varying, 'faqs'::character varying, 'feeds'::character varying, 'file'::character varying, 'files'::character varying, 'forum'::character varying, 'forums'::character varying, 'ftp'::character varying, 'ftp1'::character varying, 'ftp2'::character varying, 'ftps'::character varying, 'gallery'::character varying, 'game'::character varying, 'games'::character varying, 'gateway'::character varying, 'get'::character varying, 'git'::character varying, 'gmail'::character varying, 'grafana'::character varying, 'graphql'::character varying, 'grpc'::character varying, 'health'::character varying, 'healthcheck'::character varying, 'healthz'::character varying, 'help'::character varying, 'helpcenter'::character varying, 'helpdesk'::character varying, 'home'::character varying, 'host'::character varying, 'host2'::character varying, 'hosting'::character varying, 'id'::character varying, 'identity'::character varying, 'idp'::character varying, 'image'::character varying, 'images'::character varying, 'images2'::character varying, 'imap'::character varying, 'imaps'::character varying, 'img'::character varying, 'img2'::character varying, 'info'::character varying, 'install'::character varying, 'installer'::character varying, 'internal'::character varying, 'intranet'::character varying, 'invoice'::character varying, 'invoices'::character varying, 'iphone'::character varying, 'ipv4'::character varying, 'irc'::character varying, 'jabber'::character varying, 'jira'::character varying, 'job'::character varying, 'jobs'::character varying, 'jwks'::character varying, 'k8s'::character varying, 'kb'::character varying, 'key'::character varying, 'keys'::character varying, 'kibana'::character varying, 'kubernetes'::character varying, 'ldap'::character varying, 'legacy'::character varying, 'legal'::character varying, 'lib'::character varying, 'library'::character varying, 'list'::character varying, 'lists'::character varying, 'live'::character varying, 'local'::character varying, 'localhost'::character varying, 'log'::character varying, 'login'::character varying, 'logout'::character varying, 'logs'::character varying, 'lyncdiscover'::character varying, 'mail'::character varying, 'mail1'::character varying, 'mail2'::character varying, 'mail3'::character varying, 'mail4'::character varying, 'mailadmin'::character varying, 'mailer'::character varying, 'mailhost'::character varying, 'mailserver'::character varying, 'manage'::character varying, 'marketing'::character varying, 'master'::character varying, 'media'::character varying, 'meet'::character varying, 'member'::character varying, 'members'::character varying, 'metrics'::character varying, 'mfa'::character varying, 'mobile'::character varying, 'monitor'::character varying, 'monitoring'::character varying, 'moodle'::character varying, 'mrtg'::character varying, 'msoid'::character varying, 'mssql'::character varying, 'music'::character varying, 'mx'::character varying, 'mx1'::character varying, 'mx2'::character varying, 'mx3'::character varying, 'mysql'::character varying, 'nagios'::character varying, 'new'::character varying, 'news'::character varying, 'newsletter'::character varying, 'nosdesk'::character varying, 'ns'::character varying, 'ns0'::character varying, 'ns1'::character varying, 'ns2'::character varying, 'ns3'::character varying, 'ns4'::character varying, 'ns5'::character varying, 'ns6'::character varying, 'ntp'::character varying, 'oauth'::character varying, 'oauth2'::character varying, 'office'::character varying, 'oidc'::character varying, 'old'::character varying, 'online'::character varying, 'owa'::character varying, 'panel'::character varying, 'partner'::character varying, 'partners'::character varying, 'passkey'::character varying, 'password'::character varying, 'passwords'::character varying, 'pay'::character varying, 'payment'::character varying, 'payments'::character varying, 'pda'::character varying, 'photo'::character varying, 'photos'::character varying, 'phpmyadmin'::character varying, 'ping'::character varying, 'plan'::character varying, 'plans'::character varying, 'poczta'::character varying, 'policy'::character varying, 'pop'::character varying, 'pop3'::character varying, 'portal'::character varying, 'post'::character varying, 'preprod'::character varying, 'press'::character varying, 'preview'::character varying, 'pricing'::character varying, 'privacy'::character varying, 'private'::character varying, 'prod'::character varying, 'production'::character varying, 'project'::character varying, 'projects'::character varying, 'prometheus'::character varying, 'proxy'::character varying, 'public'::character varying, 'qa'::character varying, 'queue'::character varying, 'queues'::character varying, 'radio'::character varying, 'ready'::character varying, 'redmine'::character varying, 'register'::character varying, 'registration'::character varying, 'relay'::character varying, 'release'::character varying, 'releases'::character varying, 'remote'::character varying, 'reports'::character varying, 'root'::character varying, 'router'::character varying, 'rss'::character varying, 'saml'::character varying, 'sandbox'::character varying, 'search'::character varying, 'secure'::character varying, 'security'::character varying, 'server'::character varying, 'server1'::character varying, 'service'::character varying, 'services'::character varying, 'session'::character varying, 'sessions'::character varying, 'settings'::character varying, 'sftp'::character varying, 'sharepoint'::character varying, 'shop'::character varying, 'signin'::character varying, 'signout'::character varying, 'signup'::character varying, 'sip'::character varying, 'site'::character varying, 'sites'::character varying, 'sms'::character varying, 'smtp'::character varying, 'smtp1'::character varying, 'smtp2'::character varying, 'smtps'::character varying, 'speedtest'::character varying, 'sport'::character varying, 'sql'::character varying, 'ssh'::character varying, 'ssl'::character varying, 'sso'::character varying, 'staff'::character varying, 'stage'::character varying, 'staging'::character varying, 'start'::character varying, 'stat'::character varying, 'static'::character varying, 'stats'::character varying, 'status'::character varying, 'storage'::character varying, 'store'::character varying, 'stream'::character varying, 'streaming'::character varying, 'student'::character varying, 'sub'::character varying, 'subscribe'::character varying, 'subscription'::character varying, 'subscriptions'::character varying, 'sudo'::character varying, 'superuser'::character varying, 'support'::character varying, 'survey'::character varying, 'svn'::character varying, 'terms'::character varying, 'test'::character varying, 'test1'::character varying, 'test2'::character varying, 'testing'::character varying, 'tests'::character varying, 'time'::character varying, 'tls'::character varying, 'token'::character varying, 'tokens'::character varying, 'tools'::character varying, 'totp'::character varying, 'trac'::character varying, 'training'::character varying, 'travel'::character varying, 'uat'::character varying, 'update'::character varying, 'upgrade'::character varying, 'upload'::character varying, 'uploads'::character varying, 'validate'::character varying, 'verify'::character varying, 'video'::character varying, 'videos'::character varying, 'voip'::character varying, 'vpn'::character varying, 'vpn2'::character varying, 'vps'::character varying, 'wallet'::character varying, 'wap'::character varying, 'web'::character varying, 'web1'::character varying, 'web2'::character varying, 'web3'::character varying, 'web4'::character varying, 'web5'::character varying, 'webdisk'::character varying, 'webhook'::character varying, 'webhooks'::character varying, 'webmail'::character varying, 'webmail2'::character varying, 'websocket'::character varying, 'whm'::character varying, 'wiki'::character varying, 'worker'::character varying, 'workers'::character varying, 'ws'::character varying, 'wss'::character varying, 'ww2'::character varying, 'www'::character varying, 'www1'::character varying, 'www2'::character varying, 'www3'::character varying, 'www4'::character varying, 'www5'::character varying, 'www6'::character varying, 'wwww'::character varying])::text[])))
);


ALTER TABLE public.workspaces OWNER TO nosdesk;

--
-- Name: workspaces_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk
--

CREATE SEQUENCE public.workspaces_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.workspaces_id_seq OWNER TO nosdesk;

--
-- Name: workspaces_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk
--

ALTER SEQUENCE public.workspaces_id_seq OWNED BY public.workspaces.id;


--
-- Name: yjs_snapshots; Type: TABLE; Schema: public; Owner: nosdesk_admin
--

CREATE TABLE public.yjs_snapshots (
    id bigint NOT NULL,
    workspace_id integer NOT NULL,
    document_id text NOT NULL,
    snapshot bytea NOT NULL,
    state_vector bytea NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

ALTER TABLE ONLY public.yjs_snapshots FORCE ROW LEVEL SECURITY;


ALTER TABLE public.yjs_snapshots OWNER TO nosdesk_admin;

--
-- Name: yjs_snapshots_id_seq; Type: SEQUENCE; Schema: public; Owner: nosdesk_admin
--

CREATE SEQUENCE public.yjs_snapshots_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.yjs_snapshots_id_seq OWNER TO nosdesk_admin;

--
-- Name: yjs_snapshots_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: nosdesk_admin
--

ALTER SEQUENCE public.yjs_snapshots_id_seq OWNED BY public.yjs_snapshots.id;


--
-- Name: audit_log_2026_05; Type: TABLE ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.audit_log ATTACH PARTITION public.audit_log_2026_05 FOR VALUES FROM ('2026-05-01 00:00:00+00') TO ('2026-06-01 00:00:00+00');


--
-- Name: audit_log_2026_06; Type: TABLE ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.audit_log ATTACH PARTITION public.audit_log_2026_06 FOR VALUES FROM ('2026-06-01 00:00:00+00') TO ('2026-07-01 00:00:00+00');


--
-- Name: audit_log_2026_07; Type: TABLE ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.audit_log ATTACH PARTITION public.audit_log_2026_07 FOR VALUES FROM ('2026-07-01 00:00:00+00') TO ('2026-08-01 00:00:00+00');


--
-- Name: audit_log_2026_08; Type: TABLE ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.audit_log ATTACH PARTITION public.audit_log_2026_08 FOR VALUES FROM ('2026-08-01 00:00:00+00') TO ('2026-09-01 00:00:00+00');


--
-- Name: audit_log_default; Type: TABLE ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.audit_log ATTACH PARTITION public.audit_log_default DEFAULT;


--
-- Name: sync_actions_2026_05; Type: TABLE ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_actions ATTACH PARTITION public.sync_actions_2026_05 FOR VALUES FROM ('2026-05-01 00:00:00+00') TO ('2026-06-01 00:00:00+00');


--
-- Name: sync_actions_2026_06; Type: TABLE ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_actions ATTACH PARTITION public.sync_actions_2026_06 FOR VALUES FROM ('2026-06-01 00:00:00+00') TO ('2026-07-01 00:00:00+00');


--
-- Name: sync_actions_2026_07; Type: TABLE ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_actions ATTACH PARTITION public.sync_actions_2026_07 FOR VALUES FROM ('2026-07-01 00:00:00+00') TO ('2026-08-01 00:00:00+00');


--
-- Name: sync_actions_2026_08; Type: TABLE ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_actions ATTACH PARTITION public.sync_actions_2026_08 FOR VALUES FROM ('2026-08-01 00:00:00+00') TO ('2026-09-01 00:00:00+00');


--
-- Name: sync_actions_default; Type: TABLE ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_actions ATTACH PARTITION public.sync_actions_default DEFAULT;


--
-- Name: active_sessions id; Type: DEFAULT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.active_sessions ALTER COLUMN id SET DEFAULT nextval('public.active_sessions_id_seq'::regclass);


--
-- Name: api_tokens id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.api_tokens ALTER COLUMN id SET DEFAULT nextval('public.api_tokens_id_seq'::regclass);


--
-- Name: article_content_revisions id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.article_content_revisions ALTER COLUMN id SET DEFAULT nextval('public.article_content_revisions_id_seq'::regclass);


--
-- Name: article_contents id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.article_contents ALTER COLUMN id SET DEFAULT nextval('public.article_contents_id_seq'::regclass);


--
-- Name: asset_audits id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_audits ALTER COLUMN id SET DEFAULT nextval('public.asset_audits_id_seq'::regclass);


--
-- Name: asset_kinds id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_kinds ALTER COLUMN id SET DEFAULT nextval('public.asset_kinds_id_seq'::regclass);


--
-- Name: asset_usage_log id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_usage_log ALTER COLUMN id SET DEFAULT nextval('public.asset_usage_log_id_seq'::regclass);


--
-- Name: assets id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assets ALTER COLUMN id SET DEFAULT nextval('public.devices_id_seq'::regclass);


--
-- Name: assignment_log id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assignment_log ALTER COLUMN id SET DEFAULT nextval('public.assignment_log_id_seq'::regclass);


--
-- Name: assignment_rules id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assignment_rules ALTER COLUMN id SET DEFAULT nextval('public.assignment_rules_id_seq'::regclass);


--
-- Name: attachments id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.attachments ALTER COLUMN id SET DEFAULT nextval('public.attachments_id_seq'::regclass);


--
-- Name: audit_log id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.audit_log ALTER COLUMN id SET DEFAULT nextval('public.audit_log_id_seq'::regclass);


--
-- Name: bug_reports id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.bug_reports ALTER COLUMN id SET DEFAULT nextval('public.bug_reports_id_seq'::regclass);


--
-- Name: canned_response_insertions id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.canned_response_insertions ALTER COLUMN id SET DEFAULT nextval('public.canned_response_insertions_id_seq'::regclass);


--
-- Name: canned_responses id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.canned_responses ALTER COLUMN id SET DEFAULT nextval('public.canned_responses_id_seq'::regclass);


--
-- Name: channel_credentials id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.channel_credentials ALTER COLUMN id SET DEFAULT nextval('public.channel_credentials_id_seq'::regclass);


--
-- Name: channel_messages id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.channel_messages ALTER COLUMN id SET DEFAULT nextval('public.channel_messages_id_seq'::regclass);


--
-- Name: channels id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.channels ALTER COLUMN id SET DEFAULT nextval('public.channels_id_seq'::regclass);


--
-- Name: comments id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.comments ALTER COLUMN id SET DEFAULT nextval('public.comments_id_seq'::regclass);


--
-- Name: csp_reports id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.csp_reports ALTER COLUMN id SET DEFAULT nextval('public.csp_reports_id_seq'::regclass);


--
-- Name: cycles id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.cycles ALTER COLUMN id SET DEFAULT nextval('public.cycles_id_seq'::regclass);


--
-- Name: documentation_collection_visibility id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collection_visibility ALTER COLUMN id SET DEFAULT nextval('public.documentation_collection_visibility_id_seq'::regclass);


--
-- Name: documentation_collections id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collections ALTER COLUMN id SET DEFAULT nextval('public.documentation_collections_id_seq'::regclass);


--
-- Name: documentation_page_visibility id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_page_visibility ALTER COLUMN id SET DEFAULT nextval('public.documentation_page_visibility_id_seq'::regclass);


--
-- Name: documentation_pages id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_pages ALTER COLUMN id SET DEFAULT nextval('public.documentation_pages_id_seq'::regclass);


--
-- Name: documentation_revisions id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_revisions ALTER COLUMN id SET DEFAULT nextval('public.documentation_revisions_id_seq'::regclass);


--
-- Name: documentation_starred_pages id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_starred_pages ALTER COLUMN id SET DEFAULT nextval('public.documentation_starred_pages_id_seq'::regclass);


--
-- Name: documentation_subscriptions id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_subscriptions ALTER COLUMN id SET DEFAULT nextval('public.documentation_subscriptions_id_seq'::regclass);


--
-- Name: groups id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.groups ALTER COLUMN id SET DEFAULT nextval('public.groups_id_seq'::regclass);


--
-- Name: knowledge_gap_signals id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.knowledge_gap_signals ALTER COLUMN id SET DEFAULT nextval('public.knowledge_gap_signals_id_seq'::regclass);


--
-- Name: knowledge_gaps id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.knowledge_gaps ALTER COLUMN id SET DEFAULT nextval('public.knowledge_gaps_id_seq'::regclass);


--
-- Name: notification_preferences id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.notification_preferences ALTER COLUMN id SET DEFAULT nextval('public.notification_preferences_id_seq'::regclass);


--
-- Name: notification_rate_limits id; Type: DEFAULT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.notification_rate_limits ALTER COLUMN id SET DEFAULT nextval('public.notification_rate_limits_id_seq'::regclass);


--
-- Name: notification_types id; Type: DEFAULT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.notification_types ALTER COLUMN id SET DEFAULT nextval('public.notification_types_id_seq'::regclass);


--
-- Name: notifications id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.notifications ALTER COLUMN id SET DEFAULT nextval('public.notifications_id_seq'::regclass);


--
-- Name: outbound_emails id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.outbound_emails ALTER COLUMN id SET DEFAULT nextval('public.outbound_emails_id_seq'::regclass);


--
-- Name: plugin_activity id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_activity ALTER COLUMN id SET DEFAULT nextval('public.plugin_activity_id_seq'::regclass);


--
-- Name: plugin_collection_rows id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_collection_rows ALTER COLUMN id SET DEFAULT nextval('public.plugin_collection_rows_id_seq'::regclass);


--
-- Name: plugin_collection_schemas id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_collection_schemas ALTER COLUMN id SET DEFAULT nextval('public.plugin_collection_schemas_id_seq'::regclass);


--
-- Name: plugin_data id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_data ALTER COLUMN id SET DEFAULT nextval('public.plugin_data_id_seq'::regclass);


--
-- Name: plugin_trusted_publishers id; Type: DEFAULT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.plugin_trusted_publishers ALTER COLUMN id SET DEFAULT nextval('public.plugin_trusted_publishers_id_seq'::regclass);


--
-- Name: plugins id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugins ALTER COLUMN id SET DEFAULT nextval('public.plugins_id_seq'::regclass);


--
-- Name: projects id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.projects ALTER COLUMN id SET DEFAULT nextval('public.projects_id_seq'::regclass);


--
-- Name: refresh_tokens id; Type: DEFAULT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.refresh_tokens ALTER COLUMN id SET DEFAULT nextval('public.refresh_tokens_id_seq'::regclass);


--
-- Name: rule_applications id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.rule_applications ALTER COLUMN id SET DEFAULT nextval('public.rule_applications_id_seq'::regclass);


--
-- Name: rule_versions id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.rule_versions ALTER COLUMN id SET DEFAULT nextval('public.rule_versions_id_seq'::regclass);


--
-- Name: rules id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.rules ALTER COLUMN id SET DEFAULT nextval('public.rules_id_seq'::regclass);


--
-- Name: saved_views id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.saved_views ALTER COLUMN id SET DEFAULT nextval('public.saved_views_id_seq'::regclass);


--
-- Name: search_index_state id; Type: DEFAULT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.search_index_state ALTER COLUMN id SET DEFAULT nextval('public.search_index_state_id_seq'::regclass);


--
-- Name: search_query_log id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.search_query_log ALTER COLUMN id SET DEFAULT nextval('public.search_query_log_id_seq'::regclass);


--
-- Name: security_events id; Type: DEFAULT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.security_events ALTER COLUMN id SET DEFAULT nextval('public.security_events_id_seq'::regclass);


--
-- Name: sla_policies id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sla_policies ALTER COLUMN id SET DEFAULT nextval('public.sla_policies_id_seq'::regclass);


--
-- Name: sync_actions sync_id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_actions ALTER COLUMN sync_id SET DEFAULT nextval('public.sync_actions_sync_id_seq'::regclass);


--
-- Name: sync_delta_tokens id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_delta_tokens ALTER COLUMN id SET DEFAULT nextval('public.sync_delta_tokens_id_seq'::regclass);


--
-- Name: sync_history id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_history ALTER COLUMN id SET DEFAULT nextval('public.sync_history_id_seq'::regclass);


--
-- Name: tags id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.tags ALTER COLUMN id SET DEFAULT nextval('public.tags_id_seq'::regclass);


--
-- Name: ticket_categories id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_categories ALTER COLUMN id SET DEFAULT nextval('public.ticket_categories_id_seq'::regclass);


--
-- Name: tickets id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.tickets ALTER COLUMN id SET DEFAULT nextval('public.tickets_id_seq'::regclass);


--
-- Name: user_auth_identities id; Type: DEFAULT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.user_auth_identities ALTER COLUMN id SET DEFAULT nextval('public.user_auth_identities_id_seq'::regclass);


--
-- Name: user_emails id; Type: DEFAULT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.user_emails ALTER COLUMN id SET DEFAULT nextval('public.user_emails_id_seq'::regclass);


--
-- Name: user_recovery_codes id; Type: DEFAULT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.user_recovery_codes ALTER COLUMN id SET DEFAULT nextval('public.user_recovery_codes_id_seq'::regclass);


--
-- Name: user_ticket_views id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.user_ticket_views ALTER COLUMN id SET DEFAULT nextval('public.user_ticket_views_id_seq'::regclass);


--
-- Name: webhook_deliveries id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.webhook_deliveries ALTER COLUMN id SET DEFAULT nextval('public.webhook_deliveries_id_seq'::regclass);


--
-- Name: webhooks id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.webhooks ALTER COLUMN id SET DEFAULT nextval('public.webhooks_id_seq'::regclass);


--
-- Name: workflow_states id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.workflow_states ALTER COLUMN id SET DEFAULT nextval('public.workflow_states_id_seq'::regclass);


--
-- Name: working_calendar_holidays id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.working_calendar_holidays ALTER COLUMN id SET DEFAULT nextval('public.working_calendar_holidays_id_seq'::regclass);


--
-- Name: working_calendars id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.working_calendars ALTER COLUMN id SET DEFAULT nextval('public.working_calendars_id_seq'::regclass);


--
-- Name: workspaces id; Type: DEFAULT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.workspaces ALTER COLUMN id SET DEFAULT nextval('public.workspaces_id_seq'::regclass);


--
-- Name: yjs_snapshots id; Type: DEFAULT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.yjs_snapshots ALTER COLUMN id SET DEFAULT nextval('public.yjs_snapshots_id_seq'::regclass);


--
-- Name: active_sessions active_sessions_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.active_sessions
    ADD CONSTRAINT active_sessions_pkey PRIMARY KEY (id);


--
-- Name: active_sessions active_sessions_session_id_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.active_sessions
    ADD CONSTRAINT active_sessions_session_id_key UNIQUE (session_id);


--
-- Name: api_tokens api_tokens_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.api_tokens
    ADD CONSTRAINT api_tokens_pkey PRIMARY KEY (id);


--
-- Name: api_tokens api_tokens_token_hash_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.api_tokens
    ADD CONSTRAINT api_tokens_token_hash_key UNIQUE (token_hash);


--
-- Name: api_tokens api_tokens_uuid_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.api_tokens
    ADD CONSTRAINT api_tokens_uuid_key UNIQUE (uuid);


--
-- Name: article_content_revisions article_content_revisions_article_content_id_revision_numbe_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.article_content_revisions
    ADD CONSTRAINT article_content_revisions_article_content_id_revision_numbe_key UNIQUE (article_content_id, revision_number);


--
-- Name: article_content_revisions article_content_revisions_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.article_content_revisions
    ADD CONSTRAINT article_content_revisions_pkey PRIMARY KEY (id);


--
-- Name: article_contents article_contents_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.article_contents
    ADD CONSTRAINT article_contents_pkey PRIMARY KEY (id);


--
-- Name: asset_audits asset_audits_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_audits
    ADD CONSTRAINT asset_audits_pkey PRIMARY KEY (id);


--
-- Name: asset_kinds asset_kinds_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_kinds
    ADD CONSTRAINT asset_kinds_pkey PRIMARY KEY (id);


--
-- Name: asset_kinds asset_kinds_workspace_slug_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_kinds
    ADD CONSTRAINT asset_kinds_workspace_slug_key UNIQUE (workspace_id, slug);


--
-- Name: asset_usage_log asset_usage_log_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_usage_log
    ADD CONSTRAINT asset_usage_log_pkey PRIMARY KEY (id);


--
-- Name: assignment_log assignment_log_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assignment_log
    ADD CONSTRAINT assignment_log_pkey PRIMARY KEY (id);


--
-- Name: assignment_rule_state assignment_rule_state_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assignment_rule_state
    ADD CONSTRAINT assignment_rule_state_pkey PRIMARY KEY (rule_id);


--
-- Name: assignment_rules assignment_rules_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assignment_rules
    ADD CONSTRAINT assignment_rules_pkey PRIMARY KEY (id);


--
-- Name: assignment_rules assignment_rules_uuid_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assignment_rules
    ADD CONSTRAINT assignment_rules_uuid_key UNIQUE (uuid);


--
-- Name: attachments attachments_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.attachments
    ADD CONSTRAINT attachments_pkey PRIMARY KEY (id);


--
-- Name: audit_log audit_log_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.audit_log
    ADD CONSTRAINT audit_log_pkey PRIMARY KEY (id, occurred_at);


--
-- Name: audit_log_2026_05 audit_log_2026_05_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.audit_log_2026_05
    ADD CONSTRAINT audit_log_2026_05_pkey PRIMARY KEY (id, occurred_at);


--
-- Name: audit_log_2026_06 audit_log_2026_06_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.audit_log_2026_06
    ADD CONSTRAINT audit_log_2026_06_pkey PRIMARY KEY (id, occurred_at);


--
-- Name: audit_log_2026_07 audit_log_2026_07_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.audit_log_2026_07
    ADD CONSTRAINT audit_log_2026_07_pkey PRIMARY KEY (id, occurred_at);


--
-- Name: audit_log_2026_08 audit_log_2026_08_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.audit_log_2026_08
    ADD CONSTRAINT audit_log_2026_08_pkey PRIMARY KEY (id, occurred_at);


--
-- Name: audit_log_default audit_log_default_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.audit_log_default
    ADD CONSTRAINT audit_log_default_pkey PRIMARY KEY (id, occurred_at);


--
-- Name: backup_jobs backup_jobs_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.backup_jobs
    ADD CONSTRAINT backup_jobs_pkey PRIMARY KEY (id);


--
-- Name: bug_reports bug_reports_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.bug_reports
    ADD CONSTRAINT bug_reports_pkey PRIMARY KEY (id);


--
-- Name: canned_response_insertions canned_response_insertions_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.canned_response_insertions
    ADD CONSTRAINT canned_response_insertions_pkey PRIMARY KEY (id);


--
-- Name: canned_responses canned_responses_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.canned_responses
    ADD CONSTRAINT canned_responses_pkey PRIMARY KEY (id);


--
-- Name: category_group_visibility category_group_visibility_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.category_group_visibility
    ADD CONSTRAINT category_group_visibility_pkey PRIMARY KEY (category_id, group_id);


--
-- Name: channel_credentials channel_credentials_channel_id_credential_type_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.channel_credentials
    ADD CONSTRAINT channel_credentials_channel_id_credential_type_key UNIQUE (channel_id, credential_type);


--
-- Name: channel_credentials channel_credentials_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.channel_credentials
    ADD CONSTRAINT channel_credentials_pkey PRIMARY KEY (id);


--
-- Name: channel_messages channel_messages_channel_id_external_id_direction_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.channel_messages
    ADD CONSTRAINT channel_messages_channel_id_external_id_direction_key UNIQUE (channel_id, external_id, direction);


--
-- Name: channel_messages channel_messages_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.channel_messages
    ADD CONSTRAINT channel_messages_pkey PRIMARY KEY (id);


--
-- Name: channels channels_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.channels
    ADD CONSTRAINT channels_pkey PRIMARY KEY (id);


--
-- Name: comments comments_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.comments
    ADD CONSTRAINT comments_pkey PRIMARY KEY (id);


--
-- Name: csp_reports csp_reports_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.csp_reports
    ADD CONSTRAINT csp_reports_pkey PRIMARY KEY (id);


--
-- Name: cycle_tickets cycle_tickets_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.cycle_tickets
    ADD CONSTRAINT cycle_tickets_pkey PRIMARY KEY (cycle_id, ticket_id);


--
-- Name: cycles cycles_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.cycles
    ADD CONSTRAINT cycles_pkey PRIMARY KEY (id);


--
-- Name: cycles cycles_uuid_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.cycles
    ADD CONSTRAINT cycles_uuid_key UNIQUE (uuid);


--
-- Name: asset_groups device_groups_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_groups
    ADD CONSTRAINT device_groups_pkey PRIMARY KEY (asset_id, group_id);


--
-- Name: assets devices_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assets
    ADD CONSTRAINT devices_pkey PRIMARY KEY (id);


--
-- Name: documentation_collection_pages documentation_collection_pages_page_id_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collection_pages
    ADD CONSTRAINT documentation_collection_pages_page_id_key UNIQUE (page_id);


--
-- Name: documentation_collection_pages documentation_collection_pages_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collection_pages
    ADD CONSTRAINT documentation_collection_pages_pkey PRIMARY KEY (collection_id, page_id);


--
-- Name: documentation_collection_visibility documentation_collection_visibility_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collection_visibility
    ADD CONSTRAINT documentation_collection_visibility_pkey PRIMARY KEY (id);


--
-- Name: documentation_collections documentation_collections_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collections
    ADD CONSTRAINT documentation_collections_pkey PRIMARY KEY (id);


--
-- Name: documentation_collections documentation_collections_uuid_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collections
    ADD CONSTRAINT documentation_collections_uuid_key UNIQUE (uuid);


--
-- Name: documentation_collections documentation_collections_workspace_slug_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collections
    ADD CONSTRAINT documentation_collections_workspace_slug_key UNIQUE (workspace_id, slug);


--
-- Name: documentation_page_embeddings documentation_page_embeddings_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_page_embeddings
    ADD CONSTRAINT documentation_page_embeddings_pkey PRIMARY KEY (source_page_id, target_page_id);


--
-- Name: documentation_page_tickets documentation_page_tickets_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_page_tickets
    ADD CONSTRAINT documentation_page_tickets_pkey PRIMARY KEY (page_id, ticket_id);


--
-- Name: documentation_page_visibility documentation_page_visibility_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_page_visibility
    ADD CONSTRAINT documentation_page_visibility_pkey PRIMARY KEY (id);


--
-- Name: documentation_pages documentation_pages_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_pages
    ADD CONSTRAINT documentation_pages_pkey PRIMARY KEY (id);


--
-- Name: documentation_pages documentation_pages_uuid_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_pages
    ADD CONSTRAINT documentation_pages_uuid_key UNIQUE (uuid);


--
-- Name: documentation_pages documentation_pages_workspace_slug_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_pages
    ADD CONSTRAINT documentation_pages_workspace_slug_key UNIQUE (workspace_id, slug);


--
-- Name: documentation_revisions documentation_revisions_page_id_revision_number_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_revisions
    ADD CONSTRAINT documentation_revisions_page_id_revision_number_key UNIQUE (page_id, revision_number);


--
-- Name: documentation_revisions documentation_revisions_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_revisions
    ADD CONSTRAINT documentation_revisions_pkey PRIMARY KEY (id);


--
-- Name: documentation_starred_pages documentation_starred_pages_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_starred_pages
    ADD CONSTRAINT documentation_starred_pages_pkey PRIMARY KEY (id);


--
-- Name: documentation_starred_pages documentation_starred_pages_user_uuid_page_id_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_starred_pages
    ADD CONSTRAINT documentation_starred_pages_user_uuid_page_id_key UNIQUE (user_uuid, page_id);


--
-- Name: documentation_subscriptions documentation_subscriptions_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_subscriptions
    ADD CONSTRAINT documentation_subscriptions_pkey PRIMARY KEY (id);


--
-- Name: documentation_subscriptions documentation_subscriptions_user_uuid_page_id_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_subscriptions
    ADD CONSTRAINT documentation_subscriptions_user_uuid_page_id_key UNIQUE (user_uuid, page_id);


--
-- Name: email_suppressions email_suppressions_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.email_suppressions
    ADD CONSTRAINT email_suppressions_pkey PRIMARY KEY (email);


--
-- Name: group_includes group_includes_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.group_includes
    ADD CONSTRAINT group_includes_pkey PRIMARY KEY (parent_group_id, child_group_id);


--
-- Name: groups groups_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.groups
    ADD CONSTRAINT groups_pkey PRIMARY KEY (id);


--
-- Name: groups groups_uuid_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.groups
    ADD CONSTRAINT groups_uuid_key UNIQUE (uuid);


--
-- Name: idempotency_keys idempotency_keys_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.idempotency_keys
    ADD CONSTRAINT idempotency_keys_pkey PRIMARY KEY (key);


--
-- Name: import_jobs import_jobs_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.import_jobs
    ADD CONSTRAINT import_jobs_pkey PRIMARY KEY (id);


--
-- Name: knowledge_gap_signals knowledge_gap_signals_gap_id_source_kind_source_ref_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.knowledge_gap_signals
    ADD CONSTRAINT knowledge_gap_signals_gap_id_source_kind_source_ref_key UNIQUE (gap_id, source_kind, source_ref);


--
-- Name: knowledge_gap_signals knowledge_gap_signals_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.knowledge_gap_signals
    ADD CONSTRAINT knowledge_gap_signals_pkey PRIMARY KEY (id);


--
-- Name: knowledge_gaps knowledge_gaps_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.knowledge_gaps
    ADD CONSTRAINT knowledge_gaps_pkey PRIMARY KEY (id);


--
-- Name: linked_tickets linked_tickets_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.linked_tickets
    ADD CONSTRAINT linked_tickets_pkey PRIMARY KEY (ticket_id, linked_ticket_id);


--
-- Name: notification_preferences notification_preferences_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.notification_preferences
    ADD CONSTRAINT notification_preferences_pkey PRIMARY KEY (id);


--
-- Name: notification_preferences notification_preferences_user_uuid_notification_type_id_cha_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.notification_preferences
    ADD CONSTRAINT notification_preferences_user_uuid_notification_type_id_cha_key UNIQUE (user_uuid, notification_type_id, channel);


--
-- Name: notification_rate_limits notification_rate_limits_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.notification_rate_limits
    ADD CONSTRAINT notification_rate_limits_pkey PRIMARY KEY (id);


--
-- Name: notification_rate_limits notification_rate_limits_user_uuid_notification_type_id_ent_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.notification_rate_limits
    ADD CONSTRAINT notification_rate_limits_user_uuid_notification_type_id_ent_key UNIQUE (user_uuid, notification_type_id, entity_type, entity_id);


--
-- Name: notification_types notification_types_code_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.notification_types
    ADD CONSTRAINT notification_types_code_key UNIQUE (code);


--
-- Name: notification_types notification_types_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.notification_types
    ADD CONSTRAINT notification_types_pkey PRIMARY KEY (id);


--
-- Name: notifications notifications_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT notifications_pkey PRIMARY KEY (id);


--
-- Name: notifications notifications_uuid_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT notifications_uuid_key UNIQUE (uuid);


--
-- Name: outbound_emails outbound_emails_comment_id_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.outbound_emails
    ADD CONSTRAINT outbound_emails_comment_id_key UNIQUE (comment_id);


--
-- Name: outbound_emails outbound_emails_message_id_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.outbound_emails
    ADD CONSTRAINT outbound_emails_message_id_key UNIQUE (message_id);


--
-- Name: outbound_emails outbound_emails_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.outbound_emails
    ADD CONSTRAINT outbound_emails_pkey PRIMARY KEY (id);


--
-- Name: passkey_credentials passkey_credentials_credential_id_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.passkey_credentials
    ADD CONSTRAINT passkey_credentials_credential_id_key UNIQUE (credential_id);


--
-- Name: passkey_credentials passkey_credentials_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.passkey_credentials
    ADD CONSTRAINT passkey_credentials_pkey PRIMARY KEY (id);


--
-- Name: plugin_activity plugin_activity_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_activity
    ADD CONSTRAINT plugin_activity_pkey PRIMARY KEY (id);


--
-- Name: plugin_activity plugin_activity_uuid_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_activity
    ADD CONSTRAINT plugin_activity_uuid_key UNIQUE (uuid);


--
-- Name: plugin_collection_rows plugin_collection_rows_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_collection_rows
    ADD CONSTRAINT plugin_collection_rows_pkey PRIMARY KEY (id);


--
-- Name: plugin_collection_rows plugin_collection_rows_uuid_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_collection_rows
    ADD CONSTRAINT plugin_collection_rows_uuid_key UNIQUE (uuid);


--
-- Name: plugin_collection_schemas plugin_collection_schemas_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_collection_schemas
    ADD CONSTRAINT plugin_collection_schemas_pkey PRIMARY KEY (id);


--
-- Name: plugin_collection_schemas plugin_collection_schemas_plugin_id_collection_name_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_collection_schemas
    ADD CONSTRAINT plugin_collection_schemas_plugin_id_collection_name_key UNIQUE (plugin_id, collection_name);


--
-- Name: plugin_collection_schemas plugin_collection_schemas_uuid_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_collection_schemas
    ADD CONSTRAINT plugin_collection_schemas_uuid_key UNIQUE (uuid);


--
-- Name: plugin_data plugin_data_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_data
    ADD CONSTRAINT plugin_data_pkey PRIMARY KEY (id);


--
-- Name: plugin_data plugin_data_plugin_id_data_type_key_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_data
    ADD CONSTRAINT plugin_data_plugin_id_data_type_key_key UNIQUE (plugin_id, data_type, key);


--
-- Name: plugin_data plugin_data_uuid_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_data
    ADD CONSTRAINT plugin_data_uuid_key UNIQUE (uuid);


--
-- Name: plugin_local_signing_key plugin_local_signing_key_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.plugin_local_signing_key
    ADD CONSTRAINT plugin_local_signing_key_pkey PRIMARY KEY (id);


--
-- Name: plugin_registry_state plugin_registry_state_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.plugin_registry_state
    ADD CONSTRAINT plugin_registry_state_pkey PRIMARY KEY (id);


--
-- Name: plugin_trusted_publishers plugin_trusted_publishers_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.plugin_trusted_publishers
    ADD CONSTRAINT plugin_trusted_publishers_pkey PRIMARY KEY (id);


--
-- Name: plugin_trusted_publishers plugin_trusted_publishers_pubkey_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.plugin_trusted_publishers
    ADD CONSTRAINT plugin_trusted_publishers_pubkey_key UNIQUE (pubkey);


--
-- Name: plugins plugins_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugins
    ADD CONSTRAINT plugins_pkey PRIMARY KEY (id);


--
-- Name: plugins plugins_uuid_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugins
    ADD CONSTRAINT plugins_uuid_key UNIQUE (uuid);


--
-- Name: plugins plugins_workspace_name_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugins
    ADD CONSTRAINT plugins_workspace_name_key UNIQUE (workspace_id, name);


--
-- Name: project_tickets project_tickets_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.project_tickets
    ADD CONSTRAINT project_tickets_pkey PRIMARY KEY (project_id, ticket_id);


--
-- Name: projects projects_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.projects
    ADD CONSTRAINT projects_pkey PRIMARY KEY (id);


--
-- Name: refresh_tokens refresh_tokens_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.refresh_tokens
    ADD CONSTRAINT refresh_tokens_pkey PRIMARY KEY (id);


--
-- Name: refresh_tokens refresh_tokens_token_hash_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.refresh_tokens
    ADD CONSTRAINT refresh_tokens_token_hash_key UNIQUE (token_hash);


--
-- Name: reset_tokens reset_tokens_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.reset_tokens
    ADD CONSTRAINT reset_tokens_pkey PRIMARY KEY (token_hash);


--
-- Name: rule_applications rule_applications_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.rule_applications
    ADD CONSTRAINT rule_applications_pkey PRIMARY KEY (id);


--
-- Name: rule_versions rule_versions_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.rule_versions
    ADD CONSTRAINT rule_versions_pkey PRIMARY KEY (id);


--
-- Name: rule_versions rule_versions_rule_id_version_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.rule_versions
    ADD CONSTRAINT rule_versions_rule_id_version_key UNIQUE (rule_id, version);


--
-- Name: rules rules_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.rules
    ADD CONSTRAINT rules_pkey PRIMARY KEY (id);


--
-- Name: saved_views saved_views_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.saved_views
    ADD CONSTRAINT saved_views_pkey PRIMARY KEY (id);


--
-- Name: saved_views saved_views_uuid_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.saved_views
    ADD CONSTRAINT saved_views_uuid_key UNIQUE (uuid);


--
-- Name: search_index_state search_index_state_entity_type_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.search_index_state
    ADD CONSTRAINT search_index_state_entity_type_key UNIQUE (entity_type);


--
-- Name: search_index_state search_index_state_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.search_index_state
    ADD CONSTRAINT search_index_state_pkey PRIMARY KEY (id);


--
-- Name: search_query_log search_query_log_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.search_query_log
    ADD CONSTRAINT search_query_log_pkey PRIMARY KEY (id);


--
-- Name: security_events security_events_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.security_events
    ADD CONSTRAINT security_events_pkey PRIMARY KEY (id);


--
-- Name: site_settings site_settings_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.site_settings
    ADD CONSTRAINT site_settings_pkey PRIMARY KEY (id);


--
-- Name: sla_policies sla_policies_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sla_policies
    ADD CONSTRAINT sla_policies_pkey PRIMARY KEY (id);


--
-- Name: sync_actions sync_actions_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_actions
    ADD CONSTRAINT sync_actions_pkey PRIMARY KEY (sync_id, occurred_at);


--
-- Name: sync_actions_2026_05 sync_actions_2026_05_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_actions_2026_05
    ADD CONSTRAINT sync_actions_2026_05_pkey PRIMARY KEY (sync_id, occurred_at);


--
-- Name: sync_actions_2026_06 sync_actions_2026_06_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_actions_2026_06
    ADD CONSTRAINT sync_actions_2026_06_pkey PRIMARY KEY (sync_id, occurred_at);


--
-- Name: sync_actions_2026_07 sync_actions_2026_07_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_actions_2026_07
    ADD CONSTRAINT sync_actions_2026_07_pkey PRIMARY KEY (sync_id, occurred_at);


--
-- Name: sync_actions_2026_08 sync_actions_2026_08_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_actions_2026_08
    ADD CONSTRAINT sync_actions_2026_08_pkey PRIMARY KEY (sync_id, occurred_at);


--
-- Name: sync_actions_default sync_actions_default_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_actions_default
    ADD CONSTRAINT sync_actions_default_pkey PRIMARY KEY (sync_id, occurred_at);


--
-- Name: sync_delta_tokens sync_delta_tokens_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_delta_tokens
    ADD CONSTRAINT sync_delta_tokens_pkey PRIMARY KEY (id);


--
-- Name: sync_delta_tokens sync_delta_tokens_workspace_provider_entity_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_delta_tokens
    ADD CONSTRAINT sync_delta_tokens_workspace_provider_entity_key UNIQUE (workspace_id, provider_type, entity_type);


--
-- Name: sync_history sync_history_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_history
    ADD CONSTRAINT sync_history_pkey PRIMARY KEY (id);


--
-- Name: system_meta system_meta_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.system_meta
    ADD CONSTRAINT system_meta_pkey PRIMARY KEY (key);


--
-- Name: tags tags_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.tags
    ADD CONSTRAINT tags_pkey PRIMARY KEY (id);


--
-- Name: tags tags_workspace_name_unique; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.tags
    ADD CONSTRAINT tags_workspace_name_unique UNIQUE (workspace_id, name);


--
-- Name: ticket_categories ticket_categories_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_categories
    ADD CONSTRAINT ticket_categories_pkey PRIMARY KEY (id);


--
-- Name: ticket_categories ticket_categories_uuid_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_categories
    ADD CONSTRAINT ticket_categories_uuid_key UNIQUE (uuid);


--
-- Name: ticket_categories ticket_categories_workspace_name_unique; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_categories
    ADD CONSTRAINT ticket_categories_workspace_name_unique UNIQUE (workspace_id, name);


--
-- Name: ticket_assets ticket_devices_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_assets
    ADD CONSTRAINT ticket_devices_pkey PRIMARY KEY (ticket_id, asset_id);


--
-- Name: ticket_rule_runs ticket_rule_runs_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_rule_runs
    ADD CONSTRAINT ticket_rule_runs_pkey PRIMARY KEY (event_id, ticket_id, rule_id);


--
-- Name: ticket_tags ticket_tags_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_tags
    ADD CONSTRAINT ticket_tags_pkey PRIMARY KEY (ticket_id, tag_id);


--
-- Name: ticket_watchers ticket_watchers_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_watchers
    ADD CONSTRAINT ticket_watchers_pkey PRIMARY KEY (ticket_id, user_uuid);


--
-- Name: tickets tickets_guest_lookup_token_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.tickets
    ADD CONSTRAINT tickets_guest_lookup_token_key UNIQUE (guest_lookup_token);


--
-- Name: tickets tickets_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.tickets
    ADD CONSTRAINT tickets_pkey PRIMARY KEY (id);


--
-- Name: user_auth_identities user_auth_identities_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.user_auth_identities
    ADD CONSTRAINT user_auth_identities_pkey PRIMARY KEY (id);


--
-- Name: user_auth_identities user_auth_identities_provider_type_external_id_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.user_auth_identities
    ADD CONSTRAINT user_auth_identities_provider_type_external_id_key UNIQUE (provider_type, external_id);


--
-- Name: user_emails user_emails_email_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.user_emails
    ADD CONSTRAINT user_emails_email_key UNIQUE (email);


--
-- Name: user_emails user_emails_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.user_emails
    ADD CONSTRAINT user_emails_pkey PRIMARY KEY (id);


--
-- Name: user_groups user_groups_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.user_groups
    ADD CONSTRAINT user_groups_pkey PRIMARY KEY (user_uuid, group_id);


--
-- Name: user_preferences user_preferences_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.user_preferences
    ADD CONSTRAINT user_preferences_pkey PRIMARY KEY (user_uuid);


--
-- Name: user_recovery_codes user_recovery_codes_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.user_recovery_codes
    ADD CONSTRAINT user_recovery_codes_pkey PRIMARY KEY (id);


--
-- Name: user_ticket_views user_ticket_views_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.user_ticket_views
    ADD CONSTRAINT user_ticket_views_pkey PRIMARY KEY (id);


--
-- Name: user_ticket_views user_ticket_views_user_uuid_ticket_id_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.user_ticket_views
    ADD CONSTRAINT user_ticket_views_user_uuid_ticket_id_key UNIQUE (user_uuid, ticket_id);


--
-- Name: users users_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_pkey PRIMARY KEY (uuid);


--
-- Name: webhook_deliveries webhook_deliveries_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.webhook_deliveries
    ADD CONSTRAINT webhook_deliveries_pkey PRIMARY KEY (id);


--
-- Name: webhook_deliveries webhook_deliveries_uuid_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.webhook_deliveries
    ADD CONSTRAINT webhook_deliveries_uuid_key UNIQUE (uuid);


--
-- Name: webhook_outbox webhook_outbox_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.webhook_outbox
    ADD CONSTRAINT webhook_outbox_pkey PRIMARY KEY (sync_id);


--
-- Name: webhooks webhooks_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.webhooks
    ADD CONSTRAINT webhooks_pkey PRIMARY KEY (id);


--
-- Name: webhooks webhooks_uuid_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.webhooks
    ADD CONSTRAINT webhooks_uuid_key UNIQUE (uuid);


--
-- Name: workflow_states workflow_states_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.workflow_states
    ADD CONSTRAINT workflow_states_pkey PRIMARY KEY (id);


--
-- Name: working_calendar_holidays working_calendar_holidays_calendar_id_date_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.working_calendar_holidays
    ADD CONSTRAINT working_calendar_holidays_calendar_id_date_key UNIQUE (calendar_id, date);


--
-- Name: working_calendar_holidays working_calendar_holidays_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.working_calendar_holidays
    ADD CONSTRAINT working_calendar_holidays_pkey PRIMARY KEY (id);


--
-- Name: working_calendars working_calendars_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.working_calendars
    ADD CONSTRAINT working_calendars_pkey PRIMARY KEY (id);


--
-- Name: workspace_members workspace_members_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.workspace_members
    ADD CONSTRAINT workspace_members_pkey PRIMARY KEY (workspace_id, user_uuid);


--
-- Name: workspaces workspaces_custom_domain_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.workspaces
    ADD CONSTRAINT workspaces_custom_domain_key UNIQUE (custom_domain);


--
-- Name: workspaces workspaces_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.workspaces
    ADD CONSTRAINT workspaces_pkey PRIMARY KEY (id);


--
-- Name: workspaces workspaces_slug_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.workspaces
    ADD CONSTRAINT workspaces_slug_key UNIQUE (slug);


--
-- Name: workspaces workspaces_uuid_key; Type: CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.workspaces
    ADD CONSTRAINT workspaces_uuid_key UNIQUE (uuid);


--
-- Name: yjs_snapshots yjs_snapshots_pkey; Type: CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.yjs_snapshots
    ADD CONSTRAINT yjs_snapshots_pkey PRIMARY KEY (id);


--
-- Name: api_tokens_platform_scoped_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX api_tokens_platform_scoped_idx ON public.api_tokens USING btree (id) WHERE (is_platform_scoped = true);


--
-- Name: audit_log_actor_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX audit_log_actor_idx ON ONLY public.audit_log USING btree (actor_uuid, occurred_at DESC) WHERE (actor_uuid IS NOT NULL);


--
-- Name: audit_log_2026_05_actor_uuid_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX audit_log_2026_05_actor_uuid_occurred_at_idx ON public.audit_log_2026_05 USING btree (actor_uuid, occurred_at DESC) WHERE (actor_uuid IS NOT NULL);


--
-- Name: audit_log_occurred_at_brin; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX audit_log_occurred_at_brin ON ONLY public.audit_log USING brin (occurred_at);


--
-- Name: audit_log_2026_05_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX audit_log_2026_05_occurred_at_idx ON public.audit_log_2026_05 USING brin (occurred_at);


--
-- Name: audit_log_table_pk_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX audit_log_table_pk_idx ON ONLY public.audit_log USING btree (table_name, pk_text, occurred_at DESC);


--
-- Name: audit_log_2026_05_table_name_pk_text_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX audit_log_2026_05_table_name_pk_text_occurred_at_idx ON public.audit_log_2026_05 USING btree (table_name, pk_text, occurred_at DESC);


--
-- Name: audit_log_2026_06_actor_uuid_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX audit_log_2026_06_actor_uuid_occurred_at_idx ON public.audit_log_2026_06 USING btree (actor_uuid, occurred_at DESC) WHERE (actor_uuid IS NOT NULL);


--
-- Name: audit_log_2026_06_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX audit_log_2026_06_occurred_at_idx ON public.audit_log_2026_06 USING brin (occurred_at);


--
-- Name: audit_log_2026_06_table_name_pk_text_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX audit_log_2026_06_table_name_pk_text_occurred_at_idx ON public.audit_log_2026_06 USING btree (table_name, pk_text, occurred_at DESC);


--
-- Name: audit_log_2026_07_actor_uuid_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX audit_log_2026_07_actor_uuid_occurred_at_idx ON public.audit_log_2026_07 USING btree (actor_uuid, occurred_at DESC) WHERE (actor_uuid IS NOT NULL);


--
-- Name: audit_log_2026_07_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX audit_log_2026_07_occurred_at_idx ON public.audit_log_2026_07 USING brin (occurred_at);


--
-- Name: audit_log_2026_07_table_name_pk_text_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX audit_log_2026_07_table_name_pk_text_occurred_at_idx ON public.audit_log_2026_07 USING btree (table_name, pk_text, occurred_at DESC);


--
-- Name: audit_log_2026_08_actor_uuid_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX audit_log_2026_08_actor_uuid_occurred_at_idx ON public.audit_log_2026_08 USING btree (actor_uuid, occurred_at DESC) WHERE (actor_uuid IS NOT NULL);


--
-- Name: audit_log_2026_08_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX audit_log_2026_08_occurred_at_idx ON public.audit_log_2026_08 USING brin (occurred_at);


--
-- Name: audit_log_2026_08_table_name_pk_text_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX audit_log_2026_08_table_name_pk_text_occurred_at_idx ON public.audit_log_2026_08 USING btree (table_name, pk_text, occurred_at DESC);


--
-- Name: audit_log_default_actor_uuid_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX audit_log_default_actor_uuid_occurred_at_idx ON public.audit_log_default USING btree (actor_uuid, occurred_at DESC) WHERE (actor_uuid IS NOT NULL);


--
-- Name: audit_log_default_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX audit_log_default_occurred_at_idx ON public.audit_log_default USING brin (occurred_at);


--
-- Name: audit_log_default_table_name_pk_text_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX audit_log_default_table_name_pk_text_occurred_at_idx ON public.audit_log_default USING btree (table_name, pk_text, occurred_at DESC);


--
-- Name: bug_reports_session_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX bug_reports_session_idx ON public.bug_reports USING btree (session_id);


--
-- Name: bug_reports_user_occurred_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX bug_reports_user_occurred_idx ON public.bug_reports USING btree (user_uuid, occurred_at DESC) WHERE (user_uuid IS NOT NULL);


--
-- Name: bug_reports_workspace_occurred_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX bug_reports_workspace_occurred_idx ON public.bug_reports USING btree (workspace_id, occurred_at DESC);


--
-- Name: canned_response_insertions_response_time_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX canned_response_insertions_response_time_idx ON public.canned_response_insertions USING btree (canned_response_id, inserted_at DESC);


--
-- Name: canned_responses_title_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX canned_responses_title_idx ON public.canned_responses USING btree (lower((title)::text));


--
-- Name: channel_credentials_kek_id_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX channel_credentials_kek_id_idx ON public.channel_credentials USING btree (encrypted_kek_id);


--
-- Name: csp_reports_dedup_hash_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX csp_reports_dedup_hash_idx ON public.csp_reports USING btree (workspace_id, dedup_hash);


--
-- Name: csp_reports_effective_directive_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX csp_reports_effective_directive_idx ON public.csp_reports USING btree (effective_directive, last_seen_at DESC);


--
-- Name: csp_reports_last_seen_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX csp_reports_last_seen_at_idx ON public.csp_reports USING btree (last_seen_at DESC);


--
-- Name: cycle_tickets_one_per_ticket; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX cycle_tickets_one_per_ticket ON public.cycle_tickets USING btree (ticket_id);


--
-- Name: cycle_tickets_ticket_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX cycle_tickets_ticket_idx ON public.cycle_tickets USING btree (ticket_id);


--
-- Name: cycles_one_active_per_project; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX cycles_one_active_per_project ON public.cycles USING btree (project_id) WHERE (((state)::text = 'active'::text) AND (archived_at IS NULL));


--
-- Name: cycles_project_state_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX cycles_project_state_idx ON public.cycles USING btree (project_id, state) WHERE (archived_at IS NULL);


--
-- Name: cycles_span_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX cycles_span_idx ON public.cycles USING btree (start_at, end_at);


--
-- Name: email_suppressions_created_idx; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX email_suppressions_created_idx ON public.email_suppressions USING btree (created_at DESC);


--
-- Name: idempotency_keys_created_at_idx; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idempotency_keys_created_at_idx ON public.idempotency_keys USING btree (created_at);


--
-- Name: idx_active_sessions_expires_at; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_active_sessions_expires_at ON public.active_sessions USING btree (expires_at);


--
-- Name: idx_active_sessions_ip_address; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_active_sessions_ip_address ON public.active_sessions USING btree (ip_address);


--
-- Name: idx_active_sessions_last_active; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_active_sessions_last_active ON public.active_sessions USING btree (last_active);


--
-- Name: idx_active_sessions_session_id; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_active_sessions_session_id ON public.active_sessions USING btree (session_id);


--
-- Name: idx_active_sessions_user_uuid; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_active_sessions_user_uuid ON public.active_sessions USING btree (user_uuid);


--
-- Name: idx_api_tokens_revoked_at; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_api_tokens_revoked_at ON public.api_tokens USING btree (revoked_at) WHERE (revoked_at IS NULL);


--
-- Name: idx_api_tokens_token_hash; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_api_tokens_token_hash ON public.api_tokens USING btree (token_hash);


--
-- Name: idx_api_tokens_user_uuid; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_api_tokens_user_uuid ON public.api_tokens USING btree (user_uuid);


--
-- Name: idx_article_content_revisions_article_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_article_content_revisions_article_id ON public.article_content_revisions USING btree (article_content_id);


--
-- Name: idx_article_content_revisions_contributors; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_article_content_revisions_contributors ON public.article_content_revisions USING gin (contributed_by);


--
-- Name: idx_asset_audits_asset; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_asset_audits_asset ON public.asset_audits USING btree (asset_id, recorded_at DESC);


--
-- Name: idx_asset_groups_asset; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_asset_groups_asset ON public.asset_groups USING btree (asset_id);


--
-- Name: idx_asset_groups_external; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_asset_groups_external ON public.asset_groups USING btree (external_source);


--
-- Name: idx_asset_groups_group; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_asset_groups_group ON public.asset_groups USING btree (group_id);


--
-- Name: idx_asset_serial_unique; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX idx_asset_serial_unique ON public.assets USING btree (serial_number) WHERE (serial_number IS NOT NULL);


--
-- Name: idx_asset_usage_log_asset; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_asset_usage_log_asset ON public.asset_usage_log USING btree (asset_id, recorded_at DESC);


--
-- Name: idx_asset_usage_log_ticket; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_asset_usage_log_ticket ON public.asset_usage_log USING btree (ticket_id) WHERE (ticket_id IS NOT NULL);


--
-- Name: idx_assets_asset_tag; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_assets_asset_tag ON public.assets USING btree (asset_tag) WHERE (asset_tag IS NOT NULL);


--
-- Name: idx_assets_created_at; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_assets_created_at ON public.assets USING btree (created_at DESC);


--
-- Name: idx_assets_kind; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_assets_kind ON public.assets USING btree (kind);


--
-- Name: idx_assets_primary_user; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_assets_primary_user ON public.assets USING btree (primary_user_uuid) WHERE (primary_user_uuid IS NOT NULL);


--
-- Name: idx_assets_serial_number; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_assets_serial_number ON public.assets USING btree (serial_number) WHERE (serial_number IS NOT NULL);


--
-- Name: idx_assignment_log_assigned_at; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_assignment_log_assigned_at ON public.assignment_log USING btree (assigned_at);


--
-- Name: idx_assignment_log_ticket; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_assignment_log_ticket ON public.assignment_log USING btree (ticket_id);


--
-- Name: idx_assignment_rules_category; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_assignment_rules_category ON public.assignment_rules USING btree (category_id) WHERE (is_active = true);


--
-- Name: idx_assignment_rules_priority; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_assignment_rules_priority ON public.assignment_rules USING btree (priority) WHERE (is_active = true);


--
-- Name: idx_attachments_comment_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_attachments_comment_id ON public.attachments USING btree (comment_id);


--
-- Name: idx_attachments_uploaded_by; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_attachments_uploaded_by ON public.attachments USING btree (uploaded_by);


--
-- Name: idx_backup_jobs_created_at; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_backup_jobs_created_at ON public.backup_jobs USING btree (created_at DESC);


--
-- Name: idx_backup_jobs_created_by; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_backup_jobs_created_by ON public.backup_jobs USING btree (created_by) WHERE (created_by IS NOT NULL);


--
-- Name: idx_backup_jobs_job_type; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_backup_jobs_job_type ON public.backup_jobs USING btree (job_type);


--
-- Name: idx_backup_jobs_status; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_backup_jobs_status ON public.backup_jobs USING btree (status);


--
-- Name: idx_category_visibility_category; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_category_visibility_category ON public.category_group_visibility USING btree (category_id);


--
-- Name: idx_category_visibility_group; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_category_visibility_group ON public.category_group_visibility USING btree (group_id);


--
-- Name: idx_channel_messages_external_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_channel_messages_external_id ON public.channel_messages USING btree (external_id);


--
-- Name: idx_channel_messages_ticket_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_channel_messages_ticket_id ON public.channel_messages USING btree (ticket_id);


--
-- Name: idx_channels_enabled_provider; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_channels_enabled_provider ON public.channels USING btree (provider) WHERE (enabled = true);


--
-- Name: idx_collection_pages_collection; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_collection_pages_collection ON public.documentation_collection_pages USING btree (collection_id);


--
-- Name: idx_collection_pages_page; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_collection_pages_page ON public.documentation_collection_pages USING btree (page_id);


--
-- Name: idx_collection_vis_group; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX idx_collection_vis_group ON public.documentation_collection_visibility USING btree (workspace_id, collection_id, group_id) WHERE (group_id IS NOT NULL);


--
-- Name: idx_collection_vis_user; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX idx_collection_vis_user ON public.documentation_collection_visibility USING btree (workspace_id, collection_id, user_uuid) WHERE (user_uuid IS NOT NULL);


--
-- Name: idx_collection_visibility_collection; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_collection_visibility_collection ON public.documentation_collection_visibility USING btree (collection_id);


--
-- Name: idx_collection_visibility_group; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_collection_visibility_group ON public.documentation_collection_visibility USING btree (group_id);


--
-- Name: idx_comments_ticket_created; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_comments_ticket_created ON public.comments USING btree (ticket_id, created_at DESC);


--
-- Name: idx_comments_ticket_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_comments_ticket_id ON public.comments USING btree (ticket_id);


--
-- Name: idx_comments_ticket_id_not_deleted; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_comments_ticket_id_not_deleted ON public.comments USING btree (ticket_id) WHERE (deleted_at IS NULL);


--
-- Name: idx_comments_user_uuid; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_comments_user_uuid ON public.comments USING btree (user_uuid);


--
-- Name: idx_doc_embeddings_target; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_doc_embeddings_target ON public.documentation_page_embeddings USING btree (target_page_id);


--
-- Name: idx_doc_page_tickets_ticket_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_doc_page_tickets_ticket_id ON public.documentation_page_tickets USING btree (ticket_id);


--
-- Name: idx_doc_pages_verified_at; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_doc_pages_verified_at ON public.documentation_pages USING btree (verified_at) WHERE (verified_at IS NOT NULL);


--
-- Name: idx_doc_starred_page; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_doc_starred_page ON public.documentation_starred_pages USING btree (page_id);


--
-- Name: idx_doc_starred_user; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_doc_starred_user ON public.documentation_starred_pages USING btree (user_uuid);


--
-- Name: idx_doc_subscriptions_page; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_doc_subscriptions_page ON public.documentation_subscriptions USING btree (page_id);


--
-- Name: idx_doc_subscriptions_user; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_doc_subscriptions_user ON public.documentation_subscriptions USING btree (user_uuid);


--
-- Name: idx_documentation_collections_created_by; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_documentation_collections_created_by ON public.documentation_collections USING btree (created_by);


--
-- Name: idx_documentation_collections_slug; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_documentation_collections_slug ON public.documentation_collections USING btree (slug);


--
-- Name: idx_documentation_pages_created_by; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_documentation_pages_created_by ON public.documentation_pages USING btree (created_by);


--
-- Name: idx_documentation_pages_deleted_at; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_documentation_pages_deleted_at ON public.documentation_pages USING btree (deleted_at);


--
-- Name: idx_documentation_pages_display_order; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_documentation_pages_display_order ON public.documentation_pages USING btree (display_order);


--
-- Name: idx_documentation_pages_parent_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_documentation_pages_parent_id ON public.documentation_pages USING btree (parent_id);


--
-- Name: idx_documentation_pages_slug; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_documentation_pages_slug ON public.documentation_pages USING btree (slug);


--
-- Name: idx_documentation_pages_status; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_documentation_pages_status ON public.documentation_pages USING btree (status);


--
-- Name: idx_documentation_pages_uuid; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_documentation_pages_uuid ON public.documentation_pages USING btree (uuid);


--
-- Name: idx_documentation_revisions_created_at; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_documentation_revisions_created_at ON public.documentation_revisions USING btree (created_at);


--
-- Name: idx_documentation_revisions_page_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_documentation_revisions_page_id ON public.documentation_revisions USING btree (page_id);


--
-- Name: idx_documentation_revisions_revision_number; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_documentation_revisions_revision_number ON public.documentation_revisions USING btree (page_id, revision_number);


--
-- Name: idx_group_includes_child; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_group_includes_child ON public.group_includes USING btree (child_group_id);


--
-- Name: idx_group_includes_parent; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_group_includes_parent ON public.group_includes USING btree (parent_group_id);


--
-- Name: idx_groups_external_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX idx_groups_external_id ON public.groups USING btree (external_id) WHERE (external_id IS NOT NULL);


--
-- Name: idx_groups_external_source; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_groups_external_source ON public.groups USING btree (external_source);


--
-- Name: idx_groups_group_type; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_groups_group_type ON public.groups USING btree (group_type);


--
-- Name: idx_groups_name; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_groups_name ON public.groups USING btree (name);


--
-- Name: idx_groups_uuid; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_groups_uuid ON public.groups USING btree (uuid);


--
-- Name: idx_import_jobs_created_by; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_import_jobs_created_by ON public.import_jobs USING btree (created_by, created_at DESC);


--
-- Name: idx_import_jobs_status; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_import_jobs_status ON public.import_jobs USING btree (status) WHERE ((status)::text = ANY ((ARRAY['parsed'::character varying, 'dry_run_done'::character varying, 'committing'::character varying])::text[]));


--
-- Name: idx_kg_signals_gap_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_kg_signals_gap_id ON public.knowledge_gap_signals USING btree (gap_id) WHERE (dismissed_at IS NULL);


--
-- Name: idx_kg_signals_source; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_kg_signals_source ON public.knowledge_gap_signals USING btree (source_kind, source_ref) WHERE (dismissed_at IS NULL);


--
-- Name: idx_knowledge_gaps_active; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_knowledge_gaps_active ON public.knowledge_gaps USING btree (status, impact_score DESC, last_evidence_at DESC) WHERE ((status)::text = ANY ((ARRAY['open'::character varying, 'drafting'::character varying])::text[]));


--
-- Name: idx_linked_tickets_linked_ticket_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_linked_tickets_linked_ticket_id ON public.linked_tickets USING btree (linked_ticket_id);


--
-- Name: idx_linked_tickets_relation_type; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_linked_tickets_relation_type ON public.linked_tickets USING btree (relation_type);


--
-- Name: idx_linked_tickets_ticket_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_linked_tickets_ticket_id ON public.linked_tickets USING btree (ticket_id);


--
-- Name: idx_notification_preferences_lookup; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_notification_preferences_lookup ON public.notification_preferences USING btree (user_uuid, notification_type_id, channel);


--
-- Name: idx_notification_preferences_user; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_notification_preferences_user ON public.notification_preferences USING btree (user_uuid);


--
-- Name: idx_notification_rate_limits_lookup; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_notification_rate_limits_lookup ON public.notification_rate_limits USING btree (user_uuid, notification_type_id, entity_type, entity_id);


--
-- Name: idx_notifications_entity; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_notifications_entity ON public.notifications USING btree (entity_type, entity_id);


--
-- Name: idx_notifications_user_created; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_notifications_user_created ON public.notifications USING btree (user_uuid, created_at DESC);


--
-- Name: idx_notifications_user_unread; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_notifications_user_unread ON public.notifications USING btree (user_uuid, is_read) WHERE (is_read = false);


--
-- Name: idx_page_vis_group; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX idx_page_vis_group ON public.documentation_page_visibility USING btree (workspace_id, page_id, group_id) WHERE (group_id IS NOT NULL);


--
-- Name: idx_page_vis_user; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX idx_page_vis_user ON public.documentation_page_visibility USING btree (workspace_id, page_id, user_uuid) WHERE (user_uuid IS NOT NULL);


--
-- Name: idx_page_visibility_group; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_page_visibility_group ON public.documentation_page_visibility USING btree (group_id);


--
-- Name: idx_page_visibility_page; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_page_visibility_page ON public.documentation_page_visibility USING btree (page_id);


--
-- Name: idx_passkey_credentials_user_uuid; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_passkey_credentials_user_uuid ON public.passkey_credentials USING btree (user_uuid);


--
-- Name: idx_pcrows_data; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_pcrows_data ON public.plugin_collection_rows USING gin (data);


--
-- Name: idx_pcrows_plugin; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_pcrows_plugin ON public.plugin_collection_rows USING btree (plugin_id);


--
-- Name: idx_pcrows_schema; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_pcrows_schema ON public.plugin_collection_rows USING btree (schema_id);


--
-- Name: idx_pcschemas_plugin; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_pcschemas_plugin ON public.plugin_collection_schemas USING btree (plugin_id);


--
-- Name: idx_plugin_activity_created_at; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_plugin_activity_created_at ON public.plugin_activity USING btree (created_at DESC);


--
-- Name: idx_plugin_activity_plugin_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_plugin_activity_plugin_id ON public.plugin_activity USING btree (plugin_id);


--
-- Name: idx_plugin_data_plugin_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_plugin_data_plugin_id ON public.plugin_data USING btree (plugin_id);


--
-- Name: idx_plugin_data_plugin_type; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_plugin_data_plugin_type ON public.plugin_data USING btree (plugin_id, data_type);


--
-- Name: idx_plugin_data_type; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_plugin_data_type ON public.plugin_data USING btree (data_type);


--
-- Name: idx_plugin_trusted_publishers_pubkey; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_plugin_trusted_publishers_pubkey ON public.plugin_trusted_publishers USING btree (pubkey);


--
-- Name: idx_plugins_name; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_plugins_name ON public.plugins USING btree (name);


--
-- Name: idx_plugins_source; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_plugins_source ON public.plugins USING btree (source);


--
-- Name: idx_plugins_state; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_plugins_state ON public.plugins USING btree (state);


--
-- Name: idx_project_tickets_order; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_project_tickets_order ON public.project_tickets USING btree (project_id, display_order);


--
-- Name: idx_project_tickets_project_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_project_tickets_project_id ON public.project_tickets USING btree (project_id);


--
-- Name: idx_project_tickets_ticket_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_project_tickets_ticket_id ON public.project_tickets USING btree (ticket_id);


--
-- Name: idx_projects_created_at; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_projects_created_at ON public.projects USING btree (created_at DESC);


--
-- Name: idx_projects_owner; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_projects_owner ON public.projects USING btree (owner_uuid) WHERE (owner_uuid IS NOT NULL);


--
-- Name: idx_projects_status; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_projects_status ON public.projects USING btree (status);


--
-- Name: idx_refresh_tokens_expires_at; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_refresh_tokens_expires_at ON public.refresh_tokens USING btree (expires_at);


--
-- Name: idx_refresh_tokens_family_id; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_refresh_tokens_family_id ON public.refresh_tokens USING btree (family_id);


--
-- Name: idx_refresh_tokens_session_id; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_refresh_tokens_session_id ON public.refresh_tokens USING btree (session_id);


--
-- Name: idx_refresh_tokens_token_hash; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_refresh_tokens_token_hash ON public.refresh_tokens USING btree (token_hash);


--
-- Name: idx_refresh_tokens_user_uuid; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_refresh_tokens_user_uuid ON public.refresh_tokens USING btree (user_uuid);


--
-- Name: idx_reset_tokens_created_at; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_reset_tokens_created_at ON public.reset_tokens USING btree (created_at);


--
-- Name: idx_reset_tokens_expires_at; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_reset_tokens_expires_at ON public.reset_tokens USING btree (expires_at);


--
-- Name: idx_reset_tokens_is_used; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_reset_tokens_is_used ON public.reset_tokens USING btree (is_used);


--
-- Name: idx_reset_tokens_token_type; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_reset_tokens_token_type ON public.reset_tokens USING btree (token_type);


--
-- Name: idx_reset_tokens_user_type; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_reset_tokens_user_type ON public.reset_tokens USING btree (user_uuid, token_type);


--
-- Name: idx_reset_tokens_user_uuid; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_reset_tokens_user_uuid ON public.reset_tokens USING btree (user_uuid);


--
-- Name: idx_saved_views_user_dataset; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_saved_views_user_dataset ON public.saved_views USING btree (created_by, dataset) WHERE ((scope)::text = 'private'::text);


--
-- Name: idx_search_index_state_entity_type; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_search_index_state_entity_type ON public.search_index_state USING btree (entity_type);


--
-- Name: idx_search_query_log_failed; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_search_query_log_failed ON public.search_query_log USING btree (query_norm, searched_at) WHERE (result_count = 0);


--
-- Name: idx_search_query_log_searched_at; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_search_query_log_searched_at ON public.search_query_log USING btree (searched_at);


--
-- Name: idx_security_events_created_at; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_security_events_created_at ON public.security_events USING btree (created_at);


--
-- Name: idx_security_events_event_type; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_security_events_event_type ON public.security_events USING btree (event_type);


--
-- Name: idx_security_events_ip_address; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_security_events_ip_address ON public.security_events USING btree (ip_address);


--
-- Name: idx_security_events_session_id; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_security_events_session_id ON public.security_events USING btree (session_id);


--
-- Name: idx_security_events_severity; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_security_events_severity ON public.security_events USING btree (severity);


--
-- Name: idx_security_events_user_created; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_security_events_user_created ON public.security_events USING btree (user_uuid, created_at DESC);


--
-- Name: idx_security_events_user_uuid; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_security_events_user_uuid ON public.security_events USING btree (user_uuid);


--
-- Name: idx_sync_delta_tokens_provider; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_sync_delta_tokens_provider ON public.sync_delta_tokens USING btree (provider_type);


--
-- Name: idx_ticket_categories_active; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_ticket_categories_active ON public.ticket_categories USING btree (is_active);


--
-- Name: idx_ticket_categories_order; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_ticket_categories_order ON public.ticket_categories USING btree (display_order);


--
-- Name: idx_ticket_categories_uuid; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_ticket_categories_uuid ON public.ticket_categories USING btree (uuid);


--
-- Name: idx_tickets_assignee; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_tickets_assignee ON public.tickets USING btree (assignee_uuid) WHERE (assignee_uuid IS NOT NULL);


--
-- Name: idx_tickets_category; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_tickets_category ON public.tickets USING btree (category_id);


--
-- Name: idx_tickets_closed_at; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_tickets_closed_at ON public.tickets USING btree (closed_at DESC) WHERE (closed_at IS NOT NULL);


--
-- Name: idx_tickets_created_at; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_tickets_created_at ON public.tickets USING btree (created_at DESC);


--
-- Name: idx_tickets_guest_lookup_token; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_tickets_guest_lookup_token ON public.tickets USING btree (guest_lookup_token) WHERE (guest_lookup_token IS NOT NULL);


--
-- Name: idx_tickets_origin_channel_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_tickets_origin_channel_id ON public.tickets USING btree (origin_channel_id) WHERE (origin_channel_id IS NOT NULL);


--
-- Name: idx_tickets_priority; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_tickets_priority ON public.tickets USING btree (priority);


--
-- Name: idx_tickets_requester; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_tickets_requester ON public.tickets USING btree (requester_uuid) WHERE (requester_uuid IS NOT NULL);


--
-- Name: idx_tickets_verification_state_pending; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_tickets_verification_state_pending ON public.tickets USING btree (verification_state) WHERE ((verification_state)::text = 'pending'::text);


--
-- Name: idx_user_auth_identities_external_id; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_user_auth_identities_external_id ON public.user_auth_identities USING btree (external_id);


--
-- Name: idx_user_auth_identities_provider_type; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_user_auth_identities_provider_type ON public.user_auth_identities USING btree (provider_type);


--
-- Name: idx_user_auth_identities_user_uuid; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_user_auth_identities_user_uuid ON public.user_auth_identities USING btree (user_uuid);


--
-- Name: idx_user_emails_email; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_user_emails_email ON public.user_emails USING btree (email);


--
-- Name: idx_user_emails_is_primary; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_user_emails_is_primary ON public.user_emails USING btree (user_uuid, is_primary);


--
-- Name: idx_user_emails_user_uuid; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_user_emails_user_uuid ON public.user_emails USING btree (user_uuid);


--
-- Name: idx_user_emails_verified; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_user_emails_verified ON public.user_emails USING btree (email) WHERE (is_verified = true);


--
-- Name: idx_user_groups_group; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_user_groups_group ON public.user_groups USING btree (group_id);


--
-- Name: idx_user_groups_user; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_user_groups_user ON public.user_groups USING btree (user_uuid);


--
-- Name: idx_user_ticket_views_last_viewed_at; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_user_ticket_views_last_viewed_at ON public.user_ticket_views USING btree (last_viewed_at);


--
-- Name: idx_user_ticket_views_ticket_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_user_ticket_views_ticket_id ON public.user_ticket_views USING btree (ticket_id);


--
-- Name: idx_user_ticket_views_user_last_viewed; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_user_ticket_views_user_last_viewed ON public.user_ticket_views USING btree (user_uuid, last_viewed_at DESC);


--
-- Name: idx_user_ticket_views_user_uuid; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_user_ticket_views_user_uuid ON public.user_ticket_views USING btree (user_uuid);


--
-- Name: idx_users_created_at; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_users_created_at ON public.users USING btree (created_at DESC);


--
-- Name: idx_users_deleted_at_pending; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_users_deleted_at_pending ON public.users USING btree (deleted_at) WHERE (deleted_at IS NOT NULL);


--
-- Name: idx_users_uuid; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX idx_users_uuid ON public.users USING btree (uuid);


--
-- Name: idx_webhook_deliveries_created_at; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_webhook_deliveries_created_at ON public.webhook_deliveries USING btree (created_at DESC);


--
-- Name: idx_webhook_deliveries_next_retry; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_webhook_deliveries_next_retry ON public.webhook_deliveries USING btree (next_retry_at) WHERE ((next_retry_at IS NOT NULL) AND (delivered_at IS NULL));


--
-- Name: idx_webhook_deliveries_webhook_id; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_webhook_deliveries_webhook_id ON public.webhook_deliveries USING btree (webhook_id);


--
-- Name: idx_webhooks_enabled; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_webhooks_enabled ON public.webhooks USING btree (enabled) WHERE (enabled = true);


--
-- Name: idx_webhooks_events; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_webhooks_events ON public.webhooks USING gin (events);


--
-- Name: idx_workspace_members_user; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX idx_workspace_members_user ON public.workspace_members USING btree (user_uuid);


--
-- Name: outbound_emails_bounced_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX outbound_emails_bounced_idx ON public.outbound_emails USING btree (bounced_at DESC) WHERE (bounced_at IS NOT NULL);


--
-- Name: outbound_emails_due_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX outbound_emails_due_idx ON public.outbound_emails USING btree (next_attempt_at) WHERE (status = ANY (ARRAY['pending'::text, 'failed'::text]));


--
-- Name: outbound_emails_idempotency_key_uidx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX outbound_emails_idempotency_key_uidx ON public.outbound_emails USING btree (workspace_id, idempotency_key) WHERE (idempotency_key IS NOT NULL);


--
-- Name: outbound_emails_lease_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX outbound_emails_lease_idx ON public.outbound_emails USING btree (lease_expires_at) WHERE (status = 'sending'::text);


--
-- Name: outbound_emails_status_smtp_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX outbound_emails_status_smtp_idx ON public.outbound_emails USING btree (status, last_smtp_code) WHERE (status = ANY (ARRAY['failed'::text, 'dead'::text]));


--
-- Name: outbound_emails_ticket_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX outbound_emails_ticket_idx ON public.outbound_emails USING btree (ticket_id, created_at DESC);


--
-- Name: plugin_local_signing_key_kek_id_idx; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX plugin_local_signing_key_kek_id_idx ON public.plugin_local_signing_key USING btree (encrypted_sk_kek_id);


--
-- Name: rule_applications_failures_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX rule_applications_failures_idx ON public.rule_applications USING btree (workspace_id, applied_at DESC) WHERE (status = ANY (ARRAY['failed'::public.rule_application_status, 'suppressed_recursion_budget'::public.rule_application_status, 'suppressed_loop_guard'::public.rule_application_status]));


--
-- Name: rule_applications_rule_recent_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX rule_applications_rule_recent_idx ON public.rule_applications USING btree (rule_id, applied_at DESC);


--
-- Name: rule_applications_ticket_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX rule_applications_ticket_idx ON public.rule_applications USING btree (ticket_id, applied_at DESC);


--
-- Name: rule_applications_workspace_recent_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX rule_applications_workspace_recent_idx ON public.rule_applications USING btree (workspace_id, applied_at DESC);


--
-- Name: rule_versions_rule_recent_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX rule_versions_rule_recent_idx ON public.rule_versions USING btree (rule_id, saved_at DESC);


--
-- Name: rules_manual_pickable_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX rules_manual_pickable_idx ON public.rules USING btree (workspace_id) WHERE ((archived_at IS NULL) AND (state = 'live'::public.rule_state) AND (trigger_kind = 'manual'::public.rule_trigger_kind));


--
-- Name: rules_workspace_state_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX rules_workspace_state_idx ON public.rules USING btree (workspace_id, state) WHERE (archived_at IS NULL);


--
-- Name: rules_workspace_trigger_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX rules_workspace_trigger_idx ON public.rules USING btree (workspace_id, trigger_kind, priority) WHERE ((archived_at IS NULL) AND (state = 'live'::public.rule_state));


--
-- Name: saved_views_scope_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX saved_views_scope_idx ON public.saved_views USING btree (scope, scope_id);


--
-- Name: saved_views_viz_pickable_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX saved_views_viz_pickable_idx ON public.saved_views USING btree (workspace_id) WHERE ((viz_type)::text <> 'list'::text);


--
-- Name: sla_policies_filters_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sla_policies_filters_idx ON public.sla_policies USING btree (priority_filter, category_id_filter);


--
-- Name: sla_policies_one_default; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX sla_policies_one_default ON public.sla_policies USING btree (workspace_id, is_default) WHERE (is_default = true);


--
-- Name: sync_actions_actor_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_actor_idx ON ONLY public.sync_actions USING btree (actor_uuid, occurred_at DESC) WHERE (actor_uuid IS NOT NULL);


--
-- Name: sync_actions_2026_05_actor_uuid_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_05_actor_uuid_occurred_at_idx ON public.sync_actions_2026_05 USING btree (actor_uuid, occurred_at DESC) WHERE (actor_uuid IS NOT NULL);


--
-- Name: sync_actions_aggregate_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_aggregate_idx ON ONLY public.sync_actions USING btree (aggregate, aggregate_id, occurred_at DESC);


--
-- Name: sync_actions_2026_05_aggregate_aggregate_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_05_aggregate_aggregate_id_occurred_at_idx ON public.sync_actions_2026_05 USING btree (aggregate, aggregate_id, occurred_at DESC);


--
-- Name: sync_actions_client_tx_id_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX sync_actions_client_tx_id_idx ON ONLY public.sync_actions USING btree (client_tx_id, occurred_at) WHERE (client_tx_id IS NOT NULL);


--
-- Name: sync_actions_2026_05_client_tx_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX sync_actions_2026_05_client_tx_id_occurred_at_idx ON public.sync_actions_2026_05 USING btree (client_tx_id, occurred_at) WHERE (client_tx_id IS NOT NULL);


--
-- Name: sync_actions_correlation_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_correlation_idx ON ONLY public.sync_actions USING btree (correlation_id) WHERE (correlation_id IS NOT NULL);


--
-- Name: sync_actions_2026_05_correlation_id_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_05_correlation_id_idx ON public.sync_actions_2026_05 USING btree (correlation_id) WHERE (correlation_id IS NOT NULL);


--
-- Name: sync_actions_groups_gin; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_groups_gin ON ONLY public.sync_actions USING gin (groups);


--
-- Name: sync_actions_2026_05_groups_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_05_groups_idx ON public.sync_actions_2026_05 USING gin (groups);


--
-- Name: sync_actions_occurred_at_brin; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_occurred_at_brin ON ONLY public.sync_actions USING brin (occurred_at);


--
-- Name: sync_actions_2026_05_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_05_occurred_at_idx ON public.sync_actions_2026_05 USING brin (occurred_at);


--
-- Name: sync_actions_2026_06_actor_uuid_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_06_actor_uuid_occurred_at_idx ON public.sync_actions_2026_06 USING btree (actor_uuid, occurred_at DESC) WHERE (actor_uuid IS NOT NULL);


--
-- Name: sync_actions_2026_06_aggregate_aggregate_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_06_aggregate_aggregate_id_occurred_at_idx ON public.sync_actions_2026_06 USING btree (aggregate, aggregate_id, occurred_at DESC);


--
-- Name: sync_actions_2026_06_client_tx_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX sync_actions_2026_06_client_tx_id_occurred_at_idx ON public.sync_actions_2026_06 USING btree (client_tx_id, occurred_at) WHERE (client_tx_id IS NOT NULL);


--
-- Name: sync_actions_2026_06_correlation_id_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_06_correlation_id_idx ON public.sync_actions_2026_06 USING btree (correlation_id) WHERE (correlation_id IS NOT NULL);


--
-- Name: sync_actions_2026_06_groups_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_06_groups_idx ON public.sync_actions_2026_06 USING gin (groups);


--
-- Name: sync_actions_2026_06_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_06_occurred_at_idx ON public.sync_actions_2026_06 USING brin (occurred_at);


--
-- Name: sync_actions_2026_07_actor_uuid_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_07_actor_uuid_occurred_at_idx ON public.sync_actions_2026_07 USING btree (actor_uuid, occurred_at DESC) WHERE (actor_uuid IS NOT NULL);


--
-- Name: sync_actions_2026_07_aggregate_aggregate_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_07_aggregate_aggregate_id_occurred_at_idx ON public.sync_actions_2026_07 USING btree (aggregate, aggregate_id, occurred_at DESC);


--
-- Name: sync_actions_2026_07_client_tx_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX sync_actions_2026_07_client_tx_id_occurred_at_idx ON public.sync_actions_2026_07 USING btree (client_tx_id, occurred_at) WHERE (client_tx_id IS NOT NULL);


--
-- Name: sync_actions_2026_07_correlation_id_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_07_correlation_id_idx ON public.sync_actions_2026_07 USING btree (correlation_id) WHERE (correlation_id IS NOT NULL);


--
-- Name: sync_actions_2026_07_groups_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_07_groups_idx ON public.sync_actions_2026_07 USING gin (groups);


--
-- Name: sync_actions_2026_07_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_07_occurred_at_idx ON public.sync_actions_2026_07 USING brin (occurred_at);


--
-- Name: sync_actions_2026_08_actor_uuid_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_08_actor_uuid_occurred_at_idx ON public.sync_actions_2026_08 USING btree (actor_uuid, occurred_at DESC) WHERE (actor_uuid IS NOT NULL);


--
-- Name: sync_actions_2026_08_aggregate_aggregate_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_08_aggregate_aggregate_id_occurred_at_idx ON public.sync_actions_2026_08 USING btree (aggregate, aggregate_id, occurred_at DESC);


--
-- Name: sync_actions_2026_08_client_tx_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX sync_actions_2026_08_client_tx_id_occurred_at_idx ON public.sync_actions_2026_08 USING btree (client_tx_id, occurred_at) WHERE (client_tx_id IS NOT NULL);


--
-- Name: sync_actions_2026_08_correlation_id_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_08_correlation_id_idx ON public.sync_actions_2026_08 USING btree (correlation_id) WHERE (correlation_id IS NOT NULL);


--
-- Name: sync_actions_2026_08_groups_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_08_groups_idx ON public.sync_actions_2026_08 USING gin (groups);


--
-- Name: sync_actions_2026_08_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_2026_08_occurred_at_idx ON public.sync_actions_2026_08 USING brin (occurred_at);


--
-- Name: sync_actions_default_actor_uuid_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_default_actor_uuid_occurred_at_idx ON public.sync_actions_default USING btree (actor_uuid, occurred_at DESC) WHERE (actor_uuid IS NOT NULL);


--
-- Name: sync_actions_default_aggregate_aggregate_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_default_aggregate_aggregate_id_occurred_at_idx ON public.sync_actions_default USING btree (aggregate, aggregate_id, occurred_at DESC);


--
-- Name: sync_actions_default_client_tx_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX sync_actions_default_client_tx_id_occurred_at_idx ON public.sync_actions_default USING btree (client_tx_id, occurred_at) WHERE (client_tx_id IS NOT NULL);


--
-- Name: sync_actions_default_correlation_id_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_default_correlation_id_idx ON public.sync_actions_default USING btree (correlation_id) WHERE (correlation_id IS NOT NULL);


--
-- Name: sync_actions_default_groups_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_default_groups_idx ON public.sync_actions_default USING gin (groups);


--
-- Name: sync_actions_default_occurred_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX sync_actions_default_occurred_at_idx ON public.sync_actions_default USING brin (occurred_at);


--
-- Name: ticket_rule_runs_age_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX ticket_rule_runs_age_idx ON public.ticket_rule_runs USING btree (fired_at);


--
-- Name: ticket_tags_tag_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX ticket_tags_tag_idx ON public.ticket_tags USING btree (tag_id);


--
-- Name: ticket_watchers_user_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX ticket_watchers_user_idx ON public.ticket_watchers USING btree (user_uuid);


--
-- Name: tickets_due_date_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX tickets_due_date_idx ON public.tickets USING btree (due_date) WHERE (due_date IS NOT NULL);


--
-- Name: tickets_first_response_at_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX tickets_first_response_at_idx ON public.tickets USING btree (first_response_at) WHERE (first_response_at IS NULL);


--
-- Name: tickets_merged_into_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX tickets_merged_into_idx ON public.tickets USING btree (merged_into_ticket_id) WHERE (merged_into_ticket_id IS NOT NULL);


--
-- Name: tickets_recurrence_rule_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX tickets_recurrence_rule_idx ON public.tickets USING btree (id) WHERE (recurrence_rule IS NOT NULL);


--
-- Name: tickets_sla_resolution_scan_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX tickets_sla_resolution_scan_idx ON public.tickets USING btree (sla_resolution_target_at) WHERE ((sla_resolution_target_at IS NOT NULL) AND (sla_resolution_breached_at IS NULL));


--
-- Name: tickets_sla_response_scan_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX tickets_sla_response_scan_idx ON public.tickets USING btree (sla_response_target_at) WHERE ((sla_response_target_at IS NOT NULL) AND (sla_response_breached_at IS NULL));


--
-- Name: tickets_untriaged_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX tickets_untriaged_idx ON public.tickets USING btree (id) WHERE ((triage_state)::text = 'untriaged'::text);


--
-- Name: tickets_workflow_state; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX tickets_workflow_state ON public.tickets USING btree (workflow_state_id);


--
-- Name: user_emails_one_primary_per_user; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE UNIQUE INDEX user_emails_one_primary_per_user ON public.user_emails USING btree (user_uuid) WHERE (is_primary = true);


--
-- Name: user_recovery_codes_unused_by_user; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX user_recovery_codes_unused_by_user ON public.user_recovery_codes USING btree (user_uuid) WHERE (used_at IS NULL);


--
-- Name: users_mfa_secret_kek_id_idx; Type: INDEX; Schema: public; Owner: nosdesk
--

CREATE INDEX users_mfa_secret_kek_id_idx ON public.users USING btree (mfa_secret_kek_id) WHERE (mfa_secret IS NOT NULL);


--
-- Name: workflow_states_category; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX workflow_states_category ON public.workflow_states USING btree (category) WHERE (archived_at IS NULL);


--
-- Name: workflow_states_category_position; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX workflow_states_category_position ON public.workflow_states USING btree (workspace_id, category, "position") WHERE (archived_at IS NULL);


--
-- Name: workflow_states_default_unique; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX workflow_states_default_unique ON public.workflow_states USING btree (workspace_id, is_default) WHERE (is_default = true);


--
-- Name: working_calendar_holidays_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX working_calendar_holidays_idx ON public.working_calendar_holidays USING btree (calendar_id, date);


--
-- Name: working_calendars_one_default; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE UNIQUE INDEX working_calendars_one_default ON public.working_calendars USING btree (workspace_id, is_default) WHERE (is_default = true);


--
-- Name: yjs_snapshots_lookup_idx; Type: INDEX; Schema: public; Owner: nosdesk_admin
--

CREATE INDEX yjs_snapshots_lookup_idx ON public.yjs_snapshots USING btree (workspace_id, document_id, created_at DESC);


--
-- Name: audit_log_2026_05_actor_uuid_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_actor_idx ATTACH PARTITION public.audit_log_2026_05_actor_uuid_occurred_at_idx;


--
-- Name: audit_log_2026_05_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_occurred_at_brin ATTACH PARTITION public.audit_log_2026_05_occurred_at_idx;


--
-- Name: audit_log_2026_05_pkey; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_pkey ATTACH PARTITION public.audit_log_2026_05_pkey;


--
-- Name: audit_log_2026_05_table_name_pk_text_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_table_pk_idx ATTACH PARTITION public.audit_log_2026_05_table_name_pk_text_occurred_at_idx;


--
-- Name: audit_log_2026_06_actor_uuid_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_actor_idx ATTACH PARTITION public.audit_log_2026_06_actor_uuid_occurred_at_idx;


--
-- Name: audit_log_2026_06_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_occurred_at_brin ATTACH PARTITION public.audit_log_2026_06_occurred_at_idx;


--
-- Name: audit_log_2026_06_pkey; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_pkey ATTACH PARTITION public.audit_log_2026_06_pkey;


--
-- Name: audit_log_2026_06_table_name_pk_text_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_table_pk_idx ATTACH PARTITION public.audit_log_2026_06_table_name_pk_text_occurred_at_idx;


--
-- Name: audit_log_2026_07_actor_uuid_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_actor_idx ATTACH PARTITION public.audit_log_2026_07_actor_uuid_occurred_at_idx;


--
-- Name: audit_log_2026_07_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_occurred_at_brin ATTACH PARTITION public.audit_log_2026_07_occurred_at_idx;


--
-- Name: audit_log_2026_07_pkey; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_pkey ATTACH PARTITION public.audit_log_2026_07_pkey;


--
-- Name: audit_log_2026_07_table_name_pk_text_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_table_pk_idx ATTACH PARTITION public.audit_log_2026_07_table_name_pk_text_occurred_at_idx;


--
-- Name: audit_log_2026_08_actor_uuid_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_actor_idx ATTACH PARTITION public.audit_log_2026_08_actor_uuid_occurred_at_idx;


--
-- Name: audit_log_2026_08_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_occurred_at_brin ATTACH PARTITION public.audit_log_2026_08_occurred_at_idx;


--
-- Name: audit_log_2026_08_pkey; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_pkey ATTACH PARTITION public.audit_log_2026_08_pkey;


--
-- Name: audit_log_2026_08_table_name_pk_text_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_table_pk_idx ATTACH PARTITION public.audit_log_2026_08_table_name_pk_text_occurred_at_idx;


--
-- Name: audit_log_default_actor_uuid_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_actor_idx ATTACH PARTITION public.audit_log_default_actor_uuid_occurred_at_idx;


--
-- Name: audit_log_default_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_occurred_at_brin ATTACH PARTITION public.audit_log_default_occurred_at_idx;


--
-- Name: audit_log_default_pkey; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_pkey ATTACH PARTITION public.audit_log_default_pkey;


--
-- Name: audit_log_default_table_name_pk_text_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.audit_log_table_pk_idx ATTACH PARTITION public.audit_log_default_table_name_pk_text_occurred_at_idx;


--
-- Name: sync_actions_2026_05_actor_uuid_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_actor_idx ATTACH PARTITION public.sync_actions_2026_05_actor_uuid_occurred_at_idx;


--
-- Name: sync_actions_2026_05_aggregate_aggregate_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_aggregate_idx ATTACH PARTITION public.sync_actions_2026_05_aggregate_aggregate_id_occurred_at_idx;


--
-- Name: sync_actions_2026_05_client_tx_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_client_tx_id_idx ATTACH PARTITION public.sync_actions_2026_05_client_tx_id_occurred_at_idx;


--
-- Name: sync_actions_2026_05_correlation_id_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_correlation_idx ATTACH PARTITION public.sync_actions_2026_05_correlation_id_idx;


--
-- Name: sync_actions_2026_05_groups_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_groups_gin ATTACH PARTITION public.sync_actions_2026_05_groups_idx;


--
-- Name: sync_actions_2026_05_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_occurred_at_brin ATTACH PARTITION public.sync_actions_2026_05_occurred_at_idx;


--
-- Name: sync_actions_2026_05_pkey; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_pkey ATTACH PARTITION public.sync_actions_2026_05_pkey;


--
-- Name: sync_actions_2026_06_actor_uuid_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_actor_idx ATTACH PARTITION public.sync_actions_2026_06_actor_uuid_occurred_at_idx;


--
-- Name: sync_actions_2026_06_aggregate_aggregate_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_aggregate_idx ATTACH PARTITION public.sync_actions_2026_06_aggregate_aggregate_id_occurred_at_idx;


--
-- Name: sync_actions_2026_06_client_tx_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_client_tx_id_idx ATTACH PARTITION public.sync_actions_2026_06_client_tx_id_occurred_at_idx;


--
-- Name: sync_actions_2026_06_correlation_id_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_correlation_idx ATTACH PARTITION public.sync_actions_2026_06_correlation_id_idx;


--
-- Name: sync_actions_2026_06_groups_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_groups_gin ATTACH PARTITION public.sync_actions_2026_06_groups_idx;


--
-- Name: sync_actions_2026_06_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_occurred_at_brin ATTACH PARTITION public.sync_actions_2026_06_occurred_at_idx;


--
-- Name: sync_actions_2026_06_pkey; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_pkey ATTACH PARTITION public.sync_actions_2026_06_pkey;


--
-- Name: sync_actions_2026_07_actor_uuid_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_actor_idx ATTACH PARTITION public.sync_actions_2026_07_actor_uuid_occurred_at_idx;


--
-- Name: sync_actions_2026_07_aggregate_aggregate_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_aggregate_idx ATTACH PARTITION public.sync_actions_2026_07_aggregate_aggregate_id_occurred_at_idx;


--
-- Name: sync_actions_2026_07_client_tx_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_client_tx_id_idx ATTACH PARTITION public.sync_actions_2026_07_client_tx_id_occurred_at_idx;


--
-- Name: sync_actions_2026_07_correlation_id_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_correlation_idx ATTACH PARTITION public.sync_actions_2026_07_correlation_id_idx;


--
-- Name: sync_actions_2026_07_groups_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_groups_gin ATTACH PARTITION public.sync_actions_2026_07_groups_idx;


--
-- Name: sync_actions_2026_07_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_occurred_at_brin ATTACH PARTITION public.sync_actions_2026_07_occurred_at_idx;


--
-- Name: sync_actions_2026_07_pkey; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_pkey ATTACH PARTITION public.sync_actions_2026_07_pkey;


--
-- Name: sync_actions_2026_08_actor_uuid_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_actor_idx ATTACH PARTITION public.sync_actions_2026_08_actor_uuid_occurred_at_idx;


--
-- Name: sync_actions_2026_08_aggregate_aggregate_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_aggregate_idx ATTACH PARTITION public.sync_actions_2026_08_aggregate_aggregate_id_occurred_at_idx;


--
-- Name: sync_actions_2026_08_client_tx_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_client_tx_id_idx ATTACH PARTITION public.sync_actions_2026_08_client_tx_id_occurred_at_idx;


--
-- Name: sync_actions_2026_08_correlation_id_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_correlation_idx ATTACH PARTITION public.sync_actions_2026_08_correlation_id_idx;


--
-- Name: sync_actions_2026_08_groups_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_groups_gin ATTACH PARTITION public.sync_actions_2026_08_groups_idx;


--
-- Name: sync_actions_2026_08_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_occurred_at_brin ATTACH PARTITION public.sync_actions_2026_08_occurred_at_idx;


--
-- Name: sync_actions_2026_08_pkey; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_pkey ATTACH PARTITION public.sync_actions_2026_08_pkey;


--
-- Name: sync_actions_default_actor_uuid_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_actor_idx ATTACH PARTITION public.sync_actions_default_actor_uuid_occurred_at_idx;


--
-- Name: sync_actions_default_aggregate_aggregate_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_aggregate_idx ATTACH PARTITION public.sync_actions_default_aggregate_aggregate_id_occurred_at_idx;


--
-- Name: sync_actions_default_client_tx_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_client_tx_id_idx ATTACH PARTITION public.sync_actions_default_client_tx_id_occurred_at_idx;


--
-- Name: sync_actions_default_correlation_id_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_correlation_idx ATTACH PARTITION public.sync_actions_default_correlation_id_idx;


--
-- Name: sync_actions_default_groups_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_groups_gin ATTACH PARTITION public.sync_actions_default_groups_idx;


--
-- Name: sync_actions_default_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_occurred_at_brin ATTACH PARTITION public.sync_actions_default_occurred_at_idx;


--
-- Name: sync_actions_default_pkey; Type: INDEX ATTACH; Schema: public; Owner: nosdesk_admin
--

ALTER INDEX public.sync_actions_pkey ATTACH PARTITION public.sync_actions_default_pkey;


--
-- Name: rules rules_version_on_insert; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER rules_version_on_insert AFTER INSERT ON public.rules FOR EACH ROW EXECUTE FUNCTION public.rules_write_initial_version();


--
-- Name: rules rules_version_on_update; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER rules_version_on_update BEFORE UPDATE ON public.rules FOR EACH ROW WHEN (((((((((((((old.name)::text IS DISTINCT FROM (new.name)::text) OR (old.description IS DISTINCT FROM new.description)) OR (old.trigger_kind IS DISTINCT FROM new.trigger_kind)) OR (old.trigger_config IS DISTINCT FROM new.trigger_config)) OR (old.conditions IS DISTINCT FROM new.conditions)) OR (old.actions IS DISTINCT FROM new.actions)) OR (old.state IS DISTINCT FROM new.state)) OR (old.priority IS DISTINCT FROM new.priority)) OR (old.archived_at IS DISTINCT FROM new.archived_at)) OR (old.reads_set IS DISTINCT FROM new.reads_set)) OR (old.writes_set IS DISTINCT FROM new.writes_set))) EXECUTE FUNCTION public.rules_write_update_version();


--
-- Name: assets set_updated_at; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.assets FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();


--
-- Name: channels set_updated_at; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.channels FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();


--
-- Name: comments set_updated_at; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.comments FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();


--
-- Name: documentation_pages set_updated_at; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.documentation_pages FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();


--
-- Name: plugin_collection_rows set_updated_at; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.plugin_collection_rows FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();


--
-- Name: plugin_collection_schemas set_updated_at; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.plugin_collection_schemas FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();


--
-- Name: plugin_data set_updated_at; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.plugin_data FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();


--
-- Name: plugins set_updated_at; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.plugins FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();


--
-- Name: projects set_updated_at; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.projects FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();


--
-- Name: site_settings set_updated_at; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.site_settings FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();


--
-- Name: tickets set_updated_at; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.tickets FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();


--
-- Name: user_auth_identities set_updated_at; Type: TRIGGER; Schema: public; Owner: nosdesk
--

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.user_auth_identities FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();


--
-- Name: user_emails set_updated_at; Type: TRIGGER; Schema: public; Owner: nosdesk
--

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.user_emails FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();


--
-- Name: users set_updated_at; Type: TRIGGER; Schema: public; Owner: nosdesk
--

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.users FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();


--
-- Name: webhooks set_updated_at; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.webhooks FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();


--
-- Name: sync_actions sync_actions_notify_trigger; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER sync_actions_notify_trigger AFTER INSERT ON public.sync_actions FOR EACH ROW EXECUTE FUNCTION public.sync_actions_notify();


--
-- Name: asset_audits tr_audit_asset_audits; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_asset_audits AFTER INSERT OR DELETE OR UPDATE ON public.asset_audits FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: asset_kinds tr_audit_asset_kinds; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_asset_kinds AFTER INSERT OR DELETE OR UPDATE ON public.asset_kinds FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: asset_usage_log tr_audit_asset_usage_log; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_asset_usage_log AFTER INSERT OR DELETE OR UPDATE ON public.asset_usage_log FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: assets tr_audit_assets; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_assets AFTER INSERT OR DELETE OR UPDATE ON public.assets FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: assignment_rule_state tr_audit_assignment_rule_state; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_assignment_rule_state AFTER INSERT OR DELETE OR UPDATE ON public.assignment_rule_state FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('rule_id');


--
-- Name: assignment_rules tr_audit_assignment_rules; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_assignment_rules AFTER INSERT OR DELETE OR UPDATE ON public.assignment_rules FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: canned_responses tr_audit_canned_responses; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_canned_responses AFTER INSERT OR DELETE OR UPDATE ON public.canned_responses FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: category_group_visibility tr_audit_category_group_visibility; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_category_group_visibility AFTER INSERT OR DELETE OR UPDATE ON public.category_group_visibility FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('category_id');


--
-- Name: channel_credentials tr_audit_channel_credentials; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_channel_credentials AFTER INSERT OR DELETE OR UPDATE ON public.channel_credentials FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id', 'encrypted_value');


--
-- Name: documentation_collection_visibility tr_audit_documentation_collection_visibility; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_documentation_collection_visibility AFTER INSERT OR DELETE OR UPDATE ON public.documentation_collection_visibility FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: documentation_page_visibility tr_audit_documentation_page_visibility; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_documentation_page_visibility AFTER INSERT OR DELETE OR UPDATE ON public.documentation_page_visibility FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: groups tr_audit_groups; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_groups AFTER INSERT OR DELETE OR UPDATE ON public.groups FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: notification_preferences tr_audit_notification_preferences; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_notification_preferences AFTER INSERT OR DELETE OR UPDATE ON public.notification_preferences FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: plugin_collection_rows tr_audit_plugin_collection_rows; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_plugin_collection_rows AFTER INSERT OR DELETE OR UPDATE ON public.plugin_collection_rows FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: plugin_data tr_audit_plugin_data; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_plugin_data AFTER INSERT OR DELETE OR UPDATE ON public.plugin_data FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: plugin_local_signing_key tr_audit_plugin_local_signing_key; Type: TRIGGER; Schema: public; Owner: nosdesk
--

CREATE TRIGGER tr_audit_plugin_local_signing_key AFTER INSERT OR DELETE OR UPDATE ON public.plugin_local_signing_key FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: plugin_registry_state tr_audit_plugin_registry_state; Type: TRIGGER; Schema: public; Owner: nosdesk
--

CREATE TRIGGER tr_audit_plugin_registry_state AFTER INSERT OR DELETE OR UPDATE ON public.plugin_registry_state FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: plugin_trusted_publishers tr_audit_plugin_trusted_publishers_del; Type: TRIGGER; Schema: public; Owner: nosdesk
--

CREATE TRIGGER tr_audit_plugin_trusted_publishers_del AFTER DELETE ON public.plugin_trusted_publishers FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: plugin_trusted_publishers tr_audit_plugin_trusted_publishers_ins; Type: TRIGGER; Schema: public; Owner: nosdesk
--

CREATE TRIGGER tr_audit_plugin_trusted_publishers_ins AFTER INSERT ON public.plugin_trusted_publishers FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: plugin_trusted_publishers tr_audit_plugin_trusted_publishers_upd; Type: TRIGGER; Schema: public; Owner: nosdesk
--

CREATE TRIGGER tr_audit_plugin_trusted_publishers_upd AFTER UPDATE ON public.plugin_trusted_publishers FOR EACH ROW WHEN ((old.* IS DISTINCT FROM new.*)) EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: plugins tr_audit_plugins; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_plugins AFTER INSERT OR DELETE OR UPDATE ON public.plugins FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('uuid');


--
-- Name: site_settings tr_audit_site_settings; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_site_settings AFTER INSERT OR DELETE OR UPDATE ON public.site_settings FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: sla_policies tr_audit_sla_policies; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_sla_policies AFTER INSERT OR DELETE OR UPDATE ON public.sla_policies FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: ticket_categories tr_audit_ticket_categories; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_ticket_categories AFTER INSERT OR DELETE OR UPDATE ON public.ticket_categories FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: tickets tr_audit_tickets; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_tickets AFTER INSERT OR DELETE OR UPDATE ON public.tickets FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: user_ticket_views tr_audit_user_ticket_views; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_user_ticket_views AFTER INSERT OR DELETE OR UPDATE ON public.user_ticket_views FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('user_uuid');


--
-- Name: users tr_audit_users; Type: TRIGGER; Schema: public; Owner: nosdesk
--

CREATE TRIGGER tr_audit_users AFTER INSERT OR DELETE OR UPDATE ON public.users FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('uuid', 'name', 'mfa_secret', 'mfa_backup_codes');


--
-- Name: webhook_deliveries tr_audit_webhook_deliveries; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_webhook_deliveries AFTER INSERT OR DELETE OR UPDATE ON public.webhook_deliveries FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: webhooks tr_audit_webhooks; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_webhooks AFTER INSERT OR DELETE OR UPDATE ON public.webhooks FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: workflow_states tr_audit_workflow_states; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_audit_workflow_states AFTER INSERT OR DELETE OR UPDATE ON public.workflow_states FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


--
-- Name: outbound_emails tr_outbound_emails_notify; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_outbound_emails_notify AFTER INSERT ON public.outbound_emails FOR EACH ROW EXECUTE FUNCTION public.outbound_emails_notify_trigger();


--
-- Name: sync_actions tr_sync_actions_webhook_outbox; Type: TRIGGER; Schema: public; Owner: nosdesk_admin
--

CREATE TRIGGER tr_sync_actions_webhook_outbox AFTER INSERT ON public.sync_actions FOR EACH ROW EXECUTE FUNCTION public.webhook_outbox_enqueue();


--
-- Name: users trg_users_auto_create_preferences; Type: TRIGGER; Schema: public; Owner: nosdesk
--

CREATE TRIGGER trg_users_auto_create_preferences AFTER INSERT ON public.users FOR EACH ROW EXECUTE FUNCTION public.auto_create_user_preferences();


--
-- Name: active_sessions active_sessions_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.active_sessions
    ADD CONSTRAINT active_sessions_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: api_tokens api_tokens_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.api_tokens
    ADD CONSTRAINT api_tokens_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid);


--
-- Name: api_tokens api_tokens_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.api_tokens
    ADD CONSTRAINT api_tokens_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: api_tokens api_tokens_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.api_tokens
    ADD CONSTRAINT api_tokens_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: article_content_revisions article_content_revisions_article_content_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.article_content_revisions
    ADD CONSTRAINT article_content_revisions_article_content_id_fkey FOREIGN KEY (article_content_id) REFERENCES public.article_contents(id) ON DELETE CASCADE;


--
-- Name: article_content_revisions article_content_revisions_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.article_content_revisions
    ADD CONSTRAINT article_content_revisions_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: article_contents article_contents_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.article_contents
    ADD CONSTRAINT article_contents_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: article_contents article_contents_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.article_contents
    ADD CONSTRAINT article_contents_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE CASCADE;


--
-- Name: article_contents article_contents_updated_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.article_contents
    ADD CONSTRAINT article_contents_updated_by_fkey FOREIGN KEY (updated_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: article_contents article_contents_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.article_contents
    ADD CONSTRAINT article_contents_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: asset_audits asset_audits_asset_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_audits
    ADD CONSTRAINT asset_audits_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES public.assets(id) ON DELETE CASCADE;


--
-- Name: asset_audits asset_audits_recorded_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_audits
    ADD CONSTRAINT asset_audits_recorded_by_fkey FOREIGN KEY (recorded_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: asset_audits asset_audits_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_audits
    ADD CONSTRAINT asset_audits_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: asset_groups asset_groups_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_groups
    ADD CONSTRAINT asset_groups_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: asset_kinds asset_kinds_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_kinds
    ADD CONSTRAINT asset_kinds_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: asset_kinds asset_kinds_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_kinds
    ADD CONSTRAINT asset_kinds_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: asset_usage_log asset_usage_log_asset_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_usage_log
    ADD CONSTRAINT asset_usage_log_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES public.assets(id) ON DELETE CASCADE;


--
-- Name: asset_usage_log asset_usage_log_recorded_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_usage_log
    ADD CONSTRAINT asset_usage_log_recorded_by_fkey FOREIGN KEY (recorded_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: asset_usage_log asset_usage_log_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_usage_log
    ADD CONSTRAINT asset_usage_log_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE SET NULL;


--
-- Name: asset_usage_log asset_usage_log_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_usage_log
    ADD CONSTRAINT asset_usage_log_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: assets assets_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assets
    ADD CONSTRAINT assets_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: assignment_log assignment_log_rule_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assignment_log
    ADD CONSTRAINT assignment_log_rule_id_fkey FOREIGN KEY (rule_id) REFERENCES public.assignment_rules(id) ON DELETE SET NULL;


--
-- Name: assignment_log assignment_log_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assignment_log
    ADD CONSTRAINT assignment_log_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE CASCADE;


--
-- Name: assignment_log assignment_log_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assignment_log
    ADD CONSTRAINT assignment_log_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: assignment_rule_state assignment_rule_state_last_assigned_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assignment_rule_state
    ADD CONSTRAINT assignment_rule_state_last_assigned_user_uuid_fkey FOREIGN KEY (last_assigned_user_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: assignment_rule_state assignment_rule_state_rule_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assignment_rule_state
    ADD CONSTRAINT assignment_rule_state_rule_id_fkey FOREIGN KEY (rule_id) REFERENCES public.assignment_rules(id) ON DELETE CASCADE;


--
-- Name: assignment_rule_state assignment_rule_state_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assignment_rule_state
    ADD CONSTRAINT assignment_rule_state_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: assignment_rules assignment_rules_category_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assignment_rules
    ADD CONSTRAINT assignment_rules_category_id_fkey FOREIGN KEY (category_id) REFERENCES public.ticket_categories(id) ON DELETE SET NULL;


--
-- Name: assignment_rules assignment_rules_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assignment_rules
    ADD CONSTRAINT assignment_rules_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: assignment_rules assignment_rules_target_group_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assignment_rules
    ADD CONSTRAINT assignment_rules_target_group_id_fkey FOREIGN KEY (target_group_id) REFERENCES public.groups(id) ON DELETE SET NULL;


--
-- Name: assignment_rules assignment_rules_target_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assignment_rules
    ADD CONSTRAINT assignment_rules_target_user_uuid_fkey FOREIGN KEY (target_user_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: assignment_rules assignment_rules_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assignment_rules
    ADD CONSTRAINT assignment_rules_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: attachments attachments_comment_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.attachments
    ADD CONSTRAINT attachments_comment_id_fkey FOREIGN KEY (comment_id) REFERENCES public.comments(id) ON DELETE CASCADE;


--
-- Name: attachments attachments_uploaded_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.attachments
    ADD CONSTRAINT attachments_uploaded_by_fkey FOREIGN KEY (uploaded_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: attachments attachments_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.attachments
    ADD CONSTRAINT attachments_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: audit_log audit_log_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.audit_log
    ADD CONSTRAINT audit_log_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: backup_jobs backup_jobs_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.backup_jobs
    ADD CONSTRAINT backup_jobs_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: backup_jobs backup_jobs_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.backup_jobs
    ADD CONSTRAINT backup_jobs_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: bug_reports bug_reports_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.bug_reports
    ADD CONSTRAINT bug_reports_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: bug_reports bug_reports_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.bug_reports
    ADD CONSTRAINT bug_reports_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: canned_response_insertions canned_response_insertions_canned_response_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.canned_response_insertions
    ADD CONSTRAINT canned_response_insertions_canned_response_id_fkey FOREIGN KEY (canned_response_id) REFERENCES public.canned_responses(id) ON DELETE CASCADE;


--
-- Name: canned_response_insertions canned_response_insertions_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.canned_response_insertions
    ADD CONSTRAINT canned_response_insertions_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE SET NULL;


--
-- Name: canned_response_insertions canned_response_insertions_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.canned_response_insertions
    ADD CONSTRAINT canned_response_insertions_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: canned_response_insertions canned_response_insertions_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.canned_response_insertions
    ADD CONSTRAINT canned_response_insertions_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: canned_responses canned_responses_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.canned_responses
    ADD CONSTRAINT canned_responses_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: canned_responses canned_responses_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.canned_responses
    ADD CONSTRAINT canned_responses_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: category_group_visibility category_group_visibility_category_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.category_group_visibility
    ADD CONSTRAINT category_group_visibility_category_id_fkey FOREIGN KEY (category_id) REFERENCES public.ticket_categories(id) ON DELETE CASCADE;


--
-- Name: category_group_visibility category_group_visibility_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.category_group_visibility
    ADD CONSTRAINT category_group_visibility_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: category_group_visibility category_group_visibility_group_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.category_group_visibility
    ADD CONSTRAINT category_group_visibility_group_id_fkey FOREIGN KEY (group_id) REFERENCES public.groups(id) ON DELETE CASCADE;


--
-- Name: category_group_visibility category_group_visibility_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.category_group_visibility
    ADD CONSTRAINT category_group_visibility_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: channel_credentials channel_credentials_channel_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.channel_credentials
    ADD CONSTRAINT channel_credentials_channel_id_fkey FOREIGN KEY (channel_id) REFERENCES public.channels(id) ON DELETE CASCADE;


--
-- Name: channel_credentials channel_credentials_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.channel_credentials
    ADD CONSTRAINT channel_credentials_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: channel_messages channel_messages_author_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.channel_messages
    ADD CONSTRAINT channel_messages_author_user_uuid_fkey FOREIGN KEY (author_user_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: channel_messages channel_messages_channel_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.channel_messages
    ADD CONSTRAINT channel_messages_channel_id_fkey FOREIGN KEY (channel_id) REFERENCES public.channels(id) ON DELETE CASCADE;


--
-- Name: channel_messages channel_messages_comment_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.channel_messages
    ADD CONSTRAINT channel_messages_comment_id_fkey FOREIGN KEY (comment_id) REFERENCES public.comments(id) ON DELETE SET NULL;


--
-- Name: channel_messages channel_messages_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.channel_messages
    ADD CONSTRAINT channel_messages_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE SET NULL;


--
-- Name: channel_messages channel_messages_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.channel_messages
    ADD CONSTRAINT channel_messages_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: channels channels_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.channels
    ADD CONSTRAINT channels_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: comments comments_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.comments
    ADD CONSTRAINT comments_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE CASCADE;


--
-- Name: comments comments_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.comments
    ADD CONSTRAINT comments_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE RESTRICT;


--
-- Name: comments comments_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.comments
    ADD CONSTRAINT comments_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: csp_reports csp_reports_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.csp_reports
    ADD CONSTRAINT csp_reports_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: csp_reports csp_reports_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.csp_reports
    ADD CONSTRAINT csp_reports_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: cycle_tickets cycle_tickets_added_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.cycle_tickets
    ADD CONSTRAINT cycle_tickets_added_by_fkey FOREIGN KEY (added_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: cycle_tickets cycle_tickets_cycle_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.cycle_tickets
    ADD CONSTRAINT cycle_tickets_cycle_id_fkey FOREIGN KEY (cycle_id) REFERENCES public.cycles(id) ON DELETE CASCADE;


--
-- Name: cycle_tickets cycle_tickets_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.cycle_tickets
    ADD CONSTRAINT cycle_tickets_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE CASCADE;


--
-- Name: cycle_tickets cycle_tickets_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.cycle_tickets
    ADD CONSTRAINT cycle_tickets_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: cycles cycles_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.cycles
    ADD CONSTRAINT cycles_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: cycles cycles_project_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.cycles
    ADD CONSTRAINT cycles_project_id_fkey FOREIGN KEY (project_id) REFERENCES public.projects(id) ON DELETE CASCADE;


--
-- Name: cycles cycles_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.cycles
    ADD CONSTRAINT cycles_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: asset_groups device_groups_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_groups
    ADD CONSTRAINT device_groups_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: asset_groups device_groups_device_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_groups
    ADD CONSTRAINT device_groups_device_id_fkey FOREIGN KEY (asset_id) REFERENCES public.assets(id) ON DELETE CASCADE;


--
-- Name: asset_groups device_groups_group_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.asset_groups
    ADD CONSTRAINT device_groups_group_id_fkey FOREIGN KEY (group_id) REFERENCES public.groups(id) ON DELETE CASCADE;


--
-- Name: assets devices_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assets
    ADD CONSTRAINT devices_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: assets devices_primary_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.assets
    ADD CONSTRAINT devices_primary_user_uuid_fkey FOREIGN KEY (primary_user_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: documentation_collection_pages documentation_collection_pages_collection_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collection_pages
    ADD CONSTRAINT documentation_collection_pages_collection_id_fkey FOREIGN KEY (collection_id) REFERENCES public.documentation_collections(id) ON DELETE CASCADE;


--
-- Name: documentation_collection_pages documentation_collection_pages_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collection_pages
    ADD CONSTRAINT documentation_collection_pages_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: documentation_collection_pages documentation_collection_pages_page_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collection_pages
    ADD CONSTRAINT documentation_collection_pages_page_id_fkey FOREIGN KEY (page_id) REFERENCES public.documentation_pages(id) ON DELETE CASCADE;


--
-- Name: documentation_collection_pages documentation_collection_pages_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collection_pages
    ADD CONSTRAINT documentation_collection_pages_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: documentation_collection_visibility documentation_collection_visibility_collection_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collection_visibility
    ADD CONSTRAINT documentation_collection_visibility_collection_id_fkey FOREIGN KEY (collection_id) REFERENCES public.documentation_collections(id) ON DELETE CASCADE;


--
-- Name: documentation_collection_visibility documentation_collection_visibility_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collection_visibility
    ADD CONSTRAINT documentation_collection_visibility_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: documentation_collection_visibility documentation_collection_visibility_group_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collection_visibility
    ADD CONSTRAINT documentation_collection_visibility_group_id_fkey FOREIGN KEY (group_id) REFERENCES public.groups(id) ON DELETE CASCADE;


--
-- Name: documentation_collection_visibility documentation_collection_visibility_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collection_visibility
    ADD CONSTRAINT documentation_collection_visibility_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: documentation_collection_visibility documentation_collection_visibility_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collection_visibility
    ADD CONSTRAINT documentation_collection_visibility_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: documentation_collections documentation_collections_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collections
    ADD CONSTRAINT documentation_collections_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: documentation_collections documentation_collections_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_collections
    ADD CONSTRAINT documentation_collections_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: documentation_page_embeddings documentation_page_embeddings_source_page_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_page_embeddings
    ADD CONSTRAINT documentation_page_embeddings_source_page_id_fkey FOREIGN KEY (source_page_id) REFERENCES public.documentation_pages(id) ON DELETE CASCADE;


--
-- Name: documentation_page_embeddings documentation_page_embeddings_target_page_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_page_embeddings
    ADD CONSTRAINT documentation_page_embeddings_target_page_id_fkey FOREIGN KEY (target_page_id) REFERENCES public.documentation_pages(id) ON DELETE CASCADE;


--
-- Name: documentation_page_embeddings documentation_page_embeddings_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_page_embeddings
    ADD CONSTRAINT documentation_page_embeddings_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: documentation_page_tickets documentation_page_tickets_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_page_tickets
    ADD CONSTRAINT documentation_page_tickets_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: documentation_page_tickets documentation_page_tickets_page_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_page_tickets
    ADD CONSTRAINT documentation_page_tickets_page_id_fkey FOREIGN KEY (page_id) REFERENCES public.documentation_pages(id) ON DELETE CASCADE;


--
-- Name: documentation_page_tickets documentation_page_tickets_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_page_tickets
    ADD CONSTRAINT documentation_page_tickets_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE CASCADE;


--
-- Name: documentation_page_tickets documentation_page_tickets_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_page_tickets
    ADD CONSTRAINT documentation_page_tickets_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: documentation_page_visibility documentation_page_visibility_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_page_visibility
    ADD CONSTRAINT documentation_page_visibility_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: documentation_page_visibility documentation_page_visibility_group_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_page_visibility
    ADD CONSTRAINT documentation_page_visibility_group_id_fkey FOREIGN KEY (group_id) REFERENCES public.groups(id) ON DELETE CASCADE;


--
-- Name: documentation_page_visibility documentation_page_visibility_page_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_page_visibility
    ADD CONSTRAINT documentation_page_visibility_page_id_fkey FOREIGN KEY (page_id) REFERENCES public.documentation_pages(id) ON DELETE CASCADE;


--
-- Name: documentation_page_visibility documentation_page_visibility_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_page_visibility
    ADD CONSTRAINT documentation_page_visibility_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: documentation_page_visibility documentation_page_visibility_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_page_visibility
    ADD CONSTRAINT documentation_page_visibility_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: documentation_pages documentation_pages_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_pages
    ADD CONSTRAINT documentation_pages_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE RESTRICT;


--
-- Name: documentation_pages documentation_pages_last_edited_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_pages
    ADD CONSTRAINT documentation_pages_last_edited_by_fkey FOREIGN KEY (last_edited_by) REFERENCES public.users(uuid) ON DELETE RESTRICT;


--
-- Name: documentation_pages documentation_pages_parent_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_pages
    ADD CONSTRAINT documentation_pages_parent_id_fkey FOREIGN KEY (parent_id) REFERENCES public.documentation_pages(id) ON DELETE CASCADE;


--
-- Name: documentation_pages documentation_pages_verified_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_pages
    ADD CONSTRAINT documentation_pages_verified_by_fkey FOREIGN KEY (verified_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: documentation_pages documentation_pages_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_pages
    ADD CONSTRAINT documentation_pages_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: documentation_revisions documentation_revisions_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_revisions
    ADD CONSTRAINT documentation_revisions_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE RESTRICT;


--
-- Name: documentation_revisions documentation_revisions_page_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_revisions
    ADD CONSTRAINT documentation_revisions_page_id_fkey FOREIGN KEY (page_id) REFERENCES public.documentation_pages(id) ON DELETE CASCADE;


--
-- Name: documentation_revisions documentation_revisions_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_revisions
    ADD CONSTRAINT documentation_revisions_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: documentation_starred_pages documentation_starred_pages_page_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_starred_pages
    ADD CONSTRAINT documentation_starred_pages_page_id_fkey FOREIGN KEY (page_id) REFERENCES public.documentation_pages(id) ON DELETE CASCADE;


--
-- Name: documentation_starred_pages documentation_starred_pages_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_starred_pages
    ADD CONSTRAINT documentation_starred_pages_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: documentation_starred_pages documentation_starred_pages_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_starred_pages
    ADD CONSTRAINT documentation_starred_pages_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: documentation_subscriptions documentation_subscriptions_page_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_subscriptions
    ADD CONSTRAINT documentation_subscriptions_page_id_fkey FOREIGN KEY (page_id) REFERENCES public.documentation_pages(id) ON DELETE CASCADE;


--
-- Name: documentation_subscriptions documentation_subscriptions_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_subscriptions
    ADD CONSTRAINT documentation_subscriptions_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: documentation_subscriptions documentation_subscriptions_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.documentation_subscriptions
    ADD CONSTRAINT documentation_subscriptions_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: group_includes group_includes_child_group_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.group_includes
    ADD CONSTRAINT group_includes_child_group_id_fkey FOREIGN KEY (child_group_id) REFERENCES public.groups(id) ON DELETE CASCADE;


--
-- Name: group_includes group_includes_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.group_includes
    ADD CONSTRAINT group_includes_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: group_includes group_includes_parent_group_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.group_includes
    ADD CONSTRAINT group_includes_parent_group_id_fkey FOREIGN KEY (parent_group_id) REFERENCES public.groups(id) ON DELETE CASCADE;


--
-- Name: group_includes group_includes_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.group_includes
    ADD CONSTRAINT group_includes_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: groups groups_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.groups
    ADD CONSTRAINT groups_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: groups groups_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.groups
    ADD CONSTRAINT groups_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: import_jobs import_jobs_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.import_jobs
    ADD CONSTRAINT import_jobs_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: import_jobs import_jobs_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.import_jobs
    ADD CONSTRAINT import_jobs_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: knowledge_gap_signals knowledge_gap_signals_detected_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.knowledge_gap_signals
    ADD CONSTRAINT knowledge_gap_signals_detected_by_fkey FOREIGN KEY (detected_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: knowledge_gap_signals knowledge_gap_signals_dismissed_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.knowledge_gap_signals
    ADD CONSTRAINT knowledge_gap_signals_dismissed_by_fkey FOREIGN KEY (dismissed_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: knowledge_gap_signals knowledge_gap_signals_gap_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.knowledge_gap_signals
    ADD CONSTRAINT knowledge_gap_signals_gap_id_fkey FOREIGN KEY (gap_id) REFERENCES public.knowledge_gaps(id) ON DELETE CASCADE;


--
-- Name: knowledge_gap_signals knowledge_gap_signals_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.knowledge_gap_signals
    ADD CONSTRAINT knowledge_gap_signals_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: knowledge_gaps knowledge_gaps_assignee_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.knowledge_gaps
    ADD CONSTRAINT knowledge_gaps_assignee_uuid_fkey FOREIGN KEY (assignee_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: knowledge_gaps knowledge_gaps_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.knowledge_gaps
    ADD CONSTRAINT knowledge_gaps_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: knowledge_gaps knowledge_gaps_dismissed_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.knowledge_gaps
    ADD CONSTRAINT knowledge_gaps_dismissed_by_fkey FOREIGN KEY (dismissed_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: knowledge_gaps knowledge_gaps_resolved_page_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.knowledge_gaps
    ADD CONSTRAINT knowledge_gaps_resolved_page_id_fkey FOREIGN KEY (resolved_page_id) REFERENCES public.documentation_pages(id) ON DELETE SET NULL;


--
-- Name: knowledge_gaps knowledge_gaps_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.knowledge_gaps
    ADD CONSTRAINT knowledge_gaps_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: linked_tickets linked_tickets_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.linked_tickets
    ADD CONSTRAINT linked_tickets_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: linked_tickets linked_tickets_linked_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.linked_tickets
    ADD CONSTRAINT linked_tickets_linked_ticket_id_fkey FOREIGN KEY (linked_ticket_id) REFERENCES public.tickets(id) ON DELETE CASCADE;


--
-- Name: linked_tickets linked_tickets_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.linked_tickets
    ADD CONSTRAINT linked_tickets_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE CASCADE;


--
-- Name: linked_tickets linked_tickets_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.linked_tickets
    ADD CONSTRAINT linked_tickets_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: notification_preferences notification_preferences_notification_type_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.notification_preferences
    ADD CONSTRAINT notification_preferences_notification_type_id_fkey FOREIGN KEY (notification_type_id) REFERENCES public.notification_types(id) ON DELETE CASCADE;


--
-- Name: notification_preferences notification_preferences_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.notification_preferences
    ADD CONSTRAINT notification_preferences_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: notification_preferences notification_preferences_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.notification_preferences
    ADD CONSTRAINT notification_preferences_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: notification_rate_limits notification_rate_limits_notification_type_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.notification_rate_limits
    ADD CONSTRAINT notification_rate_limits_notification_type_id_fkey FOREIGN KEY (notification_type_id) REFERENCES public.notification_types(id) ON DELETE CASCADE;


--
-- Name: notification_rate_limits notification_rate_limits_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.notification_rate_limits
    ADD CONSTRAINT notification_rate_limits_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: notifications notifications_notification_type_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT notifications_notification_type_id_fkey FOREIGN KEY (notification_type_id) REFERENCES public.notification_types(id) ON DELETE CASCADE;


--
-- Name: notifications notifications_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT notifications_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: notifications notifications_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT notifications_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: outbound_emails outbound_emails_channel_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.outbound_emails
    ADD CONSTRAINT outbound_emails_channel_id_fkey FOREIGN KEY (channel_id) REFERENCES public.channels(id);


--
-- Name: outbound_emails outbound_emails_comment_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.outbound_emails
    ADD CONSTRAINT outbound_emails_comment_id_fkey FOREIGN KEY (comment_id) REFERENCES public.comments(id) ON DELETE SET NULL;


--
-- Name: outbound_emails outbound_emails_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.outbound_emails
    ADD CONSTRAINT outbound_emails_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE SET NULL;


--
-- Name: outbound_emails outbound_emails_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.outbound_emails
    ADD CONSTRAINT outbound_emails_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: passkey_credentials passkey_credentials_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.passkey_credentials
    ADD CONSTRAINT passkey_credentials_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: plugin_activity plugin_activity_plugin_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_activity
    ADD CONSTRAINT plugin_activity_plugin_id_fkey FOREIGN KEY (plugin_id) REFERENCES public.plugins(id) ON DELETE CASCADE;


--
-- Name: plugin_activity plugin_activity_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_activity
    ADD CONSTRAINT plugin_activity_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: plugin_activity plugin_activity_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_activity
    ADD CONSTRAINT plugin_activity_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: plugin_collection_rows plugin_collection_rows_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_collection_rows
    ADD CONSTRAINT plugin_collection_rows_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: plugin_collection_rows plugin_collection_rows_plugin_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_collection_rows
    ADD CONSTRAINT plugin_collection_rows_plugin_id_fkey FOREIGN KEY (plugin_id) REFERENCES public.plugins(id) ON DELETE CASCADE;


--
-- Name: plugin_collection_rows plugin_collection_rows_schema_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_collection_rows
    ADD CONSTRAINT plugin_collection_rows_schema_id_fkey FOREIGN KEY (schema_id) REFERENCES public.plugin_collection_schemas(id) ON DELETE CASCADE;


--
-- Name: plugin_collection_rows plugin_collection_rows_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_collection_rows
    ADD CONSTRAINT plugin_collection_rows_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: plugin_collection_schemas plugin_collection_schemas_plugin_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_collection_schemas
    ADD CONSTRAINT plugin_collection_schemas_plugin_id_fkey FOREIGN KEY (plugin_id) REFERENCES public.plugins(id) ON DELETE CASCADE;


--
-- Name: plugin_collection_schemas plugin_collection_schemas_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_collection_schemas
    ADD CONSTRAINT plugin_collection_schemas_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: plugin_data plugin_data_plugin_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_data
    ADD CONSTRAINT plugin_data_plugin_id_fkey FOREIGN KEY (plugin_id) REFERENCES public.plugins(id) ON DELETE CASCADE;


--
-- Name: plugin_data plugin_data_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugin_data
    ADD CONSTRAINT plugin_data_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: plugins plugins_installed_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugins
    ADD CONSTRAINT plugins_installed_by_fkey FOREIGN KEY (installed_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: plugins plugins_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.plugins
    ADD CONSTRAINT plugins_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: project_tickets project_tickets_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.project_tickets
    ADD CONSTRAINT project_tickets_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: project_tickets project_tickets_project_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.project_tickets
    ADD CONSTRAINT project_tickets_project_id_fkey FOREIGN KEY (project_id) REFERENCES public.projects(id) ON DELETE CASCADE;


--
-- Name: project_tickets project_tickets_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.project_tickets
    ADD CONSTRAINT project_tickets_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE CASCADE;


--
-- Name: project_tickets project_tickets_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.project_tickets
    ADD CONSTRAINT project_tickets_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: projects projects_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.projects
    ADD CONSTRAINT projects_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: projects projects_owner_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.projects
    ADD CONSTRAINT projects_owner_uuid_fkey FOREIGN KEY (owner_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: projects projects_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.projects
    ADD CONSTRAINT projects_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: refresh_tokens refresh_tokens_session_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.refresh_tokens
    ADD CONSTRAINT refresh_tokens_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.active_sessions(session_id) ON DELETE CASCADE;


--
-- Name: refresh_tokens refresh_tokens_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.refresh_tokens
    ADD CONSTRAINT refresh_tokens_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: reset_tokens reset_tokens_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.reset_tokens
    ADD CONSTRAINT reset_tokens_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: rule_applications rule_applications_actor_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.rule_applications
    ADD CONSTRAINT rule_applications_actor_uuid_fkey FOREIGN KEY (actor_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: rule_applications rule_applications_rule_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.rule_applications
    ADD CONSTRAINT rule_applications_rule_id_fkey FOREIGN KEY (rule_id) REFERENCES public.rules(id) ON DELETE CASCADE;


--
-- Name: rule_applications rule_applications_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.rule_applications
    ADD CONSTRAINT rule_applications_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE CASCADE;


--
-- Name: rule_applications rule_applications_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.rule_applications
    ADD CONSTRAINT rule_applications_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: rule_versions rule_versions_rule_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.rule_versions
    ADD CONSTRAINT rule_versions_rule_id_fkey FOREIGN KEY (rule_id) REFERENCES public.rules(id) ON DELETE CASCADE;


--
-- Name: rule_versions rule_versions_saved_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.rule_versions
    ADD CONSTRAINT rule_versions_saved_by_fkey FOREIGN KEY (saved_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: rule_versions rule_versions_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.rule_versions
    ADD CONSTRAINT rule_versions_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: rules rules_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.rules
    ADD CONSTRAINT rules_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: rules rules_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.rules
    ADD CONSTRAINT rules_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: saved_views saved_views_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.saved_views
    ADD CONSTRAINT saved_views_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: saved_views saved_views_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.saved_views
    ADD CONSTRAINT saved_views_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: search_query_log search_query_log_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.search_query_log
    ADD CONSTRAINT search_query_log_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: security_events security_events_session_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.security_events
    ADD CONSTRAINT security_events_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.active_sessions(id) ON DELETE SET NULL;


--
-- Name: security_events security_events_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.security_events
    ADD CONSTRAINT security_events_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: site_settings site_settings_updated_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.site_settings
    ADD CONSTRAINT site_settings_updated_by_fkey FOREIGN KEY (updated_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: site_settings site_settings_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.site_settings
    ADD CONSTRAINT site_settings_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: sla_policies sla_policies_assignee_group_id_filter_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sla_policies
    ADD CONSTRAINT sla_policies_assignee_group_id_filter_fkey FOREIGN KEY (assignee_group_id_filter) REFERENCES public.groups(id) ON DELETE SET NULL;


--
-- Name: sla_policies sla_policies_category_id_filter_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sla_policies
    ADD CONSTRAINT sla_policies_category_id_filter_fkey FOREIGN KEY (category_id_filter) REFERENCES public.ticket_categories(id) ON DELETE SET NULL;


--
-- Name: sla_policies sla_policies_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sla_policies
    ADD CONSTRAINT sla_policies_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: sla_policies sla_policies_working_calendar_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sla_policies
    ADD CONSTRAINT sla_policies_working_calendar_id_fkey FOREIGN KEY (working_calendar_id) REFERENCES public.working_calendars(id) ON DELETE SET NULL;


--
-- Name: sla_policies sla_policies_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sla_policies
    ADD CONSTRAINT sla_policies_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: sync_actions sync_actions_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.sync_actions
    ADD CONSTRAINT sync_actions_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: sync_delta_tokens sync_delta_tokens_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_delta_tokens
    ADD CONSTRAINT sync_delta_tokens_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: sync_history sync_history_initiated_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_history
    ADD CONSTRAINT sync_history_initiated_by_fkey FOREIGN KEY (initiated_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: sync_history sync_history_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.sync_history
    ADD CONSTRAINT sync_history_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: tags tags_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.tags
    ADD CONSTRAINT tags_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: ticket_assets ticket_assets_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_assets
    ADD CONSTRAINT ticket_assets_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: ticket_categories ticket_categories_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_categories
    ADD CONSTRAINT ticket_categories_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: ticket_categories ticket_categories_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_categories
    ADD CONSTRAINT ticket_categories_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: ticket_assets ticket_devices_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_assets
    ADD CONSTRAINT ticket_devices_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: ticket_assets ticket_devices_device_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_assets
    ADD CONSTRAINT ticket_devices_device_id_fkey FOREIGN KEY (asset_id) REFERENCES public.assets(id) ON DELETE CASCADE;


--
-- Name: ticket_assets ticket_devices_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_assets
    ADD CONSTRAINT ticket_devices_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE CASCADE;


--
-- Name: ticket_tags ticket_tags_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_tags
    ADD CONSTRAINT ticket_tags_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: ticket_tags ticket_tags_tag_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_tags
    ADD CONSTRAINT ticket_tags_tag_id_fkey FOREIGN KEY (tag_id) REFERENCES public.tags(id) ON DELETE CASCADE;


--
-- Name: ticket_tags ticket_tags_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_tags
    ADD CONSTRAINT ticket_tags_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE CASCADE;


--
-- Name: ticket_tags ticket_tags_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_tags
    ADD CONSTRAINT ticket_tags_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: ticket_watchers ticket_watchers_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_watchers
    ADD CONSTRAINT ticket_watchers_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE CASCADE;


--
-- Name: ticket_watchers ticket_watchers_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_watchers
    ADD CONSTRAINT ticket_watchers_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: ticket_watchers ticket_watchers_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.ticket_watchers
    ADD CONSTRAINT ticket_watchers_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: tickets tickets_assignee_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.tickets
    ADD CONSTRAINT tickets_assignee_uuid_fkey FOREIGN KEY (assignee_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: tickets tickets_category_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.tickets
    ADD CONSTRAINT tickets_category_id_fkey FOREIGN KEY (category_id) REFERENCES public.ticket_categories(id) ON DELETE SET NULL;


--
-- Name: tickets tickets_closed_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.tickets
    ADD CONSTRAINT tickets_closed_by_fkey FOREIGN KEY (closed_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: tickets tickets_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.tickets
    ADD CONSTRAINT tickets_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: tickets tickets_merged_by_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.tickets
    ADD CONSTRAINT tickets_merged_by_user_uuid_fkey FOREIGN KEY (merged_by_user_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: tickets tickets_merged_into_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.tickets
    ADD CONSTRAINT tickets_merged_into_ticket_id_fkey FOREIGN KEY (merged_into_ticket_id) REFERENCES public.tickets(id) ON DELETE SET NULL;


--
-- Name: tickets tickets_origin_channel_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.tickets
    ADD CONSTRAINT tickets_origin_channel_id_fkey FOREIGN KEY (origin_channel_id) REFERENCES public.channels(id) ON DELETE SET NULL;


--
-- Name: tickets tickets_recurrence_template_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.tickets
    ADD CONSTRAINT tickets_recurrence_template_id_fkey FOREIGN KEY (recurrence_template_id) REFERENCES public.tickets(id) ON DELETE SET NULL;


--
-- Name: tickets tickets_requester_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.tickets
    ADD CONSTRAINT tickets_requester_uuid_fkey FOREIGN KEY (requester_uuid) REFERENCES public.users(uuid) ON DELETE RESTRICT;


--
-- Name: tickets tickets_workflow_state_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.tickets
    ADD CONSTRAINT tickets_workflow_state_id_fkey FOREIGN KEY (workflow_state_id) REFERENCES public.workflow_states(id);


--
-- Name: tickets tickets_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.tickets
    ADD CONSTRAINT tickets_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: user_auth_identities user_auth_identities_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.user_auth_identities
    ADD CONSTRAINT user_auth_identities_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: user_auth_identities user_auth_identities_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.user_auth_identities
    ADD CONSTRAINT user_auth_identities_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: user_emails user_emails_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.user_emails
    ADD CONSTRAINT user_emails_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: user_emails user_emails_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.user_emails
    ADD CONSTRAINT user_emails_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: user_groups user_groups_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.user_groups
    ADD CONSTRAINT user_groups_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: user_groups user_groups_group_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.user_groups
    ADD CONSTRAINT user_groups_group_id_fkey FOREIGN KEY (group_id) REFERENCES public.groups(id) ON DELETE CASCADE;


--
-- Name: user_groups user_groups_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.user_groups
    ADD CONSTRAINT user_groups_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: user_groups user_groups_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.user_groups
    ADD CONSTRAINT user_groups_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: user_preferences user_preferences_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.user_preferences
    ADD CONSTRAINT user_preferences_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: user_recovery_codes user_recovery_codes_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk
--

ALTER TABLE ONLY public.user_recovery_codes
    ADD CONSTRAINT user_recovery_codes_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: user_ticket_views user_ticket_views_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.user_ticket_views
    ADD CONSTRAINT user_ticket_views_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE CASCADE;


--
-- Name: user_ticket_views user_ticket_views_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.user_ticket_views
    ADD CONSTRAINT user_ticket_views_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: user_ticket_views user_ticket_views_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.user_ticket_views
    ADD CONSTRAINT user_ticket_views_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: webhook_deliveries webhook_deliveries_webhook_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.webhook_deliveries
    ADD CONSTRAINT webhook_deliveries_webhook_id_fkey FOREIGN KEY (webhook_id) REFERENCES public.webhooks(id) ON DELETE CASCADE;


--
-- Name: webhook_deliveries webhook_deliveries_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.webhook_deliveries
    ADD CONSTRAINT webhook_deliveries_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: webhooks webhooks_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.webhooks
    ADD CONSTRAINT webhooks_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: webhooks webhooks_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.webhooks
    ADD CONSTRAINT webhooks_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: workflow_states workflow_states_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.workflow_states
    ADD CONSTRAINT workflow_states_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: workflow_states workflow_states_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.workflow_states
    ADD CONSTRAINT workflow_states_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: working_calendar_holidays working_calendar_holidays_calendar_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.working_calendar_holidays
    ADD CONSTRAINT working_calendar_holidays_calendar_id_fkey FOREIGN KEY (calendar_id) REFERENCES public.working_calendars(id) ON DELETE CASCADE;


--
-- Name: working_calendar_holidays working_calendar_holidays_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.working_calendar_holidays
    ADD CONSTRAINT working_calendar_holidays_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: working_calendars working_calendars_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.working_calendars
    ADD CONSTRAINT working_calendars_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;


--
-- Name: working_calendars working_calendars_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.working_calendars
    ADD CONSTRAINT working_calendars_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: workspace_members workspace_members_user_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.workspace_members
    ADD CONSTRAINT workspace_members_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;


--
-- Name: workspace_members workspace_members_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.workspace_members
    ADD CONSTRAINT workspace_members_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);


--
-- Name: yjs_snapshots yjs_snapshots_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE ONLY public.yjs_snapshots
    ADD CONSTRAINT yjs_snapshots_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: api_tokens; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.api_tokens ENABLE ROW LEVEL SECURITY;

--
-- Name: api_tokens api_tokens_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY api_tokens_workspace_isolation ON public.api_tokens USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: article_content_revisions; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.article_content_revisions ENABLE ROW LEVEL SECURITY;

--
-- Name: article_content_revisions article_content_revisions_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY article_content_revisions_workspace_isolation ON public.article_content_revisions USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: article_contents; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.article_contents ENABLE ROW LEVEL SECURITY;

--
-- Name: article_contents article_contents_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY article_contents_workspace_isolation ON public.article_contents USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: asset_audits; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.asset_audits ENABLE ROW LEVEL SECURITY;

--
-- Name: asset_audits asset_audits_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY asset_audits_workspace_isolation ON public.asset_audits USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: asset_groups; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.asset_groups ENABLE ROW LEVEL SECURITY;

--
-- Name: asset_groups asset_groups_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY asset_groups_workspace_isolation ON public.asset_groups USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: asset_kinds; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.asset_kinds ENABLE ROW LEVEL SECURITY;

--
-- Name: asset_kinds asset_kinds_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY asset_kinds_workspace_isolation ON public.asset_kinds USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: asset_usage_log; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.asset_usage_log ENABLE ROW LEVEL SECURITY;

--
-- Name: asset_usage_log asset_usage_log_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY asset_usage_log_workspace_isolation ON public.asset_usage_log USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: assets; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.assets ENABLE ROW LEVEL SECURITY;

--
-- Name: assets assets_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY assets_workspace_isolation ON public.assets USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: assignment_log; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.assignment_log ENABLE ROW LEVEL SECURITY;

--
-- Name: assignment_log assignment_log_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY assignment_log_workspace_isolation ON public.assignment_log USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: assignment_rule_state; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.assignment_rule_state ENABLE ROW LEVEL SECURITY;

--
-- Name: assignment_rule_state assignment_rule_state_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY assignment_rule_state_workspace_isolation ON public.assignment_rule_state USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: assignment_rules; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.assignment_rules ENABLE ROW LEVEL SECURITY;

--
-- Name: assignment_rules assignment_rules_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY assignment_rules_workspace_isolation ON public.assignment_rules USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: attachments; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.attachments ENABLE ROW LEVEL SECURITY;

--
-- Name: attachments attachments_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY attachments_workspace_isolation ON public.attachments USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: audit_log; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.audit_log ENABLE ROW LEVEL SECURITY;

--
-- Name: audit_log_2026_05; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.audit_log_2026_05 ENABLE ROW LEVEL SECURITY;

--
-- Name: audit_log_2026_05 audit_log_2026_05_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY audit_log_2026_05_workspace_isolation ON public.audit_log_2026_05 USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: audit_log_2026_06; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.audit_log_2026_06 ENABLE ROW LEVEL SECURITY;

--
-- Name: audit_log_2026_06 audit_log_2026_06_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY audit_log_2026_06_workspace_isolation ON public.audit_log_2026_06 USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: audit_log_2026_07; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.audit_log_2026_07 ENABLE ROW LEVEL SECURITY;

--
-- Name: audit_log_2026_07 audit_log_2026_07_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY audit_log_2026_07_workspace_isolation ON public.audit_log_2026_07 USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: audit_log_2026_08; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.audit_log_2026_08 ENABLE ROW LEVEL SECURITY;

--
-- Name: audit_log_2026_08 audit_log_2026_08_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY audit_log_2026_08_workspace_isolation ON public.audit_log_2026_08 USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: audit_log_default; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.audit_log_default ENABLE ROW LEVEL SECURITY;

--
-- Name: audit_log_default audit_log_default_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY audit_log_default_workspace_isolation ON public.audit_log_default USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: audit_log audit_log_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY audit_log_workspace_isolation ON public.audit_log USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: backup_jobs; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.backup_jobs ENABLE ROW LEVEL SECURITY;

--
-- Name: backup_jobs backup_jobs_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY backup_jobs_workspace_isolation ON public.backup_jobs USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: bug_reports; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.bug_reports ENABLE ROW LEVEL SECURITY;

--
-- Name: bug_reports bug_reports_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY bug_reports_workspace_isolation ON public.bug_reports USING (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text))) WITH CHECK (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text)));


--
-- Name: canned_response_insertions; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.canned_response_insertions ENABLE ROW LEVEL SECURITY;

--
-- Name: canned_response_insertions canned_response_insertions_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY canned_response_insertions_workspace_isolation ON public.canned_response_insertions USING (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text))) WITH CHECK (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text)));


--
-- Name: canned_responses; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.canned_responses ENABLE ROW LEVEL SECURITY;

--
-- Name: canned_responses canned_responses_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY canned_responses_workspace_isolation ON public.canned_responses USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: category_group_visibility; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.category_group_visibility ENABLE ROW LEVEL SECURITY;

--
-- Name: category_group_visibility category_group_visibility_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY category_group_visibility_workspace_isolation ON public.category_group_visibility USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: channel_credentials; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.channel_credentials ENABLE ROW LEVEL SECURITY;

--
-- Name: channel_credentials channel_credentials_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY channel_credentials_workspace_isolation ON public.channel_credentials USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: channel_messages; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.channel_messages ENABLE ROW LEVEL SECURITY;

--
-- Name: channel_messages channel_messages_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY channel_messages_workspace_isolation ON public.channel_messages USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: channels; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.channels ENABLE ROW LEVEL SECURITY;

--
-- Name: channels channels_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY channels_workspace_isolation ON public.channels USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: comments; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.comments ENABLE ROW LEVEL SECURITY;

--
-- Name: comments comments_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY comments_workspace_isolation ON public.comments USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: csp_reports; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.csp_reports ENABLE ROW LEVEL SECURITY;

--
-- Name: csp_reports csp_reports_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY csp_reports_workspace_isolation ON public.csp_reports USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: cycle_tickets; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.cycle_tickets ENABLE ROW LEVEL SECURITY;

--
-- Name: cycle_tickets cycle_tickets_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY cycle_tickets_workspace_isolation ON public.cycle_tickets USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: cycles; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.cycles ENABLE ROW LEVEL SECURITY;

--
-- Name: cycles cycles_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY cycles_workspace_isolation ON public.cycles USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: documentation_collection_pages; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_collection_pages ENABLE ROW LEVEL SECURITY;

--
-- Name: documentation_collection_pages documentation_collection_pages_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY documentation_collection_pages_workspace_isolation ON public.documentation_collection_pages USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: documentation_collection_visibility; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_collection_visibility ENABLE ROW LEVEL SECURITY;

--
-- Name: documentation_collection_visibility documentation_collection_visibility_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY documentation_collection_visibility_workspace_isolation ON public.documentation_collection_visibility USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: documentation_collections; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_collections ENABLE ROW LEVEL SECURITY;

--
-- Name: documentation_collections documentation_collections_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY documentation_collections_workspace_isolation ON public.documentation_collections USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: documentation_page_embeddings; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_page_embeddings ENABLE ROW LEVEL SECURITY;

--
-- Name: documentation_page_embeddings documentation_page_embeddings_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY documentation_page_embeddings_workspace_isolation ON public.documentation_page_embeddings USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: documentation_page_tickets; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_page_tickets ENABLE ROW LEVEL SECURITY;

--
-- Name: documentation_page_tickets documentation_page_tickets_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY documentation_page_tickets_workspace_isolation ON public.documentation_page_tickets USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: documentation_page_visibility; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_page_visibility ENABLE ROW LEVEL SECURITY;

--
-- Name: documentation_page_visibility documentation_page_visibility_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY documentation_page_visibility_workspace_isolation ON public.documentation_page_visibility USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: documentation_pages; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_pages ENABLE ROW LEVEL SECURITY;

--
-- Name: documentation_pages documentation_pages_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY documentation_pages_workspace_isolation ON public.documentation_pages USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: documentation_revisions; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_revisions ENABLE ROW LEVEL SECURITY;

--
-- Name: documentation_revisions documentation_revisions_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY documentation_revisions_workspace_isolation ON public.documentation_revisions USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: documentation_starred_pages; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_starred_pages ENABLE ROW LEVEL SECURITY;

--
-- Name: documentation_starred_pages documentation_starred_pages_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY documentation_starred_pages_workspace_isolation ON public.documentation_starred_pages USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: documentation_subscriptions; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_subscriptions ENABLE ROW LEVEL SECURITY;

--
-- Name: documentation_subscriptions documentation_subscriptions_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY documentation_subscriptions_workspace_isolation ON public.documentation_subscriptions USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: group_includes; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.group_includes ENABLE ROW LEVEL SECURITY;

--
-- Name: group_includes group_includes_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY group_includes_workspace_isolation ON public.group_includes USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: groups; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.groups ENABLE ROW LEVEL SECURITY;

--
-- Name: groups groups_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY groups_workspace_isolation ON public.groups USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: import_jobs; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.import_jobs ENABLE ROW LEVEL SECURITY;

--
-- Name: import_jobs import_jobs_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY import_jobs_workspace_isolation ON public.import_jobs USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: knowledge_gap_signals; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.knowledge_gap_signals ENABLE ROW LEVEL SECURITY;

--
-- Name: knowledge_gap_signals knowledge_gap_signals_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY knowledge_gap_signals_workspace_isolation ON public.knowledge_gap_signals USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: knowledge_gaps; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.knowledge_gaps ENABLE ROW LEVEL SECURITY;

--
-- Name: knowledge_gaps knowledge_gaps_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY knowledge_gaps_workspace_isolation ON public.knowledge_gaps USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: linked_tickets; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.linked_tickets ENABLE ROW LEVEL SECURITY;

--
-- Name: linked_tickets linked_tickets_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY linked_tickets_workspace_isolation ON public.linked_tickets USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: notification_preferences; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.notification_preferences ENABLE ROW LEVEL SECURITY;

--
-- Name: notification_preferences notification_preferences_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY notification_preferences_workspace_isolation ON public.notification_preferences USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: notifications; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.notifications ENABLE ROW LEVEL SECURITY;

--
-- Name: notifications notifications_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY notifications_workspace_isolation ON public.notifications USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: outbound_emails; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.outbound_emails ENABLE ROW LEVEL SECURITY;

--
-- Name: outbound_emails outbound_emails_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY outbound_emails_workspace_isolation ON public.outbound_emails USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: plugin_activity; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.plugin_activity ENABLE ROW LEVEL SECURITY;

--
-- Name: plugin_activity plugin_activity_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY plugin_activity_workspace_isolation ON public.plugin_activity USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: plugin_collection_rows; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.plugin_collection_rows ENABLE ROW LEVEL SECURITY;

--
-- Name: plugin_collection_rows plugin_collection_rows_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY plugin_collection_rows_workspace_isolation ON public.plugin_collection_rows USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: plugin_collection_schemas; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.plugin_collection_schemas ENABLE ROW LEVEL SECURITY;

--
-- Name: plugin_collection_schemas plugin_collection_schemas_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY plugin_collection_schemas_workspace_isolation ON public.plugin_collection_schemas USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: plugin_data; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.plugin_data ENABLE ROW LEVEL SECURITY;

--
-- Name: plugin_data plugin_data_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY plugin_data_workspace_isolation ON public.plugin_data USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: plugins; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.plugins ENABLE ROW LEVEL SECURITY;

--
-- Name: plugins plugins_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY plugins_workspace_isolation ON public.plugins USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: project_tickets; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.project_tickets ENABLE ROW LEVEL SECURITY;

--
-- Name: project_tickets project_tickets_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY project_tickets_workspace_isolation ON public.project_tickets USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: projects; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.projects ENABLE ROW LEVEL SECURITY;

--
-- Name: projects projects_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY projects_workspace_isolation ON public.projects USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: rule_applications; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.rule_applications ENABLE ROW LEVEL SECURITY;

--
-- Name: rule_applications rule_applications_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY rule_applications_workspace_isolation ON public.rule_applications USING (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text))) WITH CHECK (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text)));


--
-- Name: rule_versions; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.rule_versions ENABLE ROW LEVEL SECURITY;

--
-- Name: rule_versions rule_versions_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY rule_versions_workspace_isolation ON public.rule_versions USING (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text))) WITH CHECK (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text)));


--
-- Name: rules; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.rules ENABLE ROW LEVEL SECURITY;

--
-- Name: rules rules_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY rules_workspace_isolation ON public.rules USING (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text))) WITH CHECK (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text)));


--
-- Name: saved_views; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.saved_views ENABLE ROW LEVEL SECURITY;

--
-- Name: saved_views saved_views_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY saved_views_workspace_isolation ON public.saved_views USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: search_query_log; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.search_query_log ENABLE ROW LEVEL SECURITY;

--
-- Name: search_query_log search_query_log_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY search_query_log_workspace_isolation ON public.search_query_log USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: site_settings; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.site_settings ENABLE ROW LEVEL SECURITY;

--
-- Name: site_settings site_settings_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY site_settings_workspace_isolation ON public.site_settings USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: sla_policies; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.sla_policies ENABLE ROW LEVEL SECURITY;

--
-- Name: sla_policies sla_policies_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY sla_policies_workspace_isolation ON public.sla_policies USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: sync_actions; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.sync_actions ENABLE ROW LEVEL SECURITY;

--
-- Name: sync_actions_2026_05; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.sync_actions_2026_05 ENABLE ROW LEVEL SECURITY;

--
-- Name: sync_actions_2026_05 sync_actions_2026_05_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY sync_actions_2026_05_workspace_isolation ON public.sync_actions_2026_05 USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: sync_actions_2026_06; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.sync_actions_2026_06 ENABLE ROW LEVEL SECURITY;

--
-- Name: sync_actions_2026_06 sync_actions_2026_06_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY sync_actions_2026_06_workspace_isolation ON public.sync_actions_2026_06 USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: sync_actions_2026_07; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.sync_actions_2026_07 ENABLE ROW LEVEL SECURITY;

--
-- Name: sync_actions_2026_07 sync_actions_2026_07_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY sync_actions_2026_07_workspace_isolation ON public.sync_actions_2026_07 USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: sync_actions_2026_08; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.sync_actions_2026_08 ENABLE ROW LEVEL SECURITY;

--
-- Name: sync_actions_2026_08 sync_actions_2026_08_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY sync_actions_2026_08_workspace_isolation ON public.sync_actions_2026_08 USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: sync_actions_default; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.sync_actions_default ENABLE ROW LEVEL SECURITY;

--
-- Name: sync_actions_default sync_actions_default_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY sync_actions_default_workspace_isolation ON public.sync_actions_default USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: sync_actions sync_actions_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY sync_actions_workspace_isolation ON public.sync_actions USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: sync_delta_tokens; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.sync_delta_tokens ENABLE ROW LEVEL SECURITY;

--
-- Name: sync_delta_tokens sync_delta_tokens_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY sync_delta_tokens_workspace_isolation ON public.sync_delta_tokens USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: sync_history; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.sync_history ENABLE ROW LEVEL SECURITY;

--
-- Name: sync_history sync_history_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY sync_history_workspace_isolation ON public.sync_history USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: tags; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.tags ENABLE ROW LEVEL SECURITY;

--
-- Name: tags tags_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY tags_workspace_isolation ON public.tags USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: ticket_assets; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.ticket_assets ENABLE ROW LEVEL SECURITY;

--
-- Name: ticket_assets ticket_assets_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY ticket_assets_workspace_isolation ON public.ticket_assets USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: ticket_categories; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.ticket_categories ENABLE ROW LEVEL SECURITY;

--
-- Name: ticket_categories ticket_categories_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY ticket_categories_workspace_isolation ON public.ticket_categories USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: ticket_tags; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.ticket_tags ENABLE ROW LEVEL SECURITY;

--
-- Name: ticket_tags ticket_tags_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY ticket_tags_workspace_isolation ON public.ticket_tags USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: ticket_watchers; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.ticket_watchers ENABLE ROW LEVEL SECURITY;

--
-- Name: ticket_watchers ticket_watchers_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY ticket_watchers_workspace_isolation ON public.ticket_watchers USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: tickets; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.tickets ENABLE ROW LEVEL SECURITY;

--
-- Name: tickets tickets_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY tickets_workspace_isolation ON public.tickets USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: user_groups; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.user_groups ENABLE ROW LEVEL SECURITY;

--
-- Name: user_groups user_groups_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY user_groups_workspace_isolation ON public.user_groups USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: user_ticket_views; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.user_ticket_views ENABLE ROW LEVEL SECURITY;

--
-- Name: user_ticket_views user_ticket_views_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY user_ticket_views_workspace_isolation ON public.user_ticket_views USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: webhook_deliveries; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.webhook_deliveries ENABLE ROW LEVEL SECURITY;

--
-- Name: webhook_deliveries webhook_deliveries_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY webhook_deliveries_workspace_isolation ON public.webhook_deliveries USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: webhooks; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.webhooks ENABLE ROW LEVEL SECURITY;

--
-- Name: webhooks webhooks_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY webhooks_workspace_isolation ON public.webhooks USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: workflow_states; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.workflow_states ENABLE ROW LEVEL SECURITY;

--
-- Name: workflow_states workflow_states_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY workflow_states_workspace_isolation ON public.workflow_states USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: working_calendar_holidays; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.working_calendar_holidays ENABLE ROW LEVEL SECURITY;

--
-- Name: working_calendar_holidays working_calendar_holidays_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY working_calendar_holidays_workspace_isolation ON public.working_calendar_holidays USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: working_calendars; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.working_calendars ENABLE ROW LEVEL SECURITY;

--
-- Name: working_calendars working_calendars_workspace_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY working_calendars_workspace_isolation ON public.working_calendars USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: yjs_snapshots; Type: ROW SECURITY; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.yjs_snapshots ENABLE ROW LEVEL SECURITY;

--
-- Name: yjs_snapshots yjs_snapshots_tenant_isolation; Type: POLICY; Schema: public; Owner: nosdesk_admin
--

CREATE POLICY yjs_snapshots_tenant_isolation ON public.yjs_snapshots USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));


--
-- Name: SCHEMA public; Type: ACL; Schema: -; Owner: pg_database_owner
--

GRANT USAGE ON SCHEMA public TO nosdesk_app;
GRANT USAGE ON SCHEMA public TO nosdesk_admin;


--
-- Name: TABLE active_sessions; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.active_sessions TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.active_sessions TO nosdesk_admin;


--
-- Name: SEQUENCE active_sessions_id_seq; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT ALL ON SEQUENCE public.active_sessions_id_seq TO nosdesk_app;
GRANT ALL ON SEQUENCE public.active_sessions_id_seq TO nosdesk_admin;


--
-- Name: TABLE api_tokens; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.api_tokens TO nosdesk_app;


--
-- Name: SEQUENCE api_tokens_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.api_tokens_id_seq TO nosdesk_app;


--
-- Name: TABLE article_content_revisions; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.article_content_revisions TO nosdesk_app;


--
-- Name: SEQUENCE article_content_revisions_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.article_content_revisions_id_seq TO nosdesk_app;


--
-- Name: TABLE article_contents; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.article_contents TO nosdesk_app;


--
-- Name: SEQUENCE article_contents_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.article_contents_id_seq TO nosdesk_app;


--
-- Name: TABLE asset_audits; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.asset_audits TO nosdesk_app;


--
-- Name: SEQUENCE asset_audits_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.asset_audits_id_seq TO nosdesk_app;


--
-- Name: TABLE asset_groups; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.asset_groups TO nosdesk_app;


--
-- Name: TABLE asset_kinds; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.asset_kinds TO nosdesk_app;


--
-- Name: SEQUENCE asset_kinds_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.asset_kinds_id_seq TO nosdesk_app;


--
-- Name: TABLE asset_usage_log; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.asset_usage_log TO nosdesk_app;


--
-- Name: SEQUENCE asset_usage_log_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.asset_usage_log_id_seq TO nosdesk_app;


--
-- Name: TABLE assets; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.assets TO nosdesk_app;


--
-- Name: TABLE assignment_log; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.assignment_log TO nosdesk_app;


--
-- Name: SEQUENCE assignment_log_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.assignment_log_id_seq TO nosdesk_app;


--
-- Name: TABLE assignment_rule_state; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.assignment_rule_state TO nosdesk_app;


--
-- Name: TABLE assignment_rules; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.assignment_rules TO nosdesk_app;


--
-- Name: SEQUENCE assignment_rules_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.assignment_rules_id_seq TO nosdesk_app;


--
-- Name: TABLE attachments; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.attachments TO nosdesk_app;


--
-- Name: SEQUENCE attachments_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.attachments_id_seq TO nosdesk_app;


--
-- Name: TABLE audit_log; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.audit_log TO nosdesk_app;


--
-- Name: SEQUENCE audit_log_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.audit_log_id_seq TO nosdesk_app;


--
-- Name: TABLE audit_log_default; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.audit_log_default TO nosdesk_app;


--
-- Name: TABLE backup_jobs; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.backup_jobs TO nosdesk_app;


--
-- Name: TABLE bug_reports; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.bug_reports TO nosdesk_app;


--
-- Name: SEQUENCE bug_reports_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.bug_reports_id_seq TO nosdesk_app;


--
-- Name: TABLE canned_response_insertions; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.canned_response_insertions TO nosdesk_app;


--
-- Name: SEQUENCE canned_response_insertions_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.canned_response_insertions_id_seq TO nosdesk_app;


--
-- Name: TABLE canned_responses; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.canned_responses TO nosdesk_app;


--
-- Name: SEQUENCE canned_responses_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.canned_responses_id_seq TO nosdesk_app;


--
-- Name: TABLE category_group_visibility; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.category_group_visibility TO nosdesk_app;


--
-- Name: TABLE channel_credentials; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.channel_credentials TO nosdesk_app;


--
-- Name: SEQUENCE channel_credentials_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.channel_credentials_id_seq TO nosdesk_app;


--
-- Name: TABLE channel_messages; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.channel_messages TO nosdesk_app;


--
-- Name: SEQUENCE channel_messages_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.channel_messages_id_seq TO nosdesk_app;


--
-- Name: TABLE channels; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.channels TO nosdesk_app;


--
-- Name: SEQUENCE channels_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.channels_id_seq TO nosdesk_app;


--
-- Name: TABLE comments; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.comments TO nosdesk_app;


--
-- Name: SEQUENCE comments_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.comments_id_seq TO nosdesk_app;


--
-- Name: TABLE csp_reports; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.csp_reports TO nosdesk_app;


--
-- Name: SEQUENCE csp_reports_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.csp_reports_id_seq TO nosdesk_app;


--
-- Name: TABLE cycle_tickets; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.cycle_tickets TO nosdesk_app;


--
-- Name: TABLE cycles; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.cycles TO nosdesk_app;


--
-- Name: SEQUENCE cycles_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.cycles_id_seq TO nosdesk_app;


--
-- Name: SEQUENCE devices_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.devices_id_seq TO nosdesk_app;


--
-- Name: TABLE documentation_collection_pages; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.documentation_collection_pages TO nosdesk_app;


--
-- Name: TABLE documentation_collection_visibility; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.documentation_collection_visibility TO nosdesk_app;


--
-- Name: SEQUENCE documentation_collection_visibility_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.documentation_collection_visibility_id_seq TO nosdesk_app;


--
-- Name: TABLE documentation_collections; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.documentation_collections TO nosdesk_app;


--
-- Name: SEQUENCE documentation_collections_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.documentation_collections_id_seq TO nosdesk_app;


--
-- Name: TABLE documentation_page_embeddings; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.documentation_page_embeddings TO nosdesk_app;


--
-- Name: TABLE documentation_page_tickets; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.documentation_page_tickets TO nosdesk_app;


--
-- Name: TABLE documentation_page_visibility; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.documentation_page_visibility TO nosdesk_app;


--
-- Name: SEQUENCE documentation_page_visibility_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.documentation_page_visibility_id_seq TO nosdesk_app;


--
-- Name: TABLE documentation_pages; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.documentation_pages TO nosdesk_app;


--
-- Name: SEQUENCE documentation_pages_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.documentation_pages_id_seq TO nosdesk_app;


--
-- Name: TABLE documentation_revisions; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.documentation_revisions TO nosdesk_app;


--
-- Name: SEQUENCE documentation_revisions_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.documentation_revisions_id_seq TO nosdesk_app;


--
-- Name: TABLE documentation_starred_pages; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.documentation_starred_pages TO nosdesk_app;


--
-- Name: SEQUENCE documentation_starred_pages_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.documentation_starred_pages_id_seq TO nosdesk_app;


--
-- Name: TABLE documentation_subscriptions; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.documentation_subscriptions TO nosdesk_app;


--
-- Name: SEQUENCE documentation_subscriptions_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.documentation_subscriptions_id_seq TO nosdesk_app;


--
-- Name: TABLE email_suppressions; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.email_suppressions TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.email_suppressions TO nosdesk_admin;


--
-- Name: TABLE group_includes; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.group_includes TO nosdesk_app;


--
-- Name: TABLE groups; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.groups TO nosdesk_app;


--
-- Name: SEQUENCE groups_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.groups_id_seq TO nosdesk_app;


--
-- Name: TABLE idempotency_keys; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.idempotency_keys TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.idempotency_keys TO nosdesk_admin;


--
-- Name: TABLE import_jobs; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.import_jobs TO nosdesk_app;


--
-- Name: TABLE knowledge_gap_signals; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.knowledge_gap_signals TO nosdesk_app;


--
-- Name: SEQUENCE knowledge_gap_signals_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.knowledge_gap_signals_id_seq TO nosdesk_app;


--
-- Name: TABLE knowledge_gaps; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.knowledge_gaps TO nosdesk_app;


--
-- Name: SEQUENCE knowledge_gaps_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.knowledge_gaps_id_seq TO nosdesk_app;


--
-- Name: TABLE linked_tickets; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.linked_tickets TO nosdesk_app;


--
-- Name: TABLE notification_preferences; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.notification_preferences TO nosdesk_app;


--
-- Name: SEQUENCE notification_preferences_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.notification_preferences_id_seq TO nosdesk_app;


--
-- Name: TABLE notification_rate_limits; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.notification_rate_limits TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.notification_rate_limits TO nosdesk_admin;


--
-- Name: SEQUENCE notification_rate_limits_id_seq; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT ALL ON SEQUENCE public.notification_rate_limits_id_seq TO nosdesk_app;
GRANT ALL ON SEQUENCE public.notification_rate_limits_id_seq TO nosdesk_admin;


--
-- Name: TABLE notification_types; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.notification_types TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.notification_types TO nosdesk_admin;


--
-- Name: SEQUENCE notification_types_id_seq; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT ALL ON SEQUENCE public.notification_types_id_seq TO nosdesk_app;
GRANT ALL ON SEQUENCE public.notification_types_id_seq TO nosdesk_admin;


--
-- Name: TABLE notifications; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.notifications TO nosdesk_app;


--
-- Name: SEQUENCE notifications_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.notifications_id_seq TO nosdesk_app;


--
-- Name: TABLE outbound_emails; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.outbound_emails TO nosdesk_app;


--
-- Name: SEQUENCE outbound_emails_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.outbound_emails_id_seq TO nosdesk_app;


--
-- Name: TABLE passkey_credentials; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.passkey_credentials TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.passkey_credentials TO nosdesk_admin;


--
-- Name: TABLE plugin_activity; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.plugin_activity TO nosdesk_app;


--
-- Name: SEQUENCE plugin_activity_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.plugin_activity_id_seq TO nosdesk_app;


--
-- Name: TABLE plugin_collection_rows; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.plugin_collection_rows TO nosdesk_app;


--
-- Name: SEQUENCE plugin_collection_rows_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.plugin_collection_rows_id_seq TO nosdesk_app;


--
-- Name: TABLE plugin_collection_schemas; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.plugin_collection_schemas TO nosdesk_app;


--
-- Name: SEQUENCE plugin_collection_schemas_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.plugin_collection_schemas_id_seq TO nosdesk_app;


--
-- Name: TABLE plugin_data; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.plugin_data TO nosdesk_app;


--
-- Name: SEQUENCE plugin_data_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.plugin_data_id_seq TO nosdesk_app;


--
-- Name: TABLE plugin_local_signing_key; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.plugin_local_signing_key TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.plugin_local_signing_key TO nosdesk_admin;


--
-- Name: TABLE plugin_registry_state; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.plugin_registry_state TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.plugin_registry_state TO nosdesk_admin;


--
-- Name: TABLE plugin_trusted_publishers; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.plugin_trusted_publishers TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.plugin_trusted_publishers TO nosdesk_admin;


--
-- Name: SEQUENCE plugin_trusted_publishers_id_seq; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT ALL ON SEQUENCE public.plugin_trusted_publishers_id_seq TO nosdesk_app;
GRANT ALL ON SEQUENCE public.plugin_trusted_publishers_id_seq TO nosdesk_admin;


--
-- Name: TABLE plugins; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.plugins TO nosdesk_app;


--
-- Name: SEQUENCE plugins_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.plugins_id_seq TO nosdesk_app;


--
-- Name: TABLE project_tickets; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.project_tickets TO nosdesk_app;


--
-- Name: TABLE projects; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.projects TO nosdesk_app;


--
-- Name: SEQUENCE projects_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.projects_id_seq TO nosdesk_app;


--
-- Name: TABLE refresh_tokens; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.refresh_tokens TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.refresh_tokens TO nosdesk_admin;


--
-- Name: SEQUENCE refresh_tokens_id_seq; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT ALL ON SEQUENCE public.refresh_tokens_id_seq TO nosdesk_app;
GRANT ALL ON SEQUENCE public.refresh_tokens_id_seq TO nosdesk_admin;


--
-- Name: TABLE reset_tokens; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.reset_tokens TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.reset_tokens TO nosdesk_admin;


--
-- Name: TABLE rule_applications; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.rule_applications TO nosdesk_app;


--
-- Name: SEQUENCE rule_applications_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.rule_applications_id_seq TO nosdesk_app;


--
-- Name: TABLE rule_versions; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.rule_versions TO nosdesk_app;


--
-- Name: SEQUENCE rule_versions_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.rule_versions_id_seq TO nosdesk_app;


--
-- Name: TABLE rules; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.rules TO nosdesk_app;


--
-- Name: SEQUENCE rules_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.rules_id_seq TO nosdesk_app;


--
-- Name: TABLE saved_views; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.saved_views TO nosdesk_app;


--
-- Name: SEQUENCE saved_views_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.saved_views_id_seq TO nosdesk_app;


--
-- Name: TABLE search_index_state; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.search_index_state TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.search_index_state TO nosdesk_admin;


--
-- Name: SEQUENCE search_index_state_id_seq; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT ALL ON SEQUENCE public.search_index_state_id_seq TO nosdesk_app;
GRANT ALL ON SEQUENCE public.search_index_state_id_seq TO nosdesk_admin;


--
-- Name: TABLE search_query_log; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.search_query_log TO nosdesk_app;


--
-- Name: SEQUENCE search_query_log_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.search_query_log_id_seq TO nosdesk_app;


--
-- Name: TABLE security_events; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.security_events TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.security_events TO nosdesk_admin;


--
-- Name: SEQUENCE security_events_id_seq; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT ALL ON SEQUENCE public.security_events_id_seq TO nosdesk_app;
GRANT ALL ON SEQUENCE public.security_events_id_seq TO nosdesk_admin;


--
-- Name: TABLE site_settings; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.site_settings TO nosdesk_app;


--
-- Name: TABLE sla_policies; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.sla_policies TO nosdesk_app;


--
-- Name: SEQUENCE sla_policies_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.sla_policies_id_seq TO nosdesk_app;


--
-- Name: TABLE sync_actions; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.sync_actions TO nosdesk_app;


--
-- Name: SEQUENCE sync_actions_sync_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.sync_actions_sync_id_seq TO nosdesk_app;


--
-- Name: TABLE sync_actions_default; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.sync_actions_default TO nosdesk_app;


--
-- Name: TABLE sync_delta_tokens; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.sync_delta_tokens TO nosdesk_app;


--
-- Name: SEQUENCE sync_delta_tokens_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.sync_delta_tokens_id_seq TO nosdesk_app;


--
-- Name: TABLE sync_history; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.sync_history TO nosdesk_app;


--
-- Name: SEQUENCE sync_history_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.sync_history_id_seq TO nosdesk_app;


--
-- Name: TABLE system_meta; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.system_meta TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.system_meta TO nosdesk_admin;


--
-- Name: TABLE tags; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.tags TO nosdesk_app;


--
-- Name: SEQUENCE tags_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.tags_id_seq TO nosdesk_app;


--
-- Name: TABLE ticket_assets; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.ticket_assets TO nosdesk_app;


--
-- Name: TABLE ticket_categories; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.ticket_categories TO nosdesk_app;


--
-- Name: SEQUENCE ticket_categories_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.ticket_categories_id_seq TO nosdesk_app;


--
-- Name: TABLE ticket_rule_runs; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.ticket_rule_runs TO nosdesk_app;


--
-- Name: TABLE ticket_tags; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.ticket_tags TO nosdesk_app;


--
-- Name: TABLE ticket_watchers; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.ticket_watchers TO nosdesk_app;


--
-- Name: TABLE tickets; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.tickets TO nosdesk_app;


--
-- Name: SEQUENCE tickets_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.tickets_id_seq TO nosdesk_app;


--
-- Name: TABLE user_auth_identities; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.user_auth_identities TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.user_auth_identities TO nosdesk_admin;


--
-- Name: SEQUENCE user_auth_identities_id_seq; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT ALL ON SEQUENCE public.user_auth_identities_id_seq TO nosdesk_app;
GRANT ALL ON SEQUENCE public.user_auth_identities_id_seq TO nosdesk_admin;


--
-- Name: TABLE user_emails; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.user_emails TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.user_emails TO nosdesk_admin;


--
-- Name: SEQUENCE user_emails_id_seq; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT ALL ON SEQUENCE public.user_emails_id_seq TO nosdesk_app;
GRANT ALL ON SEQUENCE public.user_emails_id_seq TO nosdesk_admin;


--
-- Name: TABLE user_groups; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.user_groups TO nosdesk_app;


--
-- Name: TABLE user_preferences; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.user_preferences TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.user_preferences TO nosdesk_admin;


--
-- Name: TABLE user_recovery_codes; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.user_recovery_codes TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.user_recovery_codes TO nosdesk_admin;


--
-- Name: SEQUENCE user_recovery_codes_id_seq; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT ALL ON SEQUENCE public.user_recovery_codes_id_seq TO nosdesk_app;
GRANT ALL ON SEQUENCE public.user_recovery_codes_id_seq TO nosdesk_admin;


--
-- Name: TABLE user_ticket_views; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.user_ticket_views TO nosdesk_app;


--
-- Name: SEQUENCE user_ticket_views_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.user_ticket_views_id_seq TO nosdesk_app;


--
-- Name: TABLE users; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.users TO nosdesk_app;
GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.users TO nosdesk_admin;


--
-- Name: TABLE webhook_deliveries; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.webhook_deliveries TO nosdesk_app;


--
-- Name: SEQUENCE webhook_deliveries_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.webhook_deliveries_id_seq TO nosdesk_app;


--
-- Name: TABLE webhook_outbox; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.webhook_outbox TO nosdesk_app;


--
-- Name: TABLE webhooks; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.webhooks TO nosdesk_app;


--
-- Name: SEQUENCE webhooks_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.webhooks_id_seq TO nosdesk_app;


--
-- Name: TABLE workflow_states; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.workflow_states TO nosdesk_app;


--
-- Name: SEQUENCE workflow_states_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.workflow_states_id_seq TO nosdesk_app;


--
-- Name: TABLE working_calendar_holidays; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.working_calendar_holidays TO nosdesk_app;


--
-- Name: SEQUENCE working_calendar_holidays_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.working_calendar_holidays_id_seq TO nosdesk_app;


--
-- Name: TABLE working_calendars; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.working_calendars TO nosdesk_app;


--
-- Name: SEQUENCE working_calendars_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.working_calendars_id_seq TO nosdesk_app;


--
-- Name: TABLE workspace_members; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT ON TABLE public.workspace_members TO nosdesk_app;


--
-- Name: TABLE workspaces; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.workspaces TO nosdesk_admin;
GRANT SELECT ON TABLE public.workspaces TO nosdesk_app;


--
-- Name: SEQUENCE workspaces_id_seq; Type: ACL; Schema: public; Owner: nosdesk
--

GRANT ALL ON SEQUENCE public.workspaces_id_seq TO nosdesk_app;
GRANT ALL ON SEQUENCE public.workspaces_id_seq TO nosdesk_admin;


--
-- Name: TABLE yjs_snapshots; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.yjs_snapshots TO nosdesk_app;


--
-- Name: SEQUENCE yjs_snapshots_id_seq; Type: ACL; Schema: public; Owner: nosdesk_admin
--

GRANT ALL ON SEQUENCE public.yjs_snapshots_id_seq TO nosdesk_app;


--
-- Name: DEFAULT PRIVILEGES FOR SEQUENCES; Type: DEFAULT ACL; Schema: public; Owner: nosdesk
--

ALTER DEFAULT PRIVILEGES FOR ROLE nosdesk IN SCHEMA public GRANT ALL ON SEQUENCES TO nosdesk_app;
ALTER DEFAULT PRIVILEGES FOR ROLE nosdesk IN SCHEMA public GRANT ALL ON SEQUENCES TO nosdesk_admin;


--
-- Name: DEFAULT PRIVILEGES FOR TABLES; Type: DEFAULT ACL; Schema: public; Owner: nosdesk
--

ALTER DEFAULT PRIVILEGES FOR ROLE nosdesk IN SCHEMA public GRANT SELECT,INSERT,DELETE,UPDATE ON TABLES TO nosdesk_app;
ALTER DEFAULT PRIVILEGES FOR ROLE nosdesk IN SCHEMA public GRANT SELECT,INSERT,DELETE,UPDATE ON TABLES TO nosdesk_admin;


--
-- PostgreSQL database dump complete
--



-- ============ SEED DATA ============
--
-- PostgreSQL database dump
--


-- Dumped from database version 18.4 (Debian 18.4-1.pgdg12+1)
-- Dumped by pg_dump version 18.4 (Debian 18.4-1.pgdg12+1)


--
-- Data for Name: users; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

SET SESSION AUTHORIZATION DEFAULT;

ALTER TABLE public.users DISABLE TRIGGER ALL;



ALTER TABLE public.users ENABLE TRIGGER ALL;

--
-- Data for Name: active_sessions; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.active_sessions DISABLE TRIGGER ALL;



ALTER TABLE public.active_sessions ENABLE TRIGGER ALL;

--
-- Data for Name: workspaces; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.workspaces DISABLE TRIGGER ALL;

INSERT INTO public.workspaces VALUES (1, '2d979007-142d-4acf-9a59-35d3543f7c0e', 'default', 'Workspace', 'self_hosted', '{}', '2026-06-06 05:58:59.333557+00', NULL, NULL, NULL);


ALTER TABLE public.workspaces ENABLE TRIGGER ALL;

--
-- Data for Name: api_tokens; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.api_tokens DISABLE TRIGGER ALL;



ALTER TABLE public.api_tokens ENABLE TRIGGER ALL;

--
-- Data for Name: channels; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.channels DISABLE TRIGGER ALL;



ALTER TABLE public.channels ENABLE TRIGGER ALL;

--
-- Data for Name: ticket_categories; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.ticket_categories DISABLE TRIGGER ALL;



ALTER TABLE public.ticket_categories ENABLE TRIGGER ALL;

--
-- Data for Name: workflow_states; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.workflow_states DISABLE TRIGGER ALL;

INSERT INTO public.workflow_states VALUES (1, 'Triage', 'triage', 'slate', 0, false, NULL, '2026-06-06 05:58:59.21425+00', NULL, 1, true);
INSERT INTO public.workflow_states VALUES (2, 'Backlog', 'backlog', 'gray', 0, true, NULL, '2026-06-06 05:58:59.21425+00', NULL, 1, true);
INSERT INTO public.workflow_states VALUES (4, 'In Review', 'in_review', 'purple', 0, false, NULL, '2026-06-06 05:58:59.21425+00', NULL, 1, true);
INSERT INTO public.workflow_states VALUES (5, 'Done', 'done', 'green', 0, false, NULL, '2026-06-06 05:58:59.21425+00', NULL, 1, true);
INSERT INTO public.workflow_states VALUES (6, 'Cancelled', 'cancelled', 'subtle', 0, false, NULL, '2026-06-06 05:58:59.21425+00', NULL, 1, true);
INSERT INTO public.workflow_states VALUES (3, 'In Progress', 'active', 'blue', 0, false, NULL, '2026-06-06 05:58:59.21425+00', NULL, 1, false);
INSERT INTO public.workflow_states VALUES (7, 'Merged', 'merged', 'subtle', 0, false, NULL, '2026-06-06 05:58:59.50906+00', NULL, 1, true);


ALTER TABLE public.workflow_states ENABLE TRIGGER ALL;

--
-- Data for Name: tickets; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.tickets DISABLE TRIGGER ALL;



ALTER TABLE public.tickets ENABLE TRIGGER ALL;

--
-- Data for Name: article_contents; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.article_contents DISABLE TRIGGER ALL;



ALTER TABLE public.article_contents ENABLE TRIGGER ALL;

--
-- Data for Name: article_content_revisions; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.article_content_revisions DISABLE TRIGGER ALL;



ALTER TABLE public.article_content_revisions ENABLE TRIGGER ALL;

--
-- Data for Name: assets; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.assets DISABLE TRIGGER ALL;



ALTER TABLE public.assets ENABLE TRIGGER ALL;

--
-- Data for Name: asset_audits; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.asset_audits DISABLE TRIGGER ALL;



ALTER TABLE public.asset_audits ENABLE TRIGGER ALL;

--
-- Data for Name: groups; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.groups DISABLE TRIGGER ALL;



ALTER TABLE public.groups ENABLE TRIGGER ALL;

--
-- Data for Name: asset_groups; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.asset_groups DISABLE TRIGGER ALL;



ALTER TABLE public.asset_groups ENABLE TRIGGER ALL;

--
-- Data for Name: asset_kinds; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.asset_kinds DISABLE TRIGGER ALL;

INSERT INTO public.asset_kinds VALUES (13, 'generic', 'Generic asset', 'A workspace-neutral asset. Use for anything that does not fit a more specific kind.', 'asset', '{"type": "object", "properties": {}}', 5, true, '2026-06-06 05:58:59.307074+00', '2026-06-06 05:58:59.307074+00', NULL, 'generic', 1);
INSERT INTO public.asset_kinds VALUES (8, 'license', 'License', 'Software license with optional seat tracking.', 'license', '{"type": "object", "properties": {}}', 80, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'logical', 1);
INSERT INTO public.asset_kinds VALUES (9, 'vehicle', 'Vehicle', 'Car, van, truck, trailer.', 'vehicle', '{"type": "object", "properties": {}}', 90, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'physical', 1);
INSERT INTO public.asset_kinds VALUES (10, 'equipment', 'Equipment', 'Tools, machinery, instruments.', 'equipment', '{"type": "object", "properties": {}}', 100, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'physical', 1);
INSERT INTO public.asset_kinds VALUES (11, 'consumable', 'Consumable', 'Items consumed during work (uses quantity + unit).', 'consumable', '{"type": "object", "properties": {}}', 110, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'bulk', 1);
INSERT INTO public.asset_kinds VALUES (12, 'material', 'Material', 'Bulk material tracked by quantity (pipe lengths, cable rolls).', 'material', '{"type": "object", "properties": {}}', 120, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'bulk', 1);
INSERT INTO public.asset_kinds VALUES (1, 'device', 'Device', 'Generic IT device. Default kind for assets created via the legacy /devices path.', 'device', '{"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}', 10, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'it', 1);
INSERT INTO public.asset_kinds VALUES (2, 'laptop', 'Laptop', 'Portable computer assigned to a user.', 'laptop', '{"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}', 20, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'it', 1);
INSERT INTO public.asset_kinds VALUES (3, 'desktop', 'Desktop', 'Workstation computer at a fixed location.', 'desktop', '{"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}', 30, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'it', 1);
INSERT INTO public.asset_kinds VALUES (4, 'server', 'Server', 'Server hardware in a data centre or office.', 'server', '{"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}', 40, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'it', 1);
INSERT INTO public.asset_kinds VALUES (5, 'phone', 'Phone', 'Mobile phone or VoIP handset.', 'phone', '{"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}', 50, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'it', 1);
INSERT INTO public.asset_kinds VALUES (6, 'monitor', 'Monitor', 'External display.', 'monitor', '{"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}', 60, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'it', 1);
INSERT INTO public.asset_kinds VALUES (7, 'network_device', 'Network device', 'Switch, router, access point, firewall.', 'network', '{"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}', 70, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'it', 1);


ALTER TABLE public.asset_kinds ENABLE TRIGGER ALL;

--
-- Data for Name: asset_usage_log; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.asset_usage_log DISABLE TRIGGER ALL;



ALTER TABLE public.asset_usage_log ENABLE TRIGGER ALL;

--
-- Data for Name: assignment_rules; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.assignment_rules DISABLE TRIGGER ALL;



ALTER TABLE public.assignment_rules ENABLE TRIGGER ALL;

--
-- Data for Name: assignment_log; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.assignment_log DISABLE TRIGGER ALL;



ALTER TABLE public.assignment_log ENABLE TRIGGER ALL;

--
-- Data for Name: assignment_rule_state; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.assignment_rule_state DISABLE TRIGGER ALL;



ALTER TABLE public.assignment_rule_state ENABLE TRIGGER ALL;

--
-- Data for Name: comments; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.comments DISABLE TRIGGER ALL;



ALTER TABLE public.comments ENABLE TRIGGER ALL;

--
-- Data for Name: attachments; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.attachments DISABLE TRIGGER ALL;



ALTER TABLE public.attachments ENABLE TRIGGER ALL;

--
-- Data for Name: audit_log_2026_05; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.audit_log_2026_05 DISABLE TRIGGER ALL;



ALTER TABLE public.audit_log_2026_05 ENABLE TRIGGER ALL;

--
-- Data for Name: audit_log_2026_06; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.audit_log_2026_06 DISABLE TRIGGER ALL;

INSERT INTO public.audit_log_2026_06 VALUES (1, 'site_settings', '1', 'U', '{"id": 1, "app_name": "Nosdesk", "logo_url": null, "created_at": "2026-06-06T05:58:59.018294+00:00", "updated_at": "2026-06-06T05:58:59.018294+00:00", "updated_by": null, "favicon_url": null, "feature_flags": {}, "primary_color": null, "logo_light_url": null, "guest_tickets_enabled": false, "guest_help_page_enabled": false, "guest_kb_search_enabled": false, "channel_auto_ack_enabled": true, "channel_auto_ack_template": null, "guest_public_docs_enabled": false, "guest_ticket_intro_message": null, "guest_ticket_lookup_enabled": false, "guest_ticket_default_priority": null, "guest_ticket_email_verification": true, "guest_ticket_attachments_enabled": true, "guest_ticket_rate_limit_per_hour": 5}', '{"id": 1, "app_name": "Nosdesk", "logo_url": null, "created_at": "2026-06-06T05:58:59.018294+00:00", "updated_at": "2026-06-06T05:58:59.236554+00:00", "updated_by": null, "favicon_url": null, "feature_flags": {"projects_v2": true}, "primary_color": null, "logo_light_url": null, "guest_tickets_enabled": false, "guest_help_page_enabled": false, "guest_kb_search_enabled": false, "channel_auto_ack_enabled": true, "channel_auto_ack_template": null, "guest_public_docs_enabled": false, "guest_ticket_intro_message": null, "guest_ticket_lookup_enabled": false, "guest_ticket_default_priority": null, "guest_ticket_email_verification": true, "guest_ticket_attachments_enabled": true, "guest_ticket_rate_limit_per_hour": 5}', '{updated_at,feature_flags}', NULL, NULL, '2026-06-06 05:58:59.237271+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (2, 'site_settings', '1', 'U', '{"id": 1, "app_name": "Nosdesk", "logo_url": null, "created_at": "2026-06-06T05:58:59.018294+00:00", "updated_at": "2026-06-06T05:58:59.236554+00:00", "updated_by": null, "favicon_url": null, "feature_flags": {"projects_v2": true}, "primary_color": null, "logo_light_url": null, "guest_tickets_enabled": false, "guest_help_page_enabled": false, "guest_kb_search_enabled": false, "channel_auto_ack_enabled": true, "channel_auto_ack_template": null, "guest_public_docs_enabled": false, "guest_ticket_intro_message": null, "guest_ticket_lookup_enabled": false, "guest_ticket_default_priority": null, "guest_ticket_email_verification": true, "guest_ticket_attachments_enabled": true, "guest_ticket_rate_limit_per_hour": 5}', '{"id": 1, "app_name": "Nosdesk", "logo_url": null, "created_at": "2026-06-06T05:58:59.018294+00:00", "updated_at": "2026-06-06T05:58:59.238807+00:00", "updated_by": null, "favicon_url": null, "feature_flags": {}, "primary_color": null, "logo_light_url": null, "guest_tickets_enabled": false, "guest_help_page_enabled": false, "guest_kb_search_enabled": false, "channel_auto_ack_enabled": true, "channel_auto_ack_template": null, "guest_public_docs_enabled": false, "guest_ticket_intro_message": null, "guest_ticket_lookup_enabled": false, "guest_ticket_default_priority": null, "guest_ticket_email_verification": true, "guest_ticket_attachments_enabled": true, "guest_ticket_rate_limit_per_hour": 5}', '{updated_at,feature_flags}', NULL, NULL, '2026-06-06 05:58:59.239076+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (3, 'asset_kinds', '13', 'I', NULL, '{"id": 13, "icon": "asset", "slug": "generic", "label": "Generic asset", "created_at": "2026-06-06T05:58:59.307074+00:00", "created_by": null, "is_builtin": true, "sort_order": 5, "updated_at": "2026-06-06T05:58:59.307074+00:00", "description": "A workspace-neutral asset. Use for anything that does not fit a more specific kind.", "attribute_schema": {"type": "object", "properties": {}}}', NULL, NULL, NULL, '2026-06-06 05:58:59.307359+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (4, 'asset_kinds', '1', 'U', '{"id": 1, "icon": "device", "slug": "device", "label": "Device", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 10, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Generic IT device. Default kind for assets created via the legacy /devices path.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 1, "icon": "device", "slug": "device", "label": "Device", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 10, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Generic IT device. Default kind for assets created via the legacy /devices path.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.308976+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (5, 'asset_kinds', '2', 'U', '{"id": 2, "icon": "laptop", "slug": "laptop", "label": "Laptop", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 20, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Portable computer assigned to a user.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 2, "icon": "laptop", "slug": "laptop", "label": "Laptop", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 20, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Portable computer assigned to a user.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.309055+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (6, 'asset_kinds', '3', 'U', '{"id": 3, "icon": "desktop", "slug": "desktop", "label": "Desktop", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 30, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Workstation computer at a fixed location.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 3, "icon": "desktop", "slug": "desktop", "label": "Desktop", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 30, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Workstation computer at a fixed location.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.309129+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (7, 'asset_kinds', '4', 'U', '{"id": 4, "icon": "server", "slug": "server", "label": "Server", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 40, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Server hardware in a data centre or office.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 4, "icon": "server", "slug": "server", "label": "Server", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 40, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Server hardware in a data centre or office.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.309199+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (8, 'asset_kinds', '5', 'U', '{"id": 5, "icon": "phone", "slug": "phone", "label": "Phone", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 50, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Mobile phone or VoIP handset.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 5, "icon": "phone", "slug": "phone", "label": "Phone", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 50, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Mobile phone or VoIP handset.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.309268+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (9, 'asset_kinds', '6', 'U', '{"id": 6, "icon": "monitor", "slug": "monitor", "label": "Monitor", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 60, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "External display.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 6, "icon": "monitor", "slug": "monitor", "label": "Monitor", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 60, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "External display.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.30934+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (10, 'asset_kinds', '7', 'U', '{"id": 7, "icon": "network", "slug": "network_device", "label": "Network device", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 70, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Switch, router, access point, firewall.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 7, "icon": "network", "slug": "network_device", "label": "Network device", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 70, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Switch, router, access point, firewall.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.30955+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (11, 'asset_kinds', '8', 'U', '{"id": 8, "icon": "license", "slug": "license", "label": "License", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 80, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Software license with optional seat tracking.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 8, "icon": "license", "slug": "license", "label": "License", "category": "logical", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 80, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Software license with optional seat tracking.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.309638+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (12, 'asset_kinds', '9', 'U', '{"id": 9, "icon": "vehicle", "slug": "vehicle", "label": "Vehicle", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 90, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Car, van, truck, trailer.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 9, "icon": "vehicle", "slug": "vehicle", "label": "Vehicle", "category": "physical", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 90, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Car, van, truck, trailer.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.30973+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (13, 'asset_kinds', '10', 'U', '{"id": 10, "icon": "equipment", "slug": "equipment", "label": "Equipment", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 100, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Tools, machinery, instruments.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 10, "icon": "equipment", "slug": "equipment", "label": "Equipment", "category": "physical", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 100, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Tools, machinery, instruments.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.309791+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (14, 'asset_kinds', '11', 'U', '{"id": 11, "icon": "consumable", "slug": "consumable", "label": "Consumable", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 110, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Items consumed during work (uses quantity + unit).", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 11, "icon": "consumable", "slug": "consumable", "label": "Consumable", "category": "bulk", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 110, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Items consumed during work (uses quantity + unit).", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.309875+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (15, 'asset_kinds', '12', 'U', '{"id": 12, "icon": "material", "slug": "material", "label": "Material", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 120, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Bulk material tracked by quantity (pipe lengths, cable rolls).", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 12, "icon": "material", "slug": "material", "label": "Material", "category": "bulk", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 120, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Bulk material tracked by quantity (pipe lengths, cable rolls).", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.309935+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (16, 'asset_kinds', '1', 'U', '{"id": 1, "icon": "device", "slug": "device", "label": "Device", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 10, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Generic IT device. Default kind for assets created via the legacy /devices path.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 1, "icon": "device", "slug": "device", "label": "Device", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 10, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Generic IT device. Default kind for assets created via the legacy /devices path.", "attribute_schema": {"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}}', '{attribute_schema}', NULL, NULL, '2026-06-06 05:58:59.31314+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (17, 'asset_kinds', '2', 'U', '{"id": 2, "icon": "laptop", "slug": "laptop", "label": "Laptop", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 20, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Portable computer assigned to a user.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 2, "icon": "laptop", "slug": "laptop", "label": "Laptop", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 20, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Portable computer assigned to a user.", "attribute_schema": {"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}}', '{attribute_schema}', NULL, NULL, '2026-06-06 05:58:59.313263+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (18, 'asset_kinds', '3', 'U', '{"id": 3, "icon": "desktop", "slug": "desktop", "label": "Desktop", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 30, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Workstation computer at a fixed location.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 3, "icon": "desktop", "slug": "desktop", "label": "Desktop", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 30, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Workstation computer at a fixed location.", "attribute_schema": {"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}}', '{attribute_schema}', NULL, NULL, '2026-06-06 05:58:59.313395+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (19, 'asset_kinds', '4', 'U', '{"id": 4, "icon": "server", "slug": "server", "label": "Server", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 40, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Server hardware in a data centre or office.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 4, "icon": "server", "slug": "server", "label": "Server", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 40, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Server hardware in a data centre or office.", "attribute_schema": {"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}}', '{attribute_schema}', NULL, NULL, '2026-06-06 05:58:59.313504+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (20, 'asset_kinds', '5', 'U', '{"id": 5, "icon": "phone", "slug": "phone", "label": "Phone", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 50, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Mobile phone or VoIP handset.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 5, "icon": "phone", "slug": "phone", "label": "Phone", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 50, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Mobile phone or VoIP handset.", "attribute_schema": {"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}}', '{attribute_schema}', NULL, NULL, '2026-06-06 05:58:59.313595+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (21, 'asset_kinds', '6', 'U', '{"id": 6, "icon": "monitor", "slug": "monitor", "label": "Monitor", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 60, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "External display.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 6, "icon": "monitor", "slug": "monitor", "label": "Monitor", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 60, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "External display.", "attribute_schema": {"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}}', '{attribute_schema}', NULL, NULL, '2026-06-06 05:58:59.313673+00', 1);
INSERT INTO public.audit_log_2026_06 VALUES (22, 'asset_kinds', '7', 'U', '{"id": 7, "icon": "network", "slug": "network_device", "label": "Network device", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 70, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Switch, router, access point, firewall.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 7, "icon": "network", "slug": "network_device", "label": "Network device", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 70, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Switch, router, access point, firewall.", "attribute_schema": {"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}}', '{attribute_schema}', NULL, NULL, '2026-06-06 05:58:59.313781+00', 1);


ALTER TABLE public.audit_log_2026_06 ENABLE TRIGGER ALL;

--
-- Data for Name: audit_log_2026_07; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.audit_log_2026_07 DISABLE TRIGGER ALL;



ALTER TABLE public.audit_log_2026_07 ENABLE TRIGGER ALL;

--
-- Data for Name: audit_log_2026_08; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.audit_log_2026_08 DISABLE TRIGGER ALL;



ALTER TABLE public.audit_log_2026_08 ENABLE TRIGGER ALL;

--
-- Data for Name: audit_log_default; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.audit_log_default DISABLE TRIGGER ALL;



ALTER TABLE public.audit_log_default ENABLE TRIGGER ALL;

--
-- Data for Name: backup_jobs; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.backup_jobs DISABLE TRIGGER ALL;



ALTER TABLE public.backup_jobs ENABLE TRIGGER ALL;

--
-- Data for Name: bug_reports; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.bug_reports DISABLE TRIGGER ALL;



ALTER TABLE public.bug_reports ENABLE TRIGGER ALL;

--
-- Data for Name: canned_responses; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.canned_responses DISABLE TRIGGER ALL;



ALTER TABLE public.canned_responses ENABLE TRIGGER ALL;

--
-- Data for Name: canned_response_insertions; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.canned_response_insertions DISABLE TRIGGER ALL;



ALTER TABLE public.canned_response_insertions ENABLE TRIGGER ALL;

--
-- Data for Name: category_group_visibility; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.category_group_visibility DISABLE TRIGGER ALL;



ALTER TABLE public.category_group_visibility ENABLE TRIGGER ALL;

--
-- Data for Name: channel_credentials; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.channel_credentials DISABLE TRIGGER ALL;



ALTER TABLE public.channel_credentials ENABLE TRIGGER ALL;

--
-- Data for Name: channel_messages; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.channel_messages DISABLE TRIGGER ALL;



ALTER TABLE public.channel_messages ENABLE TRIGGER ALL;

--
-- Data for Name: csp_reports; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.csp_reports DISABLE TRIGGER ALL;



ALTER TABLE public.csp_reports ENABLE TRIGGER ALL;

--
-- Data for Name: projects; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.projects DISABLE TRIGGER ALL;



ALTER TABLE public.projects ENABLE TRIGGER ALL;

--
-- Data for Name: cycles; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.cycles DISABLE TRIGGER ALL;



ALTER TABLE public.cycles ENABLE TRIGGER ALL;

--
-- Data for Name: cycle_tickets; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.cycle_tickets DISABLE TRIGGER ALL;



ALTER TABLE public.cycle_tickets ENABLE TRIGGER ALL;

--
-- Data for Name: documentation_collections; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_collections DISABLE TRIGGER ALL;

INSERT INTO public.documentation_collections VALUES (2, 'd130d0e1-8c8a-4664-86e2-aeca506838ac', 'Getting Started', 'getting-started', 'Introduction and onboarding documentation', '🚀', NULL, true, NULL, '2026-06-06 05:58:59.118175+00', '2026-06-06 05:58:59.118175+00', 0, NULL, NULL, NULL, false, 1);
INSERT INTO public.documentation_collections VALUES (1, '9744d4a0-4ad0-4940-8c9c-f759a0e7902a', 'Tickets', 'tickets', 'Documentation pages created from ticket notes', '🎫', NULL, true, NULL, '2026-06-06 05:58:59.118175+00', '2026-06-06 05:58:59.118175+00', 1, NULL, NULL, NULL, false, 1);


ALTER TABLE public.documentation_collections ENABLE TRIGGER ALL;

--
-- Data for Name: documentation_pages; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_pages DISABLE TRIGGER ALL;



ALTER TABLE public.documentation_pages ENABLE TRIGGER ALL;

--
-- Data for Name: documentation_collection_pages; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_collection_pages DISABLE TRIGGER ALL;



ALTER TABLE public.documentation_collection_pages ENABLE TRIGGER ALL;

--
-- Data for Name: documentation_collection_visibility; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_collection_visibility DISABLE TRIGGER ALL;



ALTER TABLE public.documentation_collection_visibility ENABLE TRIGGER ALL;

--
-- Data for Name: documentation_page_embeddings; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_page_embeddings DISABLE TRIGGER ALL;



ALTER TABLE public.documentation_page_embeddings ENABLE TRIGGER ALL;

--
-- Data for Name: documentation_page_tickets; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_page_tickets DISABLE TRIGGER ALL;



ALTER TABLE public.documentation_page_tickets ENABLE TRIGGER ALL;

--
-- Data for Name: documentation_page_visibility; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_page_visibility DISABLE TRIGGER ALL;



ALTER TABLE public.documentation_page_visibility ENABLE TRIGGER ALL;

--
-- Data for Name: documentation_revisions; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_revisions DISABLE TRIGGER ALL;



ALTER TABLE public.documentation_revisions ENABLE TRIGGER ALL;

--
-- Data for Name: documentation_starred_pages; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_starred_pages DISABLE TRIGGER ALL;



ALTER TABLE public.documentation_starred_pages ENABLE TRIGGER ALL;

--
-- Data for Name: documentation_subscriptions; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.documentation_subscriptions DISABLE TRIGGER ALL;



ALTER TABLE public.documentation_subscriptions ENABLE TRIGGER ALL;

--
-- Data for Name: email_suppressions; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.email_suppressions DISABLE TRIGGER ALL;



ALTER TABLE public.email_suppressions ENABLE TRIGGER ALL;

--
-- Data for Name: group_includes; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.group_includes DISABLE TRIGGER ALL;



ALTER TABLE public.group_includes ENABLE TRIGGER ALL;

--
-- Data for Name: idempotency_keys; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.idempotency_keys DISABLE TRIGGER ALL;



ALTER TABLE public.idempotency_keys ENABLE TRIGGER ALL;

--
-- Data for Name: import_jobs; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.import_jobs DISABLE TRIGGER ALL;



ALTER TABLE public.import_jobs ENABLE TRIGGER ALL;

--
-- Data for Name: knowledge_gaps; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.knowledge_gaps DISABLE TRIGGER ALL;



ALTER TABLE public.knowledge_gaps ENABLE TRIGGER ALL;

--
-- Data for Name: knowledge_gap_signals; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.knowledge_gap_signals DISABLE TRIGGER ALL;



ALTER TABLE public.knowledge_gap_signals ENABLE TRIGGER ALL;

--
-- Data for Name: linked_tickets; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.linked_tickets DISABLE TRIGGER ALL;



ALTER TABLE public.linked_tickets ENABLE TRIGGER ALL;

--
-- Data for Name: notification_types; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.notification_types DISABLE TRIGGER ALL;

INSERT INTO public.notification_types VALUES (1, 'ticket_assigned', 'Assigned to Ticket', 'When you are assigned to a ticket', 'ticket', '["in_app", "email"]', '2026-06-06 05:58:59.083307+00');
INSERT INTO public.notification_types VALUES (2, 'ticket_status_changed', 'Ticket Status Changed', 'When a ticket you are involved with changes status', 'ticket', '["in_app"]', '2026-06-06 05:58:59.083307+00');
INSERT INTO public.notification_types VALUES (3, 'comment_added', 'New Comment', 'When someone comments on a ticket you are involved with', 'comment', '["in_app"]', '2026-06-06 05:58:59.083307+00');
INSERT INTO public.notification_types VALUES (4, 'mentioned', 'Mentioned in Comment', 'When someone mentions you with @username', 'mention', '["in_app", "email"]', '2026-06-06 05:58:59.083307+00');
INSERT INTO public.notification_types VALUES (5, 'ticket_created_requester', 'Ticket Created', 'When a ticket is created where you are the requester', 'ticket', '["in_app"]', '2026-06-06 05:58:59.083307+00');
INSERT INTO public.notification_types VALUES (6, 'doc_page_updated', 'Documentation Page Updated', 'When a documentation page you subscribe to is modified', 'documentation', '["in_app"]', '2026-06-06 05:58:59.138446+00');
INSERT INTO public.notification_types VALUES (7, 'asset_low_stock', 'Asset Low Stock', 'When a stock-tracked asset''s quantity drops to or below its low-stock threshold', 'asset', '["in_app", "email"]', '2026-06-06 05:58:59.325193+00');
INSERT INTO public.notification_types VALUES (8, 'sla_breached', 'SLA Breached', 'When a ticket''s response or resolution SLA target has been missed', 'ticket', '["in_app", "email"]', '2026-06-06 05:58:59.462702+00');


ALTER TABLE public.notification_types ENABLE TRIGGER ALL;

--
-- Data for Name: notification_preferences; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.notification_preferences DISABLE TRIGGER ALL;



ALTER TABLE public.notification_preferences ENABLE TRIGGER ALL;

--
-- Data for Name: notification_rate_limits; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.notification_rate_limits DISABLE TRIGGER ALL;



ALTER TABLE public.notification_rate_limits ENABLE TRIGGER ALL;

--
-- Data for Name: notifications; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.notifications DISABLE TRIGGER ALL;



ALTER TABLE public.notifications ENABLE TRIGGER ALL;

--
-- Data for Name: outbound_emails; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.outbound_emails DISABLE TRIGGER ALL;



ALTER TABLE public.outbound_emails ENABLE TRIGGER ALL;

--
-- Data for Name: passkey_credentials; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.passkey_credentials DISABLE TRIGGER ALL;



ALTER TABLE public.passkey_credentials ENABLE TRIGGER ALL;

--
-- Data for Name: plugins; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.plugins DISABLE TRIGGER ALL;



ALTER TABLE public.plugins ENABLE TRIGGER ALL;

--
-- Data for Name: plugin_activity; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.plugin_activity DISABLE TRIGGER ALL;



ALTER TABLE public.plugin_activity ENABLE TRIGGER ALL;

--
-- Data for Name: plugin_collection_schemas; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.plugin_collection_schemas DISABLE TRIGGER ALL;



ALTER TABLE public.plugin_collection_schemas ENABLE TRIGGER ALL;

--
-- Data for Name: plugin_collection_rows; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.plugin_collection_rows DISABLE TRIGGER ALL;



ALTER TABLE public.plugin_collection_rows ENABLE TRIGGER ALL;

--
-- Data for Name: plugin_data; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.plugin_data DISABLE TRIGGER ALL;



ALTER TABLE public.plugin_data ENABLE TRIGGER ALL;

--
-- Data for Name: plugin_local_signing_key; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.plugin_local_signing_key DISABLE TRIGGER ALL;



ALTER TABLE public.plugin_local_signing_key ENABLE TRIGGER ALL;

--
-- Data for Name: plugin_registry_state; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.plugin_registry_state DISABLE TRIGGER ALL;

INSERT INTO public.plugin_registry_state VALUES (1, 0, 0, NULL, NULL, '2026-06-06 05:58:59.183222+00');


ALTER TABLE public.plugin_registry_state ENABLE TRIGGER ALL;

--
-- Data for Name: plugin_trusted_publishers; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.plugin_trusted_publishers DISABLE TRIGGER ALL;



ALTER TABLE public.plugin_trusted_publishers ENABLE TRIGGER ALL;

--
-- Data for Name: project_tickets; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.project_tickets DISABLE TRIGGER ALL;



ALTER TABLE public.project_tickets ENABLE TRIGGER ALL;

--
-- Data for Name: refresh_tokens; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.refresh_tokens DISABLE TRIGGER ALL;



ALTER TABLE public.refresh_tokens ENABLE TRIGGER ALL;

--
-- Data for Name: reset_tokens; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.reset_tokens DISABLE TRIGGER ALL;



ALTER TABLE public.reset_tokens ENABLE TRIGGER ALL;

--
-- Data for Name: rules; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.rules DISABLE TRIGGER ALL;



ALTER TABLE public.rules ENABLE TRIGGER ALL;

--
-- Data for Name: rule_applications; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.rule_applications DISABLE TRIGGER ALL;



ALTER TABLE public.rule_applications ENABLE TRIGGER ALL;

--
-- Data for Name: rule_versions; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.rule_versions DISABLE TRIGGER ALL;



ALTER TABLE public.rule_versions ENABLE TRIGGER ALL;

--
-- Data for Name: saved_views; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.saved_views DISABLE TRIGGER ALL;



ALTER TABLE public.saved_views ENABLE TRIGGER ALL;

--
-- Data for Name: search_index_state; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.search_index_state DISABLE TRIGGER ALL;

INSERT INTO public.search_index_state VALUES (1, 'ticket', NULL, 1, 0, NULL, NULL, '2026-06-06 05:58:59.115781+00', '2026-06-06 05:58:59.115781+00');
INSERT INTO public.search_index_state VALUES (2, 'comment', NULL, 1, 0, NULL, NULL, '2026-06-06 05:58:59.115781+00', '2026-06-06 05:58:59.115781+00');
INSERT INTO public.search_index_state VALUES (3, 'documentation', NULL, 1, 0, NULL, NULL, '2026-06-06 05:58:59.115781+00', '2026-06-06 05:58:59.115781+00');
INSERT INTO public.search_index_state VALUES (4, 'attachment', NULL, 1, 0, NULL, NULL, '2026-06-06 05:58:59.115781+00', '2026-06-06 05:58:59.115781+00');
INSERT INTO public.search_index_state VALUES (5, 'device', NULL, 1, 0, NULL, NULL, '2026-06-06 05:58:59.115781+00', '2026-06-06 05:58:59.115781+00');
INSERT INTO public.search_index_state VALUES (6, 'user', NULL, 1, 0, NULL, NULL, '2026-06-06 05:58:59.115781+00', '2026-06-06 05:58:59.115781+00');


ALTER TABLE public.search_index_state ENABLE TRIGGER ALL;

--
-- Data for Name: search_query_log; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.search_query_log DISABLE TRIGGER ALL;



ALTER TABLE public.search_query_log ENABLE TRIGGER ALL;

--
-- Data for Name: security_events; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.security_events DISABLE TRIGGER ALL;



ALTER TABLE public.security_events ENABLE TRIGGER ALL;

--
-- Data for Name: site_settings; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.site_settings DISABLE TRIGGER ALL;

INSERT INTO public.site_settings VALUES (1, 'Nosdesk', NULL, NULL, NULL, NULL, '2026-06-06 05:58:59.018294+00', '2026-06-06 05:58:59.238807+00', NULL, false, false, false, false, false, NULL, 5, true, true, NULL, true, NULL, '{}', 'en-US', 'UTC', 1, NULL);


ALTER TABLE public.site_settings ENABLE TRIGGER ALL;

--
-- Data for Name: working_calendars; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.working_calendars DISABLE TRIGGER ALL;

INSERT INTO public.working_calendars VALUES (1, 'Default 9-5', 'UTC', '{"fri": [["09:00", "17:00"]], "mon": [["09:00", "17:00"]], "sat": [], "sun": [], "thu": [["09:00", "17:00"]], "tue": [["09:00", "17:00"]], "wed": [["09:00", "17:00"]]}', true, '2026-06-06 05:58:59.25774+00', '2026-06-06 05:58:59.25774+00', NULL, 1);


ALTER TABLE public.working_calendars ENABLE TRIGGER ALL;

--
-- Data for Name: sla_policies; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.sla_policies DISABLE TRIGGER ALL;

INSERT INTO public.sla_policies VALUES (1, 'Default', 240, 1440, 1, NULL, NULL, true, '2026-06-06 05:58:59.25774+00', '2026-06-06 05:58:59.25774+00', NULL, 1, NULL);


ALTER TABLE public.sla_policies ENABLE TRIGGER ALL;

--
-- Data for Name: sync_actions_2026_05; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.sync_actions_2026_05 DISABLE TRIGGER ALL;



ALTER TABLE public.sync_actions_2026_05 ENABLE TRIGGER ALL;

--
-- Data for Name: sync_actions_2026_06; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.sync_actions_2026_06 DISABLE TRIGGER ALL;



ALTER TABLE public.sync_actions_2026_06 ENABLE TRIGGER ALL;

--
-- Data for Name: sync_actions_2026_07; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.sync_actions_2026_07 DISABLE TRIGGER ALL;



ALTER TABLE public.sync_actions_2026_07 ENABLE TRIGGER ALL;

--
-- Data for Name: sync_actions_2026_08; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.sync_actions_2026_08 DISABLE TRIGGER ALL;



ALTER TABLE public.sync_actions_2026_08 ENABLE TRIGGER ALL;

--
-- Data for Name: sync_actions_default; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.sync_actions_default DISABLE TRIGGER ALL;



ALTER TABLE public.sync_actions_default ENABLE TRIGGER ALL;

--
-- Data for Name: sync_delta_tokens; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.sync_delta_tokens DISABLE TRIGGER ALL;



ALTER TABLE public.sync_delta_tokens ENABLE TRIGGER ALL;

--
-- Data for Name: sync_history; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.sync_history DISABLE TRIGGER ALL;



ALTER TABLE public.sync_history ENABLE TRIGGER ALL;

--
-- Data for Name: system_meta; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.system_meta DISABLE TRIGGER ALL;

INSERT INTO public.system_meta VALUES ('schema_hash', '""', '2026-06-06 05:58:59.217917+00');
INSERT INTO public.system_meta VALUES ('sync_id_high_water', '0', '2026-06-06 05:58:59.217917+00');
INSERT INTO public.system_meta VALUES ('partition_max_provisioned', '"2026-09-01"', '2026-06-06 05:58:59.217917+00');


ALTER TABLE public.system_meta ENABLE TRIGGER ALL;

--
-- Data for Name: tags; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.tags DISABLE TRIGGER ALL;



ALTER TABLE public.tags ENABLE TRIGGER ALL;

--
-- Data for Name: ticket_assets; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.ticket_assets DISABLE TRIGGER ALL;



ALTER TABLE public.ticket_assets ENABLE TRIGGER ALL;

--
-- Data for Name: ticket_rule_runs; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.ticket_rule_runs DISABLE TRIGGER ALL;



ALTER TABLE public.ticket_rule_runs ENABLE TRIGGER ALL;

--
-- Data for Name: ticket_tags; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.ticket_tags DISABLE TRIGGER ALL;



ALTER TABLE public.ticket_tags ENABLE TRIGGER ALL;

--
-- Data for Name: ticket_watchers; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.ticket_watchers DISABLE TRIGGER ALL;



ALTER TABLE public.ticket_watchers ENABLE TRIGGER ALL;

--
-- Data for Name: user_auth_identities; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.user_auth_identities DISABLE TRIGGER ALL;



ALTER TABLE public.user_auth_identities ENABLE TRIGGER ALL;

--
-- Data for Name: user_emails; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.user_emails DISABLE TRIGGER ALL;



ALTER TABLE public.user_emails ENABLE TRIGGER ALL;

--
-- Data for Name: user_groups; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.user_groups DISABLE TRIGGER ALL;



ALTER TABLE public.user_groups ENABLE TRIGGER ALL;

--
-- Data for Name: user_preferences; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.user_preferences DISABLE TRIGGER ALL;



ALTER TABLE public.user_preferences ENABLE TRIGGER ALL;

--
-- Data for Name: user_recovery_codes; Type: TABLE DATA; Schema: public; Owner: nosdesk
--

ALTER TABLE public.user_recovery_codes DISABLE TRIGGER ALL;



ALTER TABLE public.user_recovery_codes ENABLE TRIGGER ALL;

--
-- Data for Name: user_ticket_views; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.user_ticket_views DISABLE TRIGGER ALL;



ALTER TABLE public.user_ticket_views ENABLE TRIGGER ALL;

--
-- Data for Name: webhooks; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.webhooks DISABLE TRIGGER ALL;



ALTER TABLE public.webhooks ENABLE TRIGGER ALL;

--
-- Data for Name: webhook_deliveries; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.webhook_deliveries DISABLE TRIGGER ALL;



ALTER TABLE public.webhook_deliveries ENABLE TRIGGER ALL;

--
-- Data for Name: webhook_outbox; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.webhook_outbox DISABLE TRIGGER ALL;



ALTER TABLE public.webhook_outbox ENABLE TRIGGER ALL;

--
-- Data for Name: working_calendar_holidays; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.working_calendar_holidays DISABLE TRIGGER ALL;



ALTER TABLE public.working_calendar_holidays ENABLE TRIGGER ALL;

--
-- Data for Name: workspace_members; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.workspace_members DISABLE TRIGGER ALL;



ALTER TABLE public.workspace_members ENABLE TRIGGER ALL;

--
-- Data for Name: yjs_snapshots; Type: TABLE DATA; Schema: public; Owner: nosdesk_admin
--

ALTER TABLE public.yjs_snapshots DISABLE TRIGGER ALL;



ALTER TABLE public.yjs_snapshots ENABLE TRIGGER ALL;

--
-- Name: active_sessions_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk
--

SELECT pg_catalog.setval('public.active_sessions_id_seq', 1, false);


--
-- Name: api_tokens_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.api_tokens_id_seq', 1, false);


--
-- Name: article_content_revisions_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.article_content_revisions_id_seq', 1, false);


--
-- Name: article_contents_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.article_contents_id_seq', 1, false);


--
-- Name: asset_audits_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.asset_audits_id_seq', 1, false);


--
-- Name: asset_kinds_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.asset_kinds_id_seq', 13, true);


--
-- Name: asset_usage_log_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.asset_usage_log_id_seq', 1, false);


--
-- Name: assignment_log_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.assignment_log_id_seq', 1, false);


--
-- Name: assignment_rules_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.assignment_rules_id_seq', 1, false);


--
-- Name: attachments_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.attachments_id_seq', 1, false);


--
-- Name: audit_log_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.audit_log_id_seq', 24, true);


--
-- Name: bug_reports_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.bug_reports_id_seq', 1, false);


--
-- Name: canned_response_insertions_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.canned_response_insertions_id_seq', 1, false);


--
-- Name: canned_responses_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.canned_responses_id_seq', 1, false);


--
-- Name: channel_credentials_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.channel_credentials_id_seq', 1, false);


--
-- Name: channel_messages_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.channel_messages_id_seq', 1, false);


--
-- Name: channels_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.channels_id_seq', 1, false);


--
-- Name: comments_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.comments_id_seq', 1, false);


--
-- Name: csp_reports_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.csp_reports_id_seq', 1, false);


--
-- Name: cycles_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.cycles_id_seq', 1, false);


--
-- Name: devices_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.devices_id_seq', 1, false);


--
-- Name: documentation_collection_visibility_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.documentation_collection_visibility_id_seq', 1, false);


--
-- Name: documentation_collections_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.documentation_collections_id_seq', 2, true);


--
-- Name: documentation_page_visibility_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.documentation_page_visibility_id_seq', 1, false);


--
-- Name: documentation_pages_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.documentation_pages_id_seq', 1, false);


--
-- Name: documentation_revisions_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.documentation_revisions_id_seq', 1, false);


--
-- Name: documentation_starred_pages_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.documentation_starred_pages_id_seq', 1, false);


--
-- Name: documentation_subscriptions_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.documentation_subscriptions_id_seq', 1, false);


--
-- Name: groups_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.groups_id_seq', 1, false);


--
-- Name: knowledge_gap_signals_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.knowledge_gap_signals_id_seq', 1, false);


--
-- Name: knowledge_gaps_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.knowledge_gaps_id_seq', 1, false);


--
-- Name: notification_preferences_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.notification_preferences_id_seq', 1, false);


--
-- Name: notification_rate_limits_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk
--

SELECT pg_catalog.setval('public.notification_rate_limits_id_seq', 1, false);


--
-- Name: notification_types_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk
--

SELECT pg_catalog.setval('public.notification_types_id_seq', 8, true);


--
-- Name: notifications_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.notifications_id_seq', 1, false);


--
-- Name: outbound_emails_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.outbound_emails_id_seq', 1, false);


--
-- Name: plugin_activity_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.plugin_activity_id_seq', 1, false);


--
-- Name: plugin_collection_rows_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.plugin_collection_rows_id_seq', 1, false);


--
-- Name: plugin_collection_schemas_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.plugin_collection_schemas_id_seq', 1, false);


--
-- Name: plugin_data_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.plugin_data_id_seq', 1, false);


--
-- Name: plugin_trusted_publishers_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk
--

SELECT pg_catalog.setval('public.plugin_trusted_publishers_id_seq', 1, false);


--
-- Name: plugins_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.plugins_id_seq', 1, false);


--
-- Name: projects_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.projects_id_seq', 1, false);


--
-- Name: refresh_tokens_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk
--

SELECT pg_catalog.setval('public.refresh_tokens_id_seq', 1, false);


--
-- Name: rule_applications_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.rule_applications_id_seq', 1, false);


--
-- Name: rule_versions_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.rule_versions_id_seq', 1, false);


--
-- Name: rules_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.rules_id_seq', 1, false);


--
-- Name: saved_views_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.saved_views_id_seq', 1, false);


--
-- Name: search_index_state_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk
--

SELECT pg_catalog.setval('public.search_index_state_id_seq', 6, true);


--
-- Name: search_query_log_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.search_query_log_id_seq', 1, false);


--
-- Name: security_events_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk
--

SELECT pg_catalog.setval('public.security_events_id_seq', 1, false);


--
-- Name: sla_policies_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.sla_policies_id_seq', 1, true);


--
-- Name: sync_actions_sync_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.sync_actions_sync_id_seq', 1, false);


--
-- Name: sync_delta_tokens_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.sync_delta_tokens_id_seq', 1, false);


--
-- Name: sync_history_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.sync_history_id_seq', 1, false);


--
-- Name: tags_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.tags_id_seq', 1, false);


--
-- Name: ticket_categories_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.ticket_categories_id_seq', 1, true);


--
-- Name: tickets_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.tickets_id_seq', 1, false);


--
-- Name: user_auth_identities_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk
--

SELECT pg_catalog.setval('public.user_auth_identities_id_seq', 1, false);


--
-- Name: user_emails_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk
--

SELECT pg_catalog.setval('public.user_emails_id_seq', 1, false);


--
-- Name: user_recovery_codes_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk
--

SELECT pg_catalog.setval('public.user_recovery_codes_id_seq', 1, false);


--
-- Name: user_ticket_views_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.user_ticket_views_id_seq', 1, false);


--
-- Name: webhook_deliveries_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.webhook_deliveries_id_seq', 1, false);


--
-- Name: webhooks_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.webhooks_id_seq', 1, false);


--
-- Name: workflow_states_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.workflow_states_id_seq', 7, true);


--
-- Name: working_calendar_holidays_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.working_calendar_holidays_id_seq', 1, false);


--
-- Name: working_calendars_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.working_calendars_id_seq', 1, true);


--
-- Name: workspaces_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk
--

SELECT pg_catalog.setval('public.workspaces_id_seq', 1, true);


--
-- Name: yjs_snapshots_id_seq; Type: SEQUENCE SET; Schema: public; Owner: nosdesk_admin
--

SELECT pg_catalog.setval('public.yjs_snapshots_id_seq', 1, false);


--
-- PostgreSQL database dump complete
--


