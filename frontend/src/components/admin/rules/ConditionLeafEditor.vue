<script setup lang="ts">
/**
 * One condition: a field, how to compare it, and the value. The value
 * picker follows the field's kind and the operator: a status, category,
 * tag or person picker for ids, a list for "is any of", text, a date and
 * time, or nothing for "is empty".
 */
import { computed, inject } from 'vue'

import BaseDropdown from '@/components/common/BaseDropdown.vue'
import FormInput from '@/components/common/FormInput.vue'
import FormNumber from '@/components/common/FormNumber.vue'
import IconButton from '@/components/common/IconButton.vue'
import UserPicker from '@/components/ticketComponents/UserPicker.vue'
import type { RuleConditionLeaf, RuleConditionOp } from '@nosdesk/core/types/rule'
import { CONDITION_BUILDER } from './conditionContext'
import {
  fieldDef,
  fieldsFor,
  fromLocalInput,
  leafProblem,
  opsFor,
  removeNode,
  splitList,
  toLocalInput,
  updateLeaf,
  valueShape,
  type NodePath,
} from './conditionTree'

const props = defineProps<{
  leaf: RuleConditionLeaf
  path: NodePath
}>()

const ctx = inject(CONDITION_BUILDER)!
const t = ctx.t

const def = computed(() => fieldDef(props.leaf.field))
const shape = computed(() => valueShape(props.leaf.op))

const fieldOptions = computed(() => {
  const options = fieldsFor(ctx.trigger(), ctx.clock()).map((f) => ({ value: f.name, label: ctx.fieldLabel(f.name) }))
  // A field from another trigger stays listed so the leaf still reads.
  if (!options.some((o) => o.value === props.leaf.field)) {
    options.push({ value: props.leaf.field, label: ctx.fieldLabel(props.leaf.field) })
  }
  return options
})

function opLabel(op: RuleConditionOp): string {
  const kind = def.value?.kind
  if ((kind === 'timestamp' || kind === 'number') && ['gt', 'lt', 'gte', 'lte'].includes(op)) {
    return t(`admin-rule-conditions-op-${kind}-${op}`)
  }
  return t(`admin-rule-conditions-op-${op.replace(/_/g, '-')}`)
}
const opOptions = computed(() => opsFor(props.leaf.field).map((op) => ({ value: op, label: opLabel(op) })))

/** Options for an enum or id field's picker. */
const choiceOptions = computed<{ value: string | number; label: string }[]>(() => {
  const field = props.leaf.field
  switch (field) {
    case 'ticket.workflow_state_id':
      return ctx.statusOptions.value
    case 'ticket.category_id':
      return ctx.categoryOptions.value
    case 'ticket.tag_ids':
      return ctx.tagOptions.value
    case 'event.changed_fields':
      return ctx.changedFieldOptions.value
    case 'ticket.priority':
      return (def.value?.values ?? []).map((v) => ({ value: v, label: t(`priority-${v}`) }))
    case 'ticket.workflow_state.category':
      return (def.value?.values ?? []).map((v) => ({
        value: v,
        label: v === 'merged' ? t('admin-rule-conditions-category-merged') : t(`workflow-category-${v.replace(/_/g, '-')}`),
      }))
    case 'reply.kind':
      return (def.value?.values ?? []).map((v) => ({ value: v, label: t(`admin-rule-editor-reply-kind-${v}`) }))
    default:
      return []
  }
})
const usesChoices = computed(() => choiceOptions.value.length > 0 || ['int_id', 'enum', 'int_array', 'string_array'].includes(def.value?.kind ?? ''))

const listValue = computed<unknown[]>(() => (Array.isArray(props.leaf.value) ? props.leaf.value : []))
const textValue = computed(() => (typeof props.leaf.value === 'string' ? props.leaf.value : ''))
const uuidList = computed(() => listValue.value.filter((v): v is string => typeof v === 'string'))

const problem = computed(() => leafProblem(props.leaf, ctx.trigger(), ctx.clock()))

function edit(change: { field?: string; op?: RuleConditionOp; value?: unknown }): void {
  ctx.edit(updateLeaf(ctx.root(), props.path, change))
}

function remove(): void {
  ctx.edit(removeNode(ctx.root(), props.path))
}

function addPerson(user: { uuid: string; name: string }): void {
  if (!user.uuid || uuidList.value.includes(user.uuid)) return
  ctx.rememberUser(user.uuid, user.name)
  edit({ value: [...uuidList.value, user.uuid] })
}

function removePerson(uuid: string): void {
  edit({ value: uuidList.value.filter((u) => u !== uuid) })
}

const pickerType = computed(() => (props.leaf.field === 'ticket.requester_uuid' ? 'requester' : 'assignee'))
</script>

<template>
  <div class="flex flex-col gap-1.5" data-test="condition-leaf">
    <div class="flex flex-wrap items-start gap-2">
      <BaseDropdown
        :model-value="leaf.field"
        :options="fieldOptions"
        :aria-label="t('admin-rule-conditions-field-label')"
        :disabled="ctx.disabled()"
        size="sm"
        class="w-44"
        @update:model-value="edit({ field: String($event) })"
      />
      <BaseDropdown
        :model-value="leaf.op"
        :options="opOptions"
        :aria-label="t('admin-rule-conditions-op-label')"
        :disabled="ctx.disabled()"
        size="sm"
        class="w-40"
        @update:model-value="edit({ op: String($event) as RuleConditionOp })"
      />

      <div v-if="shape !== 'none'" class="flex-1 min-w-[10rem]">
        <!-- People -->
        <template v-if="def?.kind === 'uuid'">
          <UserPicker
            v-if="shape === 'single'"
            :type="pickerType"
            :model-value="textValue"
            :current-user="textValue && ctx.userName(textValue) ? { uuid: textValue, name: ctx.userName(textValue)! } : null"
            :placeholder="t('admin-rule-conditions-person-placeholder')"
            @pick="ctx.rememberUser($event.uuid, $event.name)"
            @update:model-value="edit({ value: $event })"
          />
          <div v-else class="flex flex-col gap-1.5">
            <ul v-if="uuidList.length > 0" class="flex flex-wrap gap-1.5">
              <li
                v-for="uuid in uuidList"
                :key="uuid"
                class="inline-flex items-center gap-1 rounded-full border border-default pl-2.5 pr-0.5 text-xs text-primary"
              >
                <span>{{ ctx.userName(uuid) ?? t('admin-rule-conditions-person-unknown') }}</span>
                <IconButton
                  size="xs"
                  variant="ghost"
                  icon="close"
                  :label="t('admin-rule-conditions-person-remove')"
                  :disabled="ctx.disabled()"
                  @click="removePerson(uuid)"
                />
              </li>
            </ul>
            <UserPicker
              :type="pickerType"
              model-value=""
              :exclude="uuidList"
              :placeholder="t('admin-rule-conditions-people-placeholder')"
              @pick="addPerson"
            />
          </div>
        </template>

        <!-- A pick from a list: statuses, categories, tags, enum values -->
        <BaseDropdown
          v-else-if="usesChoices"
          :multiple="shape === 'list'"
          :model-value="(shape === 'list' ? listValue : (leaf.value ?? '')) as never"
          :options="choiceOptions"
          :placeholder="t('admin-rule-conditions-value-placeholder')"
          :aria-label="t('admin-rule-conditions-value-label')"
          :disabled="ctx.disabled()"
          size="sm"
          @update:model-value="edit({ value: $event })"
        />

        <FormInput
          v-else-if="def?.kind === 'timestamp'"
          type="datetime-local"
          size="sm"
          :model-value="toLocalInput(leaf.value)"
          :aria-label="t('admin-rule-conditions-value-label')"
          :disabled="ctx.disabled()"
          @update:model-value="edit({ value: fromLocalInput($event) })"
        />

        <FormNumber
          v-else-if="def?.kind === 'number'"
          size="sm"
          integer
          :model-value="typeof leaf.value === 'number' ? leaf.value : null"
          :aria-label="t('admin-rule-conditions-minutes-label')"
          @update:model-value="edit({ value: $event })"
        />

        <FormInput
          v-else-if="shape === 'list'"
          size="sm"
          :model-value="listValue.join(', ')"
          :placeholder="t('admin-rule-conditions-list-placeholder')"
          :aria-label="t('admin-rule-conditions-value-label')"
          :disabled="ctx.disabled()"
          @update:model-value="edit({ value: splitList($event) })"
        />

        <FormInput
          v-else
          size="sm"
          :model-value="textValue"
          :placeholder="leaf.op === 'matches_regex' ? t('admin-rule-conditions-pattern-placeholder') : t('admin-rule-conditions-text-placeholder')"
          :aria-label="t('admin-rule-conditions-value-label')"
          :disabled="ctx.disabled()"
          @update:model-value="edit({ value: $event })"
        />
      </div>
      <div v-else class="flex-1" />

      <IconButton
        size="sm"
        variant="ghost"
        icon="trash"
        :label="t('admin-rule-conditions-remove')"
        :disabled="ctx.disabled()"
        @click="remove"
      />
    </div>
    <p v-if="problem && (ctx.showProblems() || problem === 'admin-rule-conditions-problem-trigger')" class="text-xs text-status-warning">
      {{ t(problem) }}
    </p>
  </div>
</template>
