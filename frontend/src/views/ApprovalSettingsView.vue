<script setup lang="ts">
/**
 * Approvals: the workspace-wide behaviour. Which request types need approval,
 * and from whom, is set on each request type (Categories).
 */
import { computed, ref, watch } from 'vue'
import { RouterLink } from 'vue-router'
import { useQuery, useQueryCache } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import AlertMessage from '@/components/common/AlertMessage.vue'
import Button from '@/components/common/Button.vue'
import FormNumber from '@/components/common/FormNumber.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import ToggleSwitch from '@/components/common/ToggleSwitch.vue'
import { useToastStore } from '@nosdesk/core/stores/toast'
import { extractErrorMessage } from '@/utils/errors'
import {
  approvalSettingsService,
  type ApprovalSkipBy,
  type ApprovalWaitingDisplay,
} from '@nosdesk/core/services/approvalSettingsService'

const { $t: t } = useFluent()
const toast = useToastStore()
const queryCache = useQueryCache()
const KEY = ['approval-settings']
const settings = useQuery({ key: KEY, query: () => approvalSettingsService.get() })

const display = ref<ApprovalWaitingDisplay>('badge')
const skipBy = ref<ApprovalSkipBy>('admins')
const autoApprove = ref(false)
const days = ref<number | null>(3)
const saving = ref(false)
const error = ref('')

watch(
  () => settings.data.value,
  (s) => {
    if (!s) return
    display.value = s.waiting_display
    skipBy.value = s.skip_by
    autoApprove.value = s.auto_approve_days != null
    days.value = s.auto_approve_days ?? 3
  },
  { immediate: true },
)

const displayOptions = computed(() => [
  { value: 'badge' as const, label: t('approval-settings-display-badge') },
  { value: 'held' as const, label: t('approval-settings-display-held') },
])
const skipOptions = computed(() => [
  { value: 'nobody' as const, label: t('approval-settings-skip-nobody') },
  { value: 'admins' as const, label: t('approval-settings-skip-admins') },
  { value: 'agents' as const, label: t('approval-settings-skip-agents') },
])

async function save(): Promise<void> {
  saving.value = true
  error.value = ''
  try {
    await approvalSettingsService.save({
      waiting_display: display.value,
      skip_by: skipBy.value,
      auto_approve_days: autoApprove.value ? (days.value ?? 3) : null,
    })
    await queryCache.invalidateQueries({ key: KEY })
    toast.success(t('approval-settings-saved'))
  } catch (e) {
    error.value = extractErrorMessage(e, t('approval-settings-save-failed'))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-3xl">
      <div class="flex flex-col gap-2">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ t('approval-settings-title') }}</h1>
        <p class="text-secondary">
          {{ t('approval-settings-description') }}
          <RouterLink to="/admin/categories" class="text-accent hover:underline">{{ t('approval-settings-types-link') }}</RouterLink>
        </p>
      </div>

      <AlertMessage v-if="error" type="error" :message="error" />

      <form
        v-if="settings.data.value"
        class="flex flex-col gap-6 bg-surface border border-default rounded-xl p-6"
        @submit.prevent="save"
      >
        <div class="flex flex-col gap-1.5">
          <span class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ t('approval-settings-display-label') }}</span>
          <SegmentedControl v-model="display" :options="displayOptions" :aria-label="t('approval-settings-display-label')" />
          <p class="text-xs text-secondary">
            {{ display === 'held' ? t('approval-settings-display-held-hint') : t('approval-settings-display-badge-hint') }}
          </p>
        </div>

        <div class="flex flex-col gap-1.5">
          <span class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ t('approval-settings-skip-label') }}</span>
          <SegmentedControl v-model="skipBy" :options="skipOptions" :aria-label="t('approval-settings-skip-label')" />
          <p class="text-xs text-secondary">{{ t('approval-settings-skip-hint') }}</p>
        </div>

        <div class="flex flex-col gap-3">
          <ToggleSwitch
            v-model="autoApprove"
            :label="t('approval-settings-auto-label')"
            :description="t('approval-settings-auto-hint')"
          />
          <FormNumber
            v-if="autoApprove"
            v-model="days"
            class="max-w-48"
            :label="t('approval-settings-auto-days-label')"
            :min="1"
            :max="90"
            integer
          />
        </div>

        <div class="flex">
          <Button type="submit" class="ml-auto" :loading="saving">{{ t('approval-settings-save') }}</Button>
        </div>
      </form>
    </div>
  </div>
</template>
