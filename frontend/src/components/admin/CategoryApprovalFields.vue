<script setup lang="ts">
/**
 * A request type's approval settings: whether it needs approval, from whom
 * (named people and/or the requester's manager), and whether one approval is
 * enough. Shared by the desktop panel and the mobile modal.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'

import IconButton from '@/components/common/IconButton.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import ToggleSwitch from '@/components/common/ToggleSwitch.vue'
import UserPicker from '@/components/ticketComponents/UserPicker.vue'
import type { PickerUser } from '@/composables/useUserPicker'
import type { ApprovalRule, CategoryApprover } from '@nosdesk/core/types/category'

export interface ApprovalFields {
  approval_required: boolean
  approval_rule: ApprovalRule
  approval_by_manager: boolean
  approvers: CategoryApprover[]
}

const model = defineModel<ApprovalFields>({ required: true })
const { $t: t } = useFluent()

const picking = ref('')
const ruleOptions = computed(() => [
  { value: 'any' as const, label: t('admin-categories-approval-rule-any') },
  { value: 'all' as const, label: t('admin-categories-approval-rule-all') },
])
// Manager approval alone leaves anyone without a manager stuck waiting.
const noFallback = computed(
  () => model.value.approval_by_manager && model.value.approvers.length === 0,
)
const noApprovers = computed(
  () => !model.value.approval_by_manager && model.value.approvers.length === 0,
)

function add(user: PickerUser): void {
  picking.value = ''
  if (model.value.approvers.some((a) => a.uuid === user.uuid)) return
  model.value = { ...model.value, approvers: [...model.value.approvers, { uuid: user.uuid, name: user.name }] }
}
function remove(uuid: string): void {
  model.value = { ...model.value, approvers: model.value.approvers.filter((a) => a.uuid !== uuid) }
}
function set<K extends keyof ApprovalFields>(key: K, value: ApprovalFields[K]): void {
  model.value = { ...model.value, [key]: value }
}
</script>

<template>
  <div class="flex flex-col gap-3">
    <ToggleSwitch
      :model-value="model.approval_required"
      size="sm"
      :label="t('admin-categories-approval-label')"
      :description="t('admin-categories-approval-hint')"
      @update:model-value="set('approval_required', $event)"
    />
    <div v-if="model.approval_required" class="flex flex-col gap-3 pl-3 border-l-2 border-default">
      <ToggleSwitch
        :model-value="model.approval_by_manager"
        size="sm"
        :label="t('admin-categories-approval-manager-label')"
        :description="t('admin-categories-approval-manager-hint')"
        @update:model-value="set('approval_by_manager', $event)"
      />
      <div class="flex flex-col gap-1.5">
        <span class="text-sm font-medium text-primary">{{ t('admin-categories-approval-approvers-label') }}</span>
        <ul v-if="model.approvers.length" class="flex flex-wrap gap-1.5">
          <li
            v-for="a in model.approvers"
            :key="a.uuid"
            class="flex items-center gap-1 rounded-full bg-surface-alt border border-default pl-2.5 pr-0.5 py-0.5 text-sm text-primary"
          >
            {{ a.name }}
            <IconButton
              icon="close"
              size="xs"
              :label="t('admin-categories-approval-remove', { name: a.name })"
              @click="remove(a.uuid)"
            />
          </li>
        </ul>
        <UserPicker
          v-model="picking"
          type="requester"
          hide-inline-clear
          :placeholder="t('admin-categories-approval-add-placeholder')"
          @pick="add"
        />
      </div>
      <div v-if="model.approvers.length > 1 || (model.approval_by_manager && model.approvers.length)" class="flex flex-col gap-1.5">
        <span class="text-sm font-medium text-primary">{{ t('admin-categories-approval-rule-label') }}</span>
        <SegmentedControl
          :model-value="model.approval_rule"
          :options="ruleOptions"
          :aria-label="t('admin-categories-approval-rule-label')"
          @update:model-value="set('approval_rule', $event as ApprovalRule)"
        />
      </div>
      <p v-if="noFallback" class="text-xs text-status-warning">{{ t('admin-categories-approval-no-fallback') }}</p>
      <p v-else-if="noApprovers" class="text-xs text-status-warning">{{ t('admin-categories-approval-none') }}</p>
    </div>
  </div>
</template>
