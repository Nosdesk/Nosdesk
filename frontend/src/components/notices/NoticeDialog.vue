<script setup lang="ts">
/**
 * Post, update or end a known-issue notice. Opened from an incident ticket
 * (the notice links it, so requesters can follow the issue) or from the
 * notices page.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'

import AlertMessage from '@/components/common/AlertMessage.vue'
import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'
import FormTextarea from '@/components/common/FormTextarea.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import Modal from '@/components/Modal.vue'
import { extractErrorMessage } from '@/utils/errors'
import {
  noticeService,
  type Notice,
  type NoticeSeverity,
} from '@nosdesk/core/services/noticeService'

const props = defineProps<{
  show: boolean
  /** Editing an existing notice; absent to post a new one. */
  notice?: Notice | null
  /** The incident ticket a new notice links. */
  ticketId?: number | null
  /** Pre-fills the title of a new notice. */
  defaultTitle?: string
  /** A live notice with no incident ticket, offered for `ticketId` to join. */
  linkable?: Notice | null
}>()
const emit = defineEmits<{ close: []; saved: [notice: Notice] }>()
const { $t: t } = useFluent()

const HOURS = ['1', '4', '24', '72'] as const
type Hours = (typeof HOURS)[number]

const title = ref('')
const body = ref('')
const severity = ref<NoticeSeverity>('degraded')
const hours = ref<Hours>('4')
const busy = ref(false)
const error = ref('')
// On the notices page (no ticket in hand) the incident ticket is typed in.
const incident = ref('')
const fromTicket = computed(() => props.ticketId != null)
const offerLink = computed(() => !props.notice && fromTicket.value && !!props.linkable)

const severityOptions = computed(() => [
  { value: 'info' as const, label: t('notice-severity-info') },
  { value: 'degraded' as const, label: t('notice-severity-degraded') },
  { value: 'outage' as const, label: t('notice-severity-outage') },
])
const durationOptions = computed(() =>
  HOURS.map((h) => ({ value: h, label: t('notice-duration', { hours: Number(h) }) })),
)

watch(
  () => props.show,
  (open) => {
    if (!open) return
    error.value = ''
    title.value = props.notice?.title ?? props.defaultTitle ?? ''
    body.value = props.notice?.body ?? ''
    severity.value = props.notice?.severity ?? 'degraded'
    hours.value = '4'
    incident.value = props.notice?.incident_ticket_id?.toString() ?? ''
  },
  { immediate: true },
)

async function save(): Promise<void> {
  if (!title.value.trim()) return
  busy.value = true
  error.value = ''
  const now = new Date()
  // Editing keeps the start; the chosen duration runs from now either way.
  const fields = {
    title: title.value.trim(),
    body: body.value.trim() || null,
    severity: severity.value,
    starts_at: props.notice?.starts_at ?? now.toISOString(),
    ends_at: new Date(now.getTime() + Number(hours.value) * 3600_000).toISOString(),
    incident_ticket_id: fromTicket.value
      ? (props.notice?.incident_ticket_id ?? props.ticketId ?? null)
      : incidentId(),
  }
  try {
    const saved = props.notice
      ? await noticeService.update(props.notice.id, fields)
      : await noticeService.create(fields)
    emit('saved', saved)
  } catch (e) {
    error.value = extractErrorMessage(e, t('notice-save-failed'))
  } finally {
    busy.value = false
  }
}

function incidentId(): number | null {
  const n = Number.parseInt(incident.value.replace('#', ''), 10)
  return Number.isFinite(n) && n > 0 ? n : null
}

/** Make this ticket the incident of the notice that's already live. */
async function linkExisting(): Promise<void> {
  const n = props.linkable
  if (!n || props.ticketId == null) return
  busy.value = true
  error.value = ''
  try {
    emit(
      'saved',
      await noticeService.update(n.id, {
        title: n.title,
        body: n.body,
        severity: n.severity,
        starts_at: n.starts_at,
        ends_at: n.ends_at,
        incident_ticket_id: props.ticketId,
      }),
    )
  } catch (e) {
    error.value = extractErrorMessage(e, t('notice-save-failed'))
  } finally {
    busy.value = false
  }
}

async function endNow(): Promise<void> {
  if (!props.notice) return
  busy.value = true
  try {
    emit('saved', await noticeService.end(props.notice.id))
  } catch (e) {
    error.value = extractErrorMessage(e, t('notice-save-failed'))
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <Modal
    :show="show"
    :title="notice ? t('notice-dialog-edit-title') : t('notice-dialog-title')"
    :description="t('notice-dialog-description')"
    size="md"
    @close="emit('close')"
  >
    <form class="flex flex-col gap-4" @submit.prevent="save">
      <AlertMessage v-if="error" type="error" :message="error" />
      <div
        v-if="offerLink && linkable"
        class="flex flex-col sm:flex-row sm:items-center gap-3 rounded-lg border border-default bg-surface-alt p-3"
      >
        <p class="flex-1 text-sm text-secondary">{{ t('notice-link-existing', { title: linkable.title }) }}</p>
        <Button type="button" variant="secondary" size="sm" :disabled="busy" @click="linkExisting">
          {{ t('notice-link-existing-action') }}
        </Button>
      </div>
      <FormInput v-model="title" :label="t('notice-title-label')" maxlength="120" required :disabled="busy" />
      <FormTextarea
        v-model="body"
        :label="t('notice-body-label')"
        :placeholder="t('notice-body-placeholder')"
        :rows="3"
        maxlength="1000"
        :disabled="busy"
      />
      <div class="flex flex-col gap-1.5">
        <span class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ t('notice-severity-label') }}</span>
        <SegmentedControl v-model="severity" :options="severityOptions" :aria-label="t('notice-severity-label')" />
      </div>
      <div class="flex flex-col gap-1.5">
        <span class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ t('notice-duration-label') }}</span>
        <SegmentedControl v-model="hours" :options="durationOptions" :aria-label="t('notice-duration-label')" />
      </div>
      <FormInput
        v-if="!fromTicket"
        v-model="incident"
        :label="t('notice-incident-label')"
        :description="t('notice-incident-hint')"
        placeholder="#123"
        inputmode="numeric"
        :disabled="busy"
      />
      <p v-if="fromTicket || notice?.incident_ticket_id" class="text-xs text-secondary">
        {{ t('notice-follow-hint') }}
      </p>
      <div class="modal-actions flex items-center gap-2">
        <Button v-if="notice" type="button" variant="ghost" :disabled="busy" @click="endNow">
          {{ t('notice-end-now') }}
        </Button>
        <Button type="button" variant="secondary" class="ml-auto" :disabled="busy" @click="emit('close')">
          {{ t('notice-cancel') }}
        </Button>
        <Button type="submit" :loading="busy" :disabled="!title.trim()">
          {{ notice ? t('notice-update') : t('notice-post') }}
        </Button>
      </div>
    </form>
  </Modal>
</template>
