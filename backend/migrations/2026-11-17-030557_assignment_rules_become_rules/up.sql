-- Assignment rules become rules (docs/plans/1.2-automation.md, A8). The rules
-- engine's event consumer assigns new tickets now, so each assignment rule is
-- copied into `rules` and the assignment tables go.
--
-- An assignment rule becomes one event rule for each trigger it ran on: on
-- create a `ticket_created` rule, on a category change a `ticket_updated` rule
-- that runs when `ticket.category_id` changes (two rules when it ran on both).
-- Its filters become condition leaves under one `and`, after
-- `ticket.assignee_uuid is_null`, since the old engine only routed a ticket
-- nobody was assigned. Its method becomes an `assign` step: a person directly,
-- or, for all three team methods, whoever on the team has the fewest open
-- tickets (decision D6). A `stop_processing` step after it keeps the first
-- rule that assigns the one that wins. The rules keep their order and run
-- after rules at the default priority (100).
--
-- An inactive rule is archived. So is one the old engine never ran: its person
-- or team was deleted, it had no trigger, or a filter matched no ticket (a
-- priority or status category that doesn't exist, title text longer than a
-- condition takes). Its description says why.
--
-- The assignment log becomes succeeded applications of the rule it ran as,
-- credited to the system. A log row whose rule was deleted has no rule left to
-- belong to and isn't copied. `rules` has no audit trigger, and
-- `rules_version_on_insert` writes each rule's version 1.
--
-- Two records the engine reads instead of inferring them:
-- `tickets.rules_held_at` marks a request whose creation rules wait for its
-- approval (it was made pending in a workspace that holds waiting requests):
-- its `ticket.created` never runs them. `tickets.rules_released_at` is set
-- once, by the approval that lets it go ahead, and that decision runs them;
-- `ticket_rule_runs.stopped` marks a budget slot whose run stopped the rules
-- after it, so a retried event stops there only then.

CREATE TEMPORARY TABLE assignment_rule_moves ON COMMIT DROP AS
WITH legacy AS (
    SELECT r.*,
           CASE WHEN jsonb_typeof(r.conditions) = 'object' THEN r.conditions
                ELSE '{}'::jsonb END AS c,
           row_number() OVER (PARTITION BY r.workspace_id ORDER BY r.priority, r.id) AS rank
      FROM public.assignment_rules r
),
parsed AS (
    SELECT l.*,
           -- The priority as the old parser read it: case and spaces ignored,
           -- "normal" for medium. Not a string means no priority filter.
           CASE WHEN jsonb_typeof(l.c -> 'priority') = 'string' THEN
               CASE lower(btrim(l.c ->> 'priority'))
                   WHEN 'normal' THEN 'medium'
                   ELSE lower(btrim(l.c ->> 'priority'))
               END
           END AS priority_name,
           coalesce(jsonb_typeof(l.c -> 'categories') = 'array', false) AS has_categories,
           (SELECT jsonb_agg(DISTINCT v.value)
              FROM jsonb_array_elements(
                       CASE WHEN jsonb_typeof(l.c -> 'categories') = 'array'
                            THEN l.c -> 'categories' ELSE '[]'::jsonb END) v
             WHERE v.value #>> '{}' IN ('triage', 'backlog', 'active', 'in_review',
                                        'done', 'cancelled', 'merged')
               AND jsonb_typeof(v.value) = 'string') AS categories,
           CASE WHEN jsonb_typeof(l.c -> 'title_contains') = 'string'
                 AND l.c ->> 'title_contains' <> ''
                THEN l.c ->> 'title_contains' END AS title_text
      FROM legacy l
),
judged AS (
    SELECT r.*,
           r.priority_name IS NOT NULL
               AND r.priority_name NOT IN ('none', 'low', 'medium', 'high', 'urgent')
               AS priority_bad,
           r.has_categories AND r.categories IS NULL AS categories_bad,
           coalesce(char_length(r.title_text) > 1000, false) AS title_bad,
           CASE WHEN r.method = 'direct_user' THEN r.target_user_uuid IS NULL
                ELSE r.target_group_id IS NULL END AS target_gone,
           NOT r.trigger_on_create AND NOT r.trigger_on_category_change AS no_trigger
      FROM parsed r
),
shaped AS (
    SELECT j.*,
           jsonb_build_array(jsonb_build_object(
               'kind', 'leaf', 'field', 'ticket.assignee_uuid', 'op', 'is_null'))
           || CASE WHEN j.category_id IS NOT NULL THEN jsonb_build_array(jsonb_build_object(
                  'kind', 'leaf', 'field', 'ticket.category_id', 'op', 'eq',
                  'value', j.category_id)) ELSE '[]'::jsonb END
           || CASE WHEN j.priority_name IS NOT NULL AND NOT j.priority_bad
                   THEN jsonb_build_array(jsonb_build_object(
                  'kind', 'leaf', 'field', 'ticket.priority', 'op', 'eq',
                  'value', j.priority_name)) ELSE '[]'::jsonb END
           || CASE WHEN j.categories IS NOT NULL THEN jsonb_build_array(jsonb_build_object(
                  'kind', 'leaf', 'field', 'ticket.workflow_state.category', 'op', 'in',
                  'value', j.categories)) ELSE '[]'::jsonb END
           || CASE WHEN j.title_text IS NOT NULL AND NOT j.title_bad
                   THEN jsonb_build_array(jsonb_build_object(
                  'kind', 'leaf', 'field', 'ticket.title', 'op', 'contains',
                  'value', j.title_text)) ELSE '[]'::jsonb END
               AS leaves,
           CASE j.method
               WHEN 'direct_user' THEN
                   'Moved from Assignment Rules in Nosdesk 1.2. It assigns one person.'
               WHEN 'group_round_robin' THEN
                   'Moved from Assignment Rules in Nosdesk 1.2, where it took turns around '
                   'the team (round robin). It now assigns whoever on the team has the '
                   'fewest open tickets.'
               WHEN 'group_random' THEN
                   'Moved from Assignment Rules in Nosdesk 1.2, where it picked someone on '
                   'the team at random. It now assigns whoever on the team has the fewest '
                   'open tickets.'
               ELSE
                   'Moved from Assignment Rules in Nosdesk 1.2, where it left the ticket '
                   'in the team''s queue for someone to pick up. It now assigns whoever on '
                   'the team has the fewest open tickets.'
           END AS method_note,
           concat_ws(' ',
               CASE WHEN j.target_gone AND j.method = 'direct_user' THEN
                   'Archived because the person it assigned no longer exists.'
                    WHEN j.target_gone THEN
                   'Archived because the team it assigned to no longer exists.' END,
               CASE WHEN j.no_trigger THEN
                   'Archived because it ran neither when a ticket was created nor when '
                   'its category changed.' END,
               CASE WHEN j.priority_bad THEN format(
                   'Archived because its priority filter (%s) names no priority, so it '
                   'never matched.', j.c ->> 'priority') END,
               CASE WHEN j.categories_bad THEN
                   'Archived because its status filter names no status category, so it '
                   'never matched.' END,
               CASE WHEN j.title_bad THEN
                   'Archived because its title text is longer than the 1000 characters '
                   'a condition takes.' END
           ) AS archive_note,
           j.is_active
               AND NOT (j.priority_bad OR j.categories_bad OR j.title_bad
                        OR j.target_gone OR j.no_trigger) AS live
      FROM judged j
)
-- Ids are drawn in the old evaluation order, so they keep it too.
SELECT nextval('public.rules_id_seq')::integer AS rule_id, ordered.*
  FROM (
SELECT s.id AS legacy_id,
       s.workspace_id,
       t.trigger_kind,
       t.trigger_config,
       t.name,
       concat_ws(E'\n\n', nullif(btrim(s.description), ''), s.method_note,
                 nullif(s.archive_note, '')) AS description,
       jsonb_build_object('kind', 'and', 'children', s.leaves) AS conditions,
       jsonb_build_array(
           CASE WHEN s.method = 'direct_user' THEN jsonb_build_object(
                    'kind', 'assign',
                    'config', jsonb_strip_nulls(jsonb_build_object(
                        'method', 'direct', 'user_uuid', s.target_user_uuid)))
                ELSE jsonb_build_object(
                    'kind', 'assign',
                    'config', jsonb_strip_nulls(jsonb_build_object(
                        'method', 'group', 'group_id', s.target_group_id)))
           END,
           jsonb_build_object('kind', 'stop_processing')) AS actions,
       ARRAY(SELECT DISTINCT f.field COLLATE "C"
               FROM jsonb_array_elements(s.leaves) leaf,
                    LATERAL unnest(CASE leaf ->> 'field'
                        WHEN 'ticket.workflow_state.category'
                            THEN ARRAY['ticket.workflow_state_id', 'ticket.workflow_state.category']
                        ELSE ARRAY[leaf ->> 'field'] END) AS f(field)
              ORDER BY 1) AS reads_set,
       CASE WHEN s.live THEN 'live' ELSE 'archived' END::public.rule_state AS state,
       (1000 + 10 * s.rank)::integer AS priority,
       s.created_by,
       s.created_at,
       s.updated_at,
       CASE WHEN s.live THEN NULL ELSE now() END AS archived_at
  FROM shaped s
 CROSS JOIN LATERAL (
       -- A rule that ran on no trigger is kept as a created rule.
       SELECT 'ticket_created'::public.rule_trigger_kind AS trigger_kind,
              '{}'::jsonb AS trigger_config,
              s.name::text AS name
        WHERE s.trigger_on_create OR NOT s.trigger_on_category_change
       UNION ALL
       SELECT 'ticket_updated'::public.rule_trigger_kind,
              '{"changed_fields_include": ["ticket.category_id"]}'::jsonb,
              CASE WHEN s.trigger_on_create
                   THEN left(s.name, 255 - char_length(' (category change)'))
                        || ' (category change)'
                   ELSE s.name END
        WHERE s.trigger_on_category_change
 ) t
 ORDER BY s.workspace_id, s.rank, t.trigger_kind
) ordered;

INSERT INTO public.rules (
    id, workspace_id, name, description, trigger_kind, trigger_config, conditions,
    actions, reads_set, writes_set, state, priority, created_by, created_at,
    updated_at, archived_at
)
SELECT rule_id, workspace_id, name, description, trigger_kind, trigger_config, conditions,
       actions, reads_set, ARRAY['ticket.assignee_uuid', 'ticket.updated_at'], state,
       priority, created_by, created_at, updated_at, archived_at
  FROM assignment_rule_moves
 ORDER BY rule_id;

-- Each log row goes to the rule of the trigger it ran on: a category change's
-- to the category change rule, any other to the created rule, or to the one
-- rule there is.
INSERT INTO public.rule_applications (
    workspace_id, rule_id, rule_version, ticket_id, status, actor_kind,
    originating_event_kind, condition_evaluation, actions_taken, applied_at
)
SELECT DISTINCT ON (l.id)
       l.workspace_id,
       m.rule_id,
       1,
       l.ticket_id,
       'succeeded',
       'system',
       CASE l.trigger_type
           WHEN 'ticket_created' THEN 'ticket.created'
           WHEN 'category_changed' THEN 'ticket.category_changed'
       END,
       jsonb_build_object(
           'matched', true,
           'trigger', m.trigger_kind::text,
           'migrated_from', 'assignment_log',
           'method', l.method::text),
       CASE WHEN l.new_assignee_uuid IS NOT NULL
            THEN jsonb_build_array(jsonb_build_object(
                'index', 1, 'kind', 'assign', 'assigned_to_uuid', l.new_assignee_uuid))
            ELSE '[]'::jsonb END,
       l.assigned_at
  FROM public.assignment_log l
  JOIN assignment_rule_moves m ON m.legacy_id = l.rule_id
 ORDER BY l.id,
          m.trigger_kind = CASE WHEN l.trigger_type = 'category_changed'
                                THEN 'ticket_updated'::public.rule_trigger_kind
                                ELSE 'ticket_created'::public.rule_trigger_kind END DESC;

-- How often each rule fired, as the engine counts it. Neither column is part
-- of a version, so this saves none.
UPDATE public.rules r
   SET fire_count = a.fired,
       last_fired_at = a.last_fired_at
  FROM (SELECT ra.rule_id, count(*)::integer AS fired, max(ra.applied_at) AS last_fired_at
          FROM public.rule_applications ra
          JOIN assignment_rule_moves m ON m.rule_id = ra.rule_id
         GROUP BY ra.rule_id) a
 WHERE r.id = a.rule_id;

DROP TABLE public.assignment_log;
DROP TABLE public.assignment_rule_state;
DROP TABLE public.assignment_rules;
DROP TYPE public.assignment_method;

ALTER TABLE public.tickets
    ADD COLUMN rules_held_at timestamptz,
    ADD COLUMN rules_released_at timestamptz;

-- Requests held for approval now: the old engine skipped their routing until
-- approval, and the event rules turned their creation away. Requests that
-- already went ahead have nothing waiting. Neither is an edit to the ticket.
ALTER TABLE public.tickets DISABLE TRIGGER tr_audit_tickets;
ALTER TABLE public.tickets DISABLE TRIGGER set_updated_at;
UPDATE public.tickets t
   SET rules_held_at = now()
  FROM public.site_settings s
 WHERE s.workspace_id = t.workspace_id
   AND s.approval_waiting_display = 'held'
   AND t.approval_state = 'pending';
ALTER TABLE public.tickets ENABLE TRIGGER set_updated_at;
ALTER TABLE public.tickets ENABLE TRIGGER tr_audit_tickets;

ALTER TABLE public.ticket_rule_runs ADD COLUMN stopped boolean NOT NULL DEFAULT false;
