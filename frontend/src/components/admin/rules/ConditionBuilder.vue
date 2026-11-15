<script setup lang="ts">
/**
 * Builds a rule's conditions: groups of "all", "any" or "none of these",
 * each holding conditions on a ticket field (or, for an updated or replied
 * trigger, on the event) and further groups. The tree is the editor's
 * (see conditionTree.ts); the rule editor turns it into the server's shape.
 */
import { computed, provide, reactive } from 'vue'
import { useFluent } from 'fluent-vue'

import ConditionGroupEditor from './ConditionGroupEditor.vue'
import { CONDITION_BUILDER } from './conditionContext'
import { CHANGEABLE_FIELDS, fieldDef, type ConditionGroup, type ConditionNode } from './conditionTree'
import { useWorkspaceQuery } from '@/queries/workspaceQuery'
import userService from '@/services/userService'
import { categoryService } from '@nosdesk/core/services/categoryService'
import { useTagsStore } from '@nosdesk/core/stores/tags'
import { useWorkflowStatesStore } from '@nosdesk/core/stores/workflowStates'
import type { RuleTriggerKind } from '@nosdesk/core/types/rule'

const props = withDefaults(
  defineProps<{
    modelValue: ConditionGroup
    triggerKind: RuleTriggerKind
    disabled?: boolean
    /** Show what each unfinished condition still needs. */
    showProblems?: boolean
  }>(),
  { disabled: false, showProblems: false },
)

const emit = defineEmits<{
  (e: 'update:modelValue', value: ConditionGroup): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const states = useWorkflowStatesStore()
const tags = useTagsStore()
void states.load()

const categoriesQuery = useWorkspaceQuery({
  key: ['rules', 'condition-categories'],
  query: () => categoryService.getAllCategoriesAdmin(),
  staleTime: 5 * 60 * 1000,
})

/** People the conditions name, so each shows by name. */
const namedUuids = computed(() => {
  const uuids = new Set<string>()
  const walk = (node: ConditionNode) => {
    if (node.kind === 'group') {
      node.children.forEach(walk)
      return
    }
    if (node.field !== 'ticket.assignee_uuid' && node.field !== 'ticket.requester_uuid') return
    const values = Array.isArray(node.value) ? node.value : [node.value]
    for (const v of values) if (typeof v === 'string' && v) uuids.add(v)
  }
  walk(props.modelValue)
  return [...uuids].sort()
})
const usersQuery = useWorkspaceQuery({
  key: () => ['users', 'batch', ...namedUuids.value],
  query: () => userService.getUsersBatch(namedUuids.value),
  enabled: () => namedUuids.value.length > 0,
  staleTime: 5 * 60 * 1000,
})
/** Names of people picked in this session, before the batch lookup has them. */
const picked = reactive(new Map<string, string>())

/** A field's name in words; one this editor doesn't know shows as stored. */
function fieldLabel(name: string): string {
  const known = fieldDef(name) != null || CHANGEABLE_FIELDS.includes(name)
  return known ? t(`admin-rule-conditions-field-${name.replace(/[._]/g, '-')}`) : name
}

provide(CONDITION_BUILDER, {
  trigger: () => props.triggerKind,
  root: () => props.modelValue,
  edit: (next) => emit('update:modelValue', next),
  disabled: () => props.disabled,
  showProblems: () => props.showProblems,
  t,
  fieldLabel,
  statusOptions: computed(() =>
    states.states.filter((s) => !s.archived_at).map((s) => ({ value: s.id, label: s.name })),
  ),
  categoryOptions: computed(() => (categoriesQuery.data.value ?? []).map((c) => ({ value: c.id, label: c.name }))),
  tagOptions: computed(() => tags.tags.map((tag) => ({ value: tag.id, label: tag.name }))),
  changedFieldOptions: computed(() => CHANGEABLE_FIELDS.map((name) => ({ value: name, label: fieldLabel(name) }))),
  userName: (uuid) =>
    picked.get(uuid) ?? (usersQuery.data.value ?? []).find((u) => u.uuid === uuid)?.name ?? null,
  rememberUser: (uuid, name) => {
    if (uuid && name) picked.set(uuid, name)
  },
})
</script>

<template>
  <ConditionGroupEditor :group="modelValue" :path="[]" />
</template>
