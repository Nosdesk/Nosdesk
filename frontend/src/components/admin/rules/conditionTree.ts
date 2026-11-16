/**
 * The rule condition tree as the editor builds it, and the fields and
 * operators it offers. The field list mirrors the server's
 * `ConditionField` (models/rules.rs): each field's value kind decides its
 * operators, a field that is never empty takes no `is_null`/`is_not_null`,
 * and event fields apply only to the trigger that carries them. Fields the
 * server can't read yet are left out.
 *
 * The editor works on groups: `all` and `any` are the server's `and` and
 * `or`, and `none` is `not` around an `or`. A `not` around an `and` (only
 * from the API) shows as `not_all`. `toWire` turns the editor's tree back
 * into the server's shape, with `[]` for no conditions.
 */
import type {
  RuleCondition,
  RuleConditionLeaf,
  RuleConditionOp,
  RuleTriggerKind,
} from '@nosdesk/core/types/rule'

export type FieldKind =
  | 'string'
  | 'enum'
  | 'int_id'
  | 'uuid'
  | 'int_array'
  | 'string_array'
  | 'timestamp'
  | 'number'

export interface ConditionFieldDef {
  name: string
  kind: FieldKind
  /** Whether the field can be empty, so `is_null` means something. */
  nullable: boolean
  /** The only trigger whose event carries this field. */
  triggerOnly?: RuleTriggerKind
  /** An enum field's values. */
  values?: string[]
}

export const CONDITION_FIELDS: ConditionFieldDef[] = [
  { name: 'ticket.title', kind: 'string', nullable: false },
  { name: 'ticket.workflow_state_id', kind: 'int_id', nullable: false },
  {
    name: 'ticket.workflow_state.category',
    kind: 'enum',
    nullable: false,
    values: ['triage', 'backlog', 'active', 'in_review', 'done', 'cancelled', 'merged'],
  },
  { name: 'ticket.priority', kind: 'enum', nullable: false, values: ['none', 'low', 'medium', 'high', 'urgent'] },
  { name: 'ticket.category_id', kind: 'int_id', nullable: true },
  { name: 'ticket.tag_ids', kind: 'int_array', nullable: true },
  { name: 'ticket.assignee_uuid', kind: 'uuid', nullable: true },
  { name: 'ticket.requester_uuid', kind: 'uuid', nullable: true },
  { name: 'ticket.requester.email', kind: 'string', nullable: true },
  { name: 'ticket.requester.organization', kind: 'string', nullable: true },
  { name: 'ticket.created_at', kind: 'timestamp', nullable: false },
  { name: 'ticket.updated_at', kind: 'timestamp', nullable: false },
  { name: 'ticket.first_response_at', kind: 'timestamp', nullable: true },
  { name: 'ticket.resolved_at', kind: 'timestamp', nullable: true },
  { name: 'event.changed_fields', kind: 'string_array', nullable: true, triggerOnly: 'ticket_updated' },
  { name: 'reply.kind', kind: 'enum', nullable: false, triggerOnly: 'ticket_replied', values: ['customer', 'agent'] },
  { name: 'reply.author_role', kind: 'string', nullable: true, triggerOnly: 'ticket_replied' },
  { name: 'clock.minutes_since', kind: 'number', nullable: false, triggerOnly: 'time_elapsed' },
]

/** The operators each kind of field takes (`allowed_ops` on the server). */
export const OPS_BY_KIND: Record<FieldKind, RuleConditionOp[]> = {
  string: [
    'eq',
    'neq',
    'contains',
    'not_contains',
    'starts_with',
    'ends_with',
    'in',
    'not_in',
    'matches_regex',
    'is_null',
    'is_not_null',
  ],
  enum: ['eq', 'neq', 'in', 'not_in', 'is_null', 'is_not_null'],
  int_id: ['eq', 'neq', 'in', 'not_in', 'is_null', 'is_not_null'],
  uuid: ['eq', 'neq', 'in', 'not_in', 'is_null', 'is_not_null'],
  int_array: ['has_any', 'has_all', 'is_null', 'is_not_null'],
  string_array: ['has_any', 'has_all', 'is_null', 'is_not_null'],
  timestamp: ['gt', 'lt', 'gte', 'lte', 'is_null', 'is_not_null'],
  number: ['gt', 'lt', 'gte', 'lte'],
}

/**
 * The `ticket.*` names a ticket update lists in `changed_fields`, for the
 * changed-field filter on an updated trigger and the `event.changed_fields`
 * condition.
 */
export const CHANGEABLE_FIELDS = [
  'ticket.workflow_state_id',
  'ticket.workflow_state.category',
  'ticket.priority',
  'ticket.assignee_uuid',
  'ticket.requester_uuid',
  'ticket.category_id',
  'ticket.tag_ids',
  'ticket.title',
  'ticket.due_date',
]

export function fieldDef(name: string): ConditionFieldDef | undefined {
  return CONDITION_FIELDS.find((f) => f.name === name)
}

/** The fields a rule with this trigger can check. */
export function fieldsFor(trigger: RuleTriggerKind): ConditionFieldDef[] {
  return CONDITION_FIELDS.filter((f) => !f.triggerOnly || f.triggerOnly === trigger)
}

/** The operators a field takes: its kind's, less the empty checks on a field
 *  that is never empty. */
export function opsFor(field: string): RuleConditionOp[] {
  const def = fieldDef(field)
  if (!def) return []
  const ops = OPS_BY_KIND[def.kind]
  return def.nullable ? ops : ops.filter((op) => op !== 'is_null' && op !== 'is_not_null')
}

const NO_VALUE: RuleConditionOp[] = ['is_null', 'is_not_null']
const LIST_VALUE: RuleConditionOp[] = ['in', 'not_in', 'has_any', 'has_all']

/** The shape of value an operator takes. */
export function valueShape(op: RuleConditionOp): 'none' | 'list' | 'single' {
  if (NO_VALUE.includes(op)) return 'none'
  if (LIST_VALUE.includes(op)) return 'list'
  return 'single'
}

export type GroupMode = 'all' | 'any' | 'none' | 'not_all'

export interface ConditionGroup {
  kind: 'group'
  mode: GroupMode
  children: ConditionNode[]
}

export type ConditionNode = ConditionGroup | RuleConditionLeaf

/** Where a node sits: child indexes from the root group down. */
export type NodePath = number[]

export function emptyGroup(mode: GroupMode = 'all'): ConditionGroup {
  return { kind: 'group', mode, children: [] }
}

/** A new leaf on the field, with the first operator it takes. */
export function newLeaf(field = 'ticket.priority'): RuleConditionLeaf {
  const op = opsFor(field)[0] ?? 'eq'
  return { kind: 'leaf', field, op, value: defaultValue(field, op) }
}

/** The value a leaf starts with, before the admin picks one. */
export function defaultValue(field: string, op: RuleConditionOp): unknown {
  const shape = valueShape(op)
  if (shape === 'none') return null
  if (shape === 'list') return []
  const kind = fieldDef(field)?.kind
  if (kind === 'int_id' || kind === 'number') return null
  return ''
}

// ---------------------------------------------------------------------------
// Wire shape
// ---------------------------------------------------------------------------

function nodeFromWire(node: RuleCondition): ConditionNode {
  if (node.kind === 'leaf') {
    return { kind: 'leaf', field: node.field, op: node.op, value: node.value ?? null }
  }
  return groupFromWire(node)
}

function groupFromWire(node: RuleCondition): ConditionGroup {
  switch (node.kind) {
    case 'and':
      return { kind: 'group', mode: 'all', children: node.children.map(nodeFromWire) }
    case 'or':
      return { kind: 'group', mode: 'any', children: node.children.map(nodeFromWire) }
    case 'not': {
      const inner = node.child
      if (inner.kind === 'and') return { kind: 'group', mode: 'not_all', children: inner.children.map(nodeFromWire) }
      if (inner.kind === 'or') return { kind: 'group', mode: 'none', children: inner.children.map(nodeFromWire) }
      // not(x) is none of [x].
      return { kind: 'group', mode: 'none', children: [nodeFromWire(inner)] }
    }
    default:
      return { kind: 'group', mode: 'all', children: [nodeFromWire(node)] }
  }
}

/** A rule's stored conditions as the editor's root group. */
export function fromWire(conditions: RuleCondition[] | RuleCondition | null | undefined): ConditionGroup {
  if (conditions == null) return emptyGroup()
  if (Array.isArray(conditions)) {
    return { kind: 'group', mode: 'all', children: conditions.map(nodeFromWire) }
  }
  return groupFromWire(conditions)
}

function nodeToWire(node: ConditionNode): RuleCondition {
  if (node.kind === 'leaf') {
    return { kind: 'leaf', field: node.field, op: node.op, value: valueShape(node.op) === 'none' ? null : node.value }
  }
  const children = node.children.map(nodeToWire)
  switch (node.mode) {
    case 'any':
      return { kind: 'or', children }
    case 'none':
      return { kind: 'not', child: { kind: 'or', children } }
    case 'not_all':
      return { kind: 'not', child: { kind: 'and', children } }
    default:
      return { kind: 'and', children }
  }
}

/** The editor's root group in the server's shape: `[]` when it's empty. */
export function toWire(root: ConditionGroup): RuleCondition[] | RuleCondition {
  return root.children.length === 0 ? [] : nodeToWire(root)
}

// ---------------------------------------------------------------------------
// Edits. Each returns a new root and leaves the old one as it was.
// ---------------------------------------------------------------------------

// The tree is plain JSON, and may arrive wrapped in Vue's reactive proxies,
// which structuredClone refuses.
function clone(root: ConditionGroup): ConditionGroup {
  return JSON.parse(JSON.stringify(root)) as ConditionGroup
}

function groupAt(root: ConditionGroup, path: NodePath): ConditionGroup {
  let node: ConditionNode = root
  for (const i of path) {
    if (node.kind !== 'group') throw new Error('condition path runs through a leaf')
    node = node.children[i]
  }
  if (node?.kind !== 'group') throw new Error('condition path does not end at a group')
  return node
}

export function addChild(root: ConditionGroup, groupPath: NodePath, child: ConditionNode): ConditionGroup {
  const next = clone(root)
  groupAt(next, groupPath).children.push(child)
  return next
}

export function removeNode(root: ConditionGroup, path: NodePath): ConditionGroup {
  if (path.length === 0) return emptyGroup(root.mode)
  const next = clone(root)
  groupAt(next, path.slice(0, -1)).children.splice(path[path.length - 1], 1)
  return next
}

export function setMode(root: ConditionGroup, groupPath: NodePath, mode: GroupMode): ConditionGroup {
  const next = clone(root)
  groupAt(next, groupPath).mode = mode
  return next
}

/**
 * Change a leaf. A new field keeps the operator when the field takes it,
 * else starts on its first; a new operator keeps the value when its shape
 * still fits, else starts empty.
 */
export function updateLeaf(
  root: ConditionGroup,
  path: NodePath,
  change: { field?: string; op?: RuleConditionOp; value?: unknown },
): ConditionGroup {
  const next = clone(root)
  const parent = groupAt(next, path.slice(0, -1))
  const index = path[path.length - 1]
  const leaf = parent.children[index]
  if (leaf?.kind !== 'leaf') throw new Error('condition path does not end at a leaf')
  let { field, op, value } = leaf
  if (change.field !== undefined && change.field !== field) {
    const sameKind = fieldDef(change.field)?.kind === fieldDef(field)?.kind
    field = change.field
    const ops = opsFor(field)
    if (!ops.includes(op)) op = ops[0] ?? op
    value = sameKind && valueFits(field, op, value) ? value : defaultValue(field, op)
  }
  if (change.op !== undefined && change.op !== op) {
    op = change.op
    if (!valueFits(field, op, value)) value = defaultValue(field, op)
  }
  if ('value' in change) value = change.value
  parent.children[index] = { kind: 'leaf', field, op, value }
  return next
}

function valueFits(field: string, op: RuleConditionOp, value: unknown): boolean {
  const shape = valueShape(op)
  if (shape === 'none') return value === null
  if (shape === 'list') return Array.isArray(value)
  const kind = fieldDef(field)?.kind
  if (kind === 'int_id' || kind === 'number') return typeof value === 'number' || value === null
  return typeof value === 'string'
}

// ---------------------------------------------------------------------------
// What still needs doing before the conditions can be saved
// ---------------------------------------------------------------------------

/** What a leaf still needs, as a message key, or null when it's complete. */
export function leafProblem(leaf: RuleConditionLeaf, trigger: RuleTriggerKind): string | null {
  const def = fieldDef(leaf.field)
  if (!def) return 'admin-rule-conditions-problem-field'
  if (def.triggerOnly && def.triggerOnly !== trigger) return 'admin-rule-conditions-problem-trigger'
  if (!opsFor(leaf.field).includes(leaf.op)) return 'admin-rule-conditions-problem-op'
  const shape = valueShape(leaf.op)
  if (shape === 'none') return null
  const value = leaf.value
  if (shape === 'list') {
    return Array.isArray(value) && value.length > 0 ? null : 'admin-rule-conditions-problem-value'
  }
  switch (def.kind) {
    case 'int_id':
      return typeof value === 'number' && value > 0 ? null : 'admin-rule-conditions-problem-value'
    case 'number':
      return typeof value === 'number' && Number.isInteger(value) && value >= 0
        ? null
        : 'admin-rule-conditions-problem-value'
    case 'timestamp':
      return typeof value === 'string' && !Number.isNaN(Date.parse(value)) ? null : 'admin-rule-conditions-problem-value'
    default:
      return typeof value === 'string' && value.trim() !== '' ? null : 'admin-rule-conditions-problem-value'
  }
}

/** Whether every leaf is complete and no group below the root is empty. */
export function treeComplete(root: ConditionGroup, trigger: RuleTriggerKind): boolean {
  const walk = (node: ConditionNode, isRoot: boolean): boolean => {
    if (node.kind === 'leaf') return leafProblem(node, trigger) === null
    if (!isRoot && node.children.length === 0) return false
    return node.children.every((child) => walk(child, false))
  }
  return walk(root, true)
}

/** A list of text values typed as `a, b, c`. */
export function splitList(text: string): string[] {
  return text
    .split(',')
    .map((part) => part.trim())
    .filter((part) => part !== '')
}

/** An ISO timestamp as a `datetime-local` input's value, in local time. */
export function toLocalInput(value: unknown): string {
  if (typeof value !== 'string' || Number.isNaN(Date.parse(value))) return ''
  const d = new Date(value)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`
}

/** A `datetime-local` input's value as an ISO timestamp, or '' when blank. */
export function fromLocalInput(text: string): string {
  if (!text) return ''
  const ms = Date.parse(text)
  return Number.isNaN(ms) ? '' : new Date(ms).toISOString()
}
