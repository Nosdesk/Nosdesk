<script setup lang="ts">
/**
 * A ticket's approval, above its details. While it waits: who's been asked and
 * where each stands, Approve/Decline for a waiting approver, and Skip for staff
 * the workspace allows. Once decided: one line saying how.
 */
import { computed, ref, watch } from 'vue'
import { useQuery, useQueryCache } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import FormTextarea from '@/components/common/FormTextarea.vue'
import Icon from '@/components/common/Icon.vue'
import { useToastStore } from '@nosdesk/core/stores/toast'
import { extractErrorMessage } from '@/utils/errors'
import { approvalService, type ApprovalState } from '@nosdesk/core/services/approvalService'

const props = defineProps<{ ticketId: number; state: ApprovalState }>()
const { $t: t } = useFluent()
const toast = useToastStore()
const queryCache = useQueryCache()

const key = computed(() => ['ticket-approval', props.ticketId])
const approval = useQuery({ key, query: () => approvalService.get(props.ticketId) })
// The pool's state moves first (a decision anywhere); refetch the detail.
watch(
  () => props.state,
  () => void queryCache.invalidateQueries({ key: key.value }),
)

const data = computed(() => approval.data.value)
const mode = ref<'none' | 'decline' | 'skip'>('none')
const reason = ref('')
const busy = ref(false)

const decidedLine = computed(() => {
  const rows = data.value?.approval.approvers ?? []
  const by = (d: string) => rows.filter((r) => r.decision === d).map((r) => r.name).join(', ')
  const reasonOf = (d: string) => rows.find((r) => r.decision === d && r.comment)?.comment ?? ''
  switch (props.state) {
    case 'approved':
      return by('approved')
        ? t('approval-banner-approved-by', { names: by('approved') })
        : t('approval-banner-approved')
    case 'declined':
      return t('approval-banner-declined-by', { names: by('declined'), reason: reasonOf('declined') })
    case 'skipped':
      return reasonOf('skipped')
        ? t('approval-banner-skipped-reason', { reason: reasonOf('skipped') })
        : t('approval-banner-skipped')
    default:
      return ''
  }
})

function statusLabel(decision: string | null): string {
  if (decision === 'approved') return t('approval-banner-status-approved')
  if (decision === 'declined') return t('approval-banner-status-declined')
  if (decision === 'skipped') return t('approval-banner-status-skipped')
  return t('approval-banner-status-waiting')
}

async function run(action: () => Promise<void>, done: string): Promise<void> {
  busy.value = true
  try {
    await action()
    mode.value = 'none'
    reason.value = ''
    toast.success(done)
    await queryCache.invalidateQueries({ key: key.value })
  } catch (e) {
    toast.error(extractErrorMessage(e, t('approval-banner-failed')))
  } finally {
    busy.value = false
  }
}
const approve = () =>
  run(() => approvalService.decide(props.ticketId, true), t('approval-banner-done-approved'))
const decline = () =>
  run(
    () => approvalService.decide(props.ticketId, false, reason.value.trim()),
    t('approval-banner-done-declined'),
  )
const skip = () =>
  run(() => approvalService.skip(props.ticketId, reason.value.trim()), t('approval-banner-done-skipped'))
</script>

<template>
  <div
    v-if="state === 'pending'"
    class="flex flex-col gap-3 rounded-lg border border-amber-500/40 bg-amber-500/10 px-3 py-2.5 text-sm"
    role="region"
    :aria-label="t('approval-banner-title')"
  >
    <div class="flex items-start gap-2">
      <Icon name="clock" size="sm" class="mt-0.5 text-amber-600 dark:text-amber-400 flex-shrink-0" />
      <div class="flex flex-col gap-1 flex-1 min-w-0">
        <span class="font-medium text-amber-900 dark:text-amber-100">{{ t('approval-banner-title') }}</span>
        <p v-if="data?.no_approver" class="text-amber-800 dark:text-amber-200">
          {{ t('approval-banner-no-approver') }}
        </p>
        <ul v-else-if="data" class="flex flex-wrap gap-x-3 gap-y-1 text-amber-800 dark:text-amber-200">
          <li v-for="a in data.approval.approvers" :key="a.uuid">
            {{ a.name }} <span class="text-xs opacity-80">({{ statusLabel(a.decision) }})</span>
          </li>
        </ul>
      </div>
    </div>

    <div v-if="mode !== 'none'" class="flex flex-col gap-2">
      <FormTextarea
        v-model="reason"
        :label="mode === 'decline' ? t('approval-banner-decline-reason') : t('approval-banner-skip-reason')"
        :rows="2"
        :required="mode === 'decline'"
        :disabled="busy"
      />
      <div class="flex items-center gap-2">
        <Button
          size="xs"
          :variant="mode === 'decline' ? 'danger' : 'primary'"
          :loading="busy"
          :disabled="mode === 'decline' && !reason.trim()"
          @click="mode === 'decline' ? decline() : skip()"
        >
          {{ mode === 'decline' ? t('approval-banner-confirm-decline') : t('approval-banner-confirm-skip') }}
        </Button>
        <Button size="xs" variant="ghost" :disabled="busy" @click="mode = 'none'">{{ t('approval-banner-cancel') }}</Button>
      </div>
    </div>
    <div v-else-if="data?.can_decide || data?.can_skip" class="flex flex-wrap items-center gap-2">
      <template v-if="data?.can_decide">
        <Button size="xs" icon="check" :loading="busy" @click="approve">{{ t('approval-banner-approve') }}</Button>
        <Button size="xs" variant="secondary" :disabled="busy" @click="mode = 'decline'">{{ t('approval-banner-decline') }}</Button>
      </template>
      <Button v-if="data?.can_skip" size="xs" variant="ghost" class="ml-auto" :disabled="busy" @click="mode = 'skip'">
        {{ t('approval-banner-skip') }}
      </Button>
    </div>
  </div>
  <p
    v-else-if="decidedLine"
    class="flex items-center gap-2 rounded-lg border border-default bg-surface-alt px-3 py-2 text-sm text-secondary"
  >
    <Icon :name="state === 'declined' ? 'close' : 'checkCircle'" size="sm" class="flex-shrink-0" />
    {{ decidedLine }}
  </p>
</template>
