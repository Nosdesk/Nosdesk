<script setup lang="ts">
/**
 * One request waiting for the signed-in person's approval. The email link
 * lands here; nothing is decided until a button is pressed (mail scanners
 * follow links). A decline needs a reason, which the requester sees.
 */
import { computed, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { useQuery, useQueryCache } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import FormTextarea from '@/components/common/FormTextarea.vue'
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils'
import { extractErrorMessage } from '@/utils/errors'

import PortalLayout from '../components/PortalLayout.vue'
import { decideApproval, getMyApproval } from '../service'

const props = defineProps<{ id: string }>()
const { $t: t } = useFluent()
const queryCache = useQueryCache()

const ticketId = computed(() => Number(props.id))
const key = computed(() => ['portal', 'approval', ticketId.value])
const detail = useQuery({ key, query: () => getMyApproval(ticketId.value) })

const declining = ref(false)
const reason = ref('')
const busy = ref(false)
const error = ref('')

const approval = computed(() => detail.data.value?.approval)
// With several approvers, show where the others stand.
const several = computed(() => (approval.value?.approvers.length ?? 0) > 1)
// Where things stand when the viewer can't (or no longer needs to) decide.
const outcome = computed(() => {
  const d = detail.data.value
  if (!d) return ''
  if (d.mine.decision === 'approved') return t('portal-approval-you-approved')
  if (d.mine.decision === 'declined') return t('portal-approval-you-declined')
  switch (d.approval.approval_state) {
    case 'approved':
      return t('portal-approval-already-approved')
    case 'declined':
      return t('portal-approval-already-declined')
    case 'skipped':
      return t('portal-approval-no-longer-needed')
    default:
      return ''
  }
})

function statusLabel(decision: string | null): string {
  if (decision === 'approved') return t('portal-approval-status-approved')
  if (decision === 'declined') return t('portal-approval-status-declined')
  if (decision === 'skipped') return t('portal-approval-status-skipped')
  return t('portal-approval-status-waiting')
}

async function decide(approve: boolean): Promise<void> {
  busy.value = true
  error.value = ''
  try {
    await decideApproval(ticketId.value, approve, approve ? undefined : reason.value.trim())
    declining.value = false
    await queryCache.invalidateQueries({ key: key.value })
    void queryCache.invalidateQueries({ key: ['portal', 'approvals'] })
  } catch (e) {
    error.value = extractErrorMessage(e, t('portal-approval-failed'))
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <PortalLayout>
    <RouterLink to="/approvals" class="text-sm text-secondary hover:text-primary">
      {{ t('portal-approval-back') }}
    </RouterLink>
    <p v-if="detail.error.value && !detail.data.value" class="text-sm text-status-error">
      {{ t('portal-approval-not-found') }}
    </p>
    <template v-else-if="approval && detail.data.value">
      <div class="flex flex-col gap-1">
        <span class="text-xs font-medium uppercase tracking-wide text-tertiary">{{ t('portal-approval-eyebrow') }}</span>
        <h1 class="text-xl font-semibold text-primary">{{ approval.title }}</h1>
        <p class="text-xs text-tertiary">
          {{ t('portal-approvals-from', { name: approval.requester_name ?? '' }) }}
          <template v-if="approval.request_type"> · {{ approval.request_type }}</template>
          · {{ formatRelativeTime(approval.created_at) }}
        </p>
      </div>

      <div v-if="approval.description" class="bg-surface border border-default rounded-xl p-4 text-sm text-primary whitespace-pre-wrap break-words">
        {{ approval.description }}
      </div>

      <ul v-if="several" class="flex flex-col gap-1 text-sm text-secondary">
        <li v-for="a in approval.approvers" :key="a.uuid">{{ a.name }}: {{ statusLabel(a.decision) }}</li>
      </ul>

      <p v-if="error" role="alert" class="text-sm text-status-error">{{ error }}</p>

      <div v-if="detail.data.value.can_decide" class="bg-surface border border-default rounded-xl p-4 flex flex-col gap-3">
        <template v-if="declining">
          <FormTextarea
            v-model="reason"
            :label="t('portal-approval-decline-reason')"
            :description="t('portal-approval-decline-hint')"
            :rows="3"
            required
            :disabled="busy"
          />
          <div class="flex items-center gap-2">
            <Button variant="danger" :loading="busy" :disabled="!reason.trim()" @click="decide(false)">
              {{ t('portal-approval-confirm-decline') }}
            </Button>
            <Button variant="ghost" :disabled="busy" @click="declining = false">{{ t('portal-approval-cancel') }}</Button>
          </div>
        </template>
        <div v-else class="flex flex-wrap items-center gap-2">
          <Button icon="check" :loading="busy" @click="decide(true)">{{ t('portal-approval-approve') }}</Button>
          <Button variant="secondary" :disabled="busy" @click="declining = true">{{ t('portal-approval-decline') }}</Button>
        </div>
      </div>
      <p v-else-if="outcome" class="text-sm text-secondary bg-surface border border-default rounded-xl p-4">
        {{ outcome }}
      </p>
    </template>
  </PortalLayout>
</template>
