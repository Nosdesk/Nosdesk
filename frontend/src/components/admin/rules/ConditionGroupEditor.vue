<script setup lang="ts">
/**
 * A group of conditions: all of them, any of them or none of them must
 * hold. Holds leaves and further groups; the root group is the rule's
 * conditions and can't be removed.
 */
import { computed, inject } from 'vue'

import Button from '@/components/common/Button.vue'
import IconButton from '@/components/common/IconButton.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import ConditionLeafEditor from './ConditionLeafEditor.vue'
import { CONDITION_BUILDER, MAX_NESTED_GROUPS } from './conditionContext'
import {
  addChild,
  emptyGroup,
  newLeaf,
  removeNode,
  setMode,
  type ConditionGroup,
  type GroupMode,
  type NodePath,
} from './conditionTree'

defineOptions({ name: 'ConditionGroupEditor' })

const props = defineProps<{
  group: ConditionGroup
  path: NodePath
}>()

const ctx = inject(CONDITION_BUILDER)!
const t = ctx.t

const isRoot = computed(() => props.path.length === 0)

// "Not all of these" only comes from a rule written through the API, so it's
// offered only to a group already set to it.
const modeOptions = computed(() => {
  const modes: GroupMode[] = ['all', 'any', 'none']
  if (props.group.mode === 'not_all') modes.push('not_all')
  return modes.map((mode) => ({ value: mode, label: t(`admin-rule-conditions-mode-${mode.replace(/_/g, '-')}`) }))
})

const canNest = computed(() => props.path.length < MAX_NESTED_GROUPS)
const firstField = () => {
  const trigger = ctx.trigger()
  return trigger === 'ticket_replied' ? 'reply.kind' : 'ticket.priority'
}

function addCondition(): void {
  ctx.edit(addChild(ctx.root(), props.path, newLeaf(firstField())))
}

function addGroup(): void {
  const group = emptyGroup(props.group.mode === 'any' ? 'all' : 'any')
  group.children.push(newLeaf(firstField()))
  ctx.edit(addChild(ctx.root(), props.path, group))
}

function changeMode(mode: GroupMode): void {
  ctx.edit(setMode(ctx.root(), props.path, mode))
}

function remove(): void {
  ctx.edit(removeNode(ctx.root(), props.path))
}
</script>

<template>
  <div
    class="flex flex-col gap-3"
    :class="isRoot ? '' : 'rounded-lg border border-default bg-surface-alt p-3'"
    data-test="condition-group"
  >
    <div v-if="group.children.length > 0 || !isRoot" class="flex flex-wrap items-center gap-2">
      <span class="text-sm text-secondary">{{ t('admin-rule-conditions-match') }}</span>
      <SegmentedControl
        :model-value="group.mode"
        :options="modeOptions"
        :aria-label="t('admin-rule-conditions-match')"
        size="sm"
        @update:model-value="changeMode($event)"
      />
      <IconButton
        v-if="!isRoot"
        class="ml-auto"
        size="sm"
        variant="ghost"
        icon="trash"
        :label="t('admin-rule-conditions-remove-group')"
        :disabled="ctx.disabled()"
        @click="remove"
      />
    </div>

    <p v-if="isRoot && group.children.length === 0" class="text-sm text-secondary">
      {{ t('admin-rule-conditions-empty') }}
    </p>
    <p v-else-if="group.children.length === 0" class="text-xs text-status-warning">
      {{ t('admin-rule-conditions-problem-group-empty') }}
    </p>

    <ul v-if="group.children.length > 0" class="flex flex-col gap-3">
      <li v-for="(child, i) in group.children" :key="i">
        <ConditionLeafEditor v-if="child.kind === 'leaf'" :leaf="child" :path="[...path, i]" />
        <ConditionGroupEditor v-else :group="child" :path="[...path, i]" />
      </li>
    </ul>

    <div class="flex flex-wrap items-center gap-2">
      <Button variant="ghost" size="sm" icon="add" :disabled="ctx.disabled()" @click="addCondition">
        {{ t('admin-rule-conditions-add') }}
      </Button>
      <Button v-if="canNest" variant="ghost" size="sm" icon="add" :disabled="ctx.disabled()" @click="addGroup">
        {{ t('admin-rule-conditions-add-group') }}
      </Button>
    </div>
  </div>
</template>
