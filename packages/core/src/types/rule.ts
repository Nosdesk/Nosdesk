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
  | 'time_elapsed';

export type RuleState = 'draft' | 'dry_run' | 'live' | 'archived';

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
    | 'notify'
    | 'apply_macro_template'
    | 'webhook'
    | 'stop_processing';
  config?: Record<string, unknown>;
}

/** Recursive AND/OR/NOT/leaf condition node. */
export type RuleCondition =
  | { kind: 'and'; children: RuleCondition[] }
  | { kind: 'or'; children: RuleCondition[] }
  | { kind: 'not'; child: RuleCondition }
  | {
      kind: 'leaf';
      field: string;
      op: string;
      value: unknown;
    };

/** Wire shape of a Rule. Manual rules have `conditions: []`. */
export interface Rule {
  id: number;
  workspace_id: number;
  name: string;
  description: string | null;
  trigger_kind: RuleTriggerKind;
  trigger_config: Record<string, unknown>;
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
}

export interface CreateRuleRequest {
  name: string;
  description?: string | null;
  trigger_kind: RuleTriggerKind;
  trigger_config?: Record<string, unknown>;
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
  trigger_config?: Record<string, unknown>;
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
  trigger_config: Record<string, unknown>;
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
