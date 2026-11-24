/**
 * Rules engine types (docs/rules-and-actions-plan.md). The agent-
 * facing surface calls these "Actions" (the toolbar button label
 * locked by decision 26); the entity name stays `Rule` in code +
 * the admin Settings page is `/admin/rules`.
 */

export type RuleTriggerKind =
  | 'manual'
  | 'ticket_created'
  | 'ticket_updated'
  | 'ticket_replied'
  | 'time_elapsed'
  | 'plugin_event';

/**
 * `paused` is a rule an admin stopped. `dry_run` shadow-runs a rule with an
 * automated trigger; a manual rule never takes it.
 */
export type RuleState = 'draft' | 'dry_run' | 'live' | 'paused' | 'archived';

export type RuleApplicationStatus =
  | 'succeeded'
  | 'dry_run'
  | 'skipped_preflight'
  | 'skipped_condition_unmet'
  | 'suppressed_recursion_budget'
  | 'suppressed_loop_guard'
  | 'failed';

/** Typed action object stored in `Rule.actions`. */
export interface RuleAction {
  kind:
    | 'reply'
    | 'set_status'
    | 'assign'
    | 'unassign'
    | 'add_tags'
    | 'remove_tags'
    | 'set_priority'
    | 'assign_assets_to_requester'
    | 'lend_assets'
    | 'return_requester_assets'
    | 'notify'
    | 'apply_macro_template'
    | 'webhook'
    | 'stop_processing';
  config?: Record<string, unknown>;
}

/** A condition leaf's comparison (`ConditionOp` on the server). */
export type RuleConditionOp =
  | 'eq'
  | 'neq'
  | 'in'
  | 'not_in'
  | 'contains'
  | 'not_contains'
  | 'starts_with'
  | 'ends_with'
  | 'gt'
  | 'lt'
  | 'gte'
  | 'lte'
  | 'matches_regex'
  | 'is_null'
  | 'is_not_null'
  | 'has_any'
  | 'has_all';

/** One condition leaf: `field` is a `ticket.*`, `event.*`, `reply.*` or
 *  `clock.*` name; `value` is null for `is_null` / `is_not_null`. */
export interface RuleConditionLeaf {
  kind: 'leaf';
  field: string;
  op: RuleConditionOp;
  value: unknown;
}

/** Recursive AND/OR/NOT/leaf condition node. */
export type RuleCondition =
  | { kind: 'and'; children: RuleCondition[] }
  | { kind: 'or'; children: RuleCondition[] }
  | { kind: 'not'; child: RuleCondition }
  | RuleConditionLeaf;

/** Whose public reply fires a `ticket_replied` rule. */
export type RuleReplyKind = 'customer' | 'agent' | 'any';

/**
 * `trigger_config` by trigger. A `ticket_updated` rule runs only when the
 * update changed one of `changed_fields_include` (`ticket.*` names, as in
 * `reads_set`), or on any change when the list is absent. A `ticket_replied`
 * rule runs on replies of `reply_kind`. A `plugin_event` rule runs on the
 * plugin `plugin_uuid`'s `event_type` events (one of its manifest `events`),
 * on the ticket the event names. A `time_elapsed` rule runs `minutes` after
 * its `clock_name` clock started (for `until_due_at`, `minutes` before the
 * ticket is due), and again every `repeat_every_minutes` while the clock
 * runs, when set. Other triggers take `{}`.
 */
export interface RuleTriggerConfig {
  changed_fields_include?: string[];
  reply_kind?: RuleReplyKind;
  plugin_uuid?: string;
  event_type?: string;
  clock_name?: string;
  minutes?: number;
  repeat_every_minutes?: number | null;
}

/** Wire shape of a Rule. Manual rules have `conditions: []`. */
export interface Rule {
  id: number;
  workspace_id: number;
  name: string;
  description: string | null;
  trigger_kind: RuleTriggerKind;
  trigger_config: RuleTriggerConfig;
  /** Empty array (`[]`) for manual rules; single condition node otherwise. */
  conditions: RuleCondition[] | RuleCondition;
  actions: RuleAction[];
  reads_set: string[];
  writes_set: string[];
  state: RuleState;
  priority: number;
  last_fired_at: string | null;
  fire_count: number;
  created_by: string | null;
  created_at: string;
  updated_at: string;
  archived_at: string | null;
  /** Events turned away before any rule ran (the ticket was held for
   *  approval, or merged). They leave no application row. */
  skipped_preflight_count?: number;
  /** Events a live rule skipped because the ticket didn't meet its
   *  conditions. They leave no application row; a dry run logs them. */
  skipped_condition_unmet_count?: number;
}

/** A ready-made rule from the starter catalogue (`GET /api/admin/rule-starters`),
 *  named and described in the request's language. */
export interface StarterRule {
  id: string;
  name: string;
  description: string;
  trigger_kind: RuleTriggerKind;
  /** As a rule of its trigger takes it: a time rule's clock and wait. */
  trigger_config: RuleTriggerConfig & {
    clock_name?: string;
    minutes?: number;
    repeat_every_minutes?: number | null;
  };
  conditions: RuleCondition[] | RuleCondition;
  actions: RuleAction[];
}

/** How many of the workspace's recent tickets a rule's conditions match
 *  (`POST /api/rules/preview`, and on every save). */
export interface RulePreview {
  matched: number;
  scanned: number;
  /** Up to five matching tickets, newest first. */
  sample_ticket_ids: number[];
}

/** A rule as create and update return it, with its preview. */
export type SavedRule = Rule & { preview: RulePreview };

export interface PreviewRuleRequest {
  trigger_kind: RuleTriggerKind;
  conditions: RuleCondition[] | RuleCondition;
}

export interface CreateRuleRequest {
  name: string;
  description?: string | null;
  trigger_kind: RuleTriggerKind;
  trigger_config?: RuleTriggerConfig;
  conditions?: RuleCondition[] | RuleCondition;
  actions: RuleAction[];
  priority?: number;
  /** Skip the self-referential save linter (writes ∩ reads != ∅). */
  override_self_reference?: boolean;
}

export interface UpdateRuleRequest {
  name?: string;
  description?: string | null;
  trigger_kind?: RuleTriggerKind;
  trigger_config?: RuleTriggerConfig;
  conditions?: RuleCondition[] | RuleCondition;
  actions?: RuleAction[];
  priority?: number;
  override_self_reference?: boolean;
}

export interface StateTransitionRequest {
  state: RuleState;
}

export interface ListRulesQuery {
  trigger_kind?: RuleTriggerKind;
  state?: RuleState;
  q?: string;
  include_archived?: boolean;
}

/** Rule version snapshot (one row per save). */
export interface RuleVersion {
  id: number;
  rule_id: number;
  workspace_id: number;
  version: number;
  name: string;
  description: string | null;
  trigger_kind: RuleTriggerKind;
  trigger_config: RuleTriggerConfig;
  conditions: RuleCondition[] | RuleCondition;
  actions: RuleAction[];
  state: RuleState;
  priority: number;
  saved_by: string | null;
  saved_at: string;
}

/** One row per fire attempt (manual apply, event match, time match, dry-run). */
export interface RuleApplication {
  id: number;
  workspace_id: number;
  rule_id: number;
  rule_version: number;
  ticket_id: number;
  status: RuleApplicationStatus;
  correlation_id: string | null;
  actor_uuid: string | null;
  actor_kind: 'user' | 'system';
  originating_event_id: string | null;
  originating_event_kind: string | null;
  condition_evaluation: Record<string, unknown> | null;
  actions_taken: Array<Record<string, unknown>> | null;
  actions_skipped: Array<Record<string, unknown>> | null;
  failure_reason: string | null;
  applied_at: string;
}

export interface ListApplicationsQuery {
  rule_id?: number;
  ticket_id?: number;
  status?: RuleApplicationStatus;
  actor_uuid?: string;
  from?: string;
  to?: string;
  limit?: number;
}

export interface ApplyRuleRequest {
  ticket_id: number;
  overrides?: {
    body?: string;
    /** 1-indexed action positions to skip. */
    suppress_actions?: number[];
  };
}

export interface ApplyRuleResponse {
  rule: Rule;
  application_id: number;
  correlation_id: string | null;
  comment_id: number | null;
  actions_executed: number;
  actions_suppressed: number;
}

/** A rule the conflicts inspector checked: live or dry run, not manual. */
export interface RuleConflictRule {
  id: number;
  name: string;
  trigger_kind: RuleTriggerKind;
  state: RuleState;
  priority: number;
}

/** The writer's steps change `fields`, which the reader's conditions check. */
export interface RuleOverlap {
  writer_rule_id: number;
  reader_rule_id: number;
  /** `ticket.*` keys, as in `reads_set` / `writes_set`. */
  fields: string[];
}

/** Two checked rules that both ran on `ticket_count` tickets since `since`. */
export interface RuleCoFire {
  first_rule_id: number;
  second_rule_id: number;
  ticket_count: number;
  last_co_fired_at: string;
  /** Up to five, most recent first. */
  sample_ticket_ids: number[];
}

export interface RuleConflicts {
  since: string;
  rules: RuleConflictRule[];
  overlaps: RuleOverlap[];
  co_fires: RuleCoFire[];
}
