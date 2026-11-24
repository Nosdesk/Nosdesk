/**
 * What the condition builder hands down to its groups and leaves: the
 * trigger, the edit entry point and the option lists the value pickers use.
 */
import type { ComputedRef, InjectionKey } from 'vue'
import type { RuleTriggerKind } from '@nosdesk/core/types/rule'
import type { ConditionGroup } from './conditionTree'

export interface ConditionOption<T = string | number> {
  value: T
  label: string
}

export interface ConditionBuilderContext {
  trigger: () => RuleTriggerKind
  /** A time-elapsed rule's clock; empty for other triggers. */
  clock: () => string
  root: () => ConditionGroup
  /** Replace the whole tree (every edit builds a new one). */
  edit: (next: ConditionGroup) => void
  disabled: () => boolean
  /** Show what each unfinished leaf still needs. */
  showProblems: () => boolean
  t: (key: string, args?: Record<string, string | number>) => string
  fieldLabel: (name: string) => string
  statusOptions: ComputedRef<ConditionOption<number>[]>
  categoryOptions: ComputedRef<ConditionOption<number>[]>
  tagOptions: ComputedRef<ConditionOption<number>[]>
  changedFieldOptions: ComputedRef<ConditionOption<string>[]>
  /** A person's name, once it's known. */
  userName: (uuid: string) => string | null
  rememberUser: (uuid: string, name: string) => void
}

export const CONDITION_BUILDER: InjectionKey<ConditionBuilderContext> = Symbol('condition-builder')

/** How deep groups nest below the root. The server takes trees 8 nodes
 *  deep, and a "none" group spends two of them (a `not` around an `or`). */
export const MAX_NESTED_GROUPS = 3
