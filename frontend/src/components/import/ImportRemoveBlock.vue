<script setup lang="ts">
/**
 * Remove imported assets, on Done and on a past import's page: the ghost
 * danger button with its window, the confirmation with what would be
 * deleted and what is kept and why, then the result in place of the
 * button. Past the 30 days it says the window closed.
 *
 * The counts come from the server when the button is pressed, so the
 * dialog never shows a stale plan; the removal checks again under its
 * locks before it deletes anything.
 *
 * A removal that fails is read back before it is called failed: another
 * admin may have removed the assets already, and a request that timed out
 * or found the removal still running is waited on until the job says it
 * ran.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import { RouterLink } from 'vue-router'

import Button from '@/components/common/Button.vue'
import Callout from '@/components/common/Callout.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import ErrorBanner from '@/components/common/ErrorBanner.vue'
import { formatCompactDate } from '@nosdesk/core/utils/dateUtils'
import {
  importService,
  type ImportJob,
  type KeptAsset,
  type RemovalPlan,
} from '@nosdesk/core/services/importService'

import { errorBody, removalRefusalText } from './refusals'
import { dialogMessage, reasonLine, removalState, resultLine } from './removeModel'

const props = defineProps<{
  job: ImportJob
  /** An enabled webhook hears about assets: the dialog says so. */
  webhooks?: boolean
}>()

const emit = defineEmits<{
  (e: 'removed', job: ImportJob): void
  /** The job as it is now, read again after a refusal. */
  (e: 'refreshed', job: ImportJob): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const lang = typeof document === 'undefined' ? undefined : document.documentElement.lang || undefined
const list = (items: string[]) =>
  typeof Intl.ListFormat === 'function'
    ? new Intl.ListFormat(lang, { type: 'conjunction' }).format(items)
    : items.join(', ')
const date = (iso: string) => formatCompactDate(iso)

const state = computed(() => removalState(props.job))
const closesOn = computed(() =>
  props.job.removal_closes_at ? formatCompactDate(props.job.removal_closes_at) : '',
)

const plan = ref<RemovalPlan | null>(null)
const showDialog = ref(false)
const checking = ref(false)
const removing = ref(false)
const errorMessage = ref('')
/** The removal is still running on the server and is being waited on. */
const waiting = ref(false)

function codeOf(e: unknown): { status?: number; code?: string } {
  const response = (e as { response?: { status?: number; data?: { code?: string } } })?.response
  return { status: response?.status, code: response?.data?.code }
}

/** Seconds between reads while a removal still runs, and how many. */
const POLL_MS = 2000
const POLL_LIMIT = 150
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

/** The job as it is now; `null` when it can't be read. */
async function reread(): Promise<ImportJob | null> {
  try {
    return await importService.get(props.job.id)
  } catch {
    return null
  }
}

const hasRun = (j: ImportJob | null): j is ImportJob => !!j && (!!j.removed_at || !!j.remove_result)

/** The request may have reached a server still removing: no answer, a
 *  gateway timeout, or the job's lock held. */
function mayStillRun(e: unknown): boolean {
  const { status, code } = codeOf(e)
  return status === undefined || status === 502 || status === 503 || status === 504 || code === 'import_busy'
}

/** Read the job until the removal shows on it, or give up. */
async function waitForRemoval() {
  waiting.value = true
  try {
    for (let i = 0; i < POLL_LIMIT; i++) {
      await sleep(POLL_MS)
      const found = await reread()
      if (hasRun(found)) {
        emit('removed', found)
        return
      }
    }
    errorMessage.value = t('import-remove-still-running-later')
  } finally {
    waiting.value = false
  }
}

/** A refusal: say why and show the job as it is now. `false` for a
 *  failure that isn't one. */
async function refused(e: unknown): Promise<boolean> {
  const code = codeOf(e).code
  const worded = removalRefusalText(t, errorBody(e))
  if (code === 'import_already_removed') {
    errorMessage.value = t('import-remove-already-removed')
  } else if (code === 'import_removal_window_closed') {
    errorMessage.value = t('import-remove-window-closed', { date: closesOn.value })
  } else if (worded) {
    errorMessage.value = worded
  } else {
    return false
  }
  const found = await reread()
  if (found) emit('refreshed', found)
  return true
}

async function openDialog() {
  if (checking.value) return
  checking.value = true
  errorMessage.value = ''
  try {
    plan.value = await importService.removalPlan(props.job.id)
    showDialog.value = true
  } catch (e) {
    if (!(await refused(e))) errorMessage.value = t('import-remove-load-failed')
  } finally {
    checking.value = false
  }
}

async function confirm() {
  if (removing.value) return
  removing.value = true
  errorMessage.value = ''
  try {
    const updated = await importService.removeCreated(props.job.id)
    showDialog.value = false
    emit('removed', updated)
  } catch (e) {
    showDialog.value = false
    if (await refused(e)) return
    if (mayStillRun(e)) {
      await waitForRemoval()
      return
    }
    // Read back before calling it failed: it may have run all the same.
    const found = await reread()
    if (hasRun(found)) emit('removed', found)
    else errorMessage.value = t('import-remove-failed')
  } finally {
    removing.value = false
  }
}

const deletable = computed(() => plan.value?.deletable ?? 0)
const title = computed(() =>
  deletable.value > 0 ? t('import-remove-title', { count: deletable.value }) : t('import-remove-title-none'),
)
const message = computed(() =>
  plan.value ? dialogMessage(t, plan.value, { webhooks: !!props.webhooks, list }) : '',
)
const planKept = computed(() => plan.value?.kept ?? [])

const result = computed(() => resultLine(t, props.job, date))
const resultKept = computed(() => props.job.remove_result?.kept ?? [])

const keptReason = (k: KeptAsset) => (k.reasons[0] ? reasonLine(t, k.reasons[0], date) : '')
</script>

<template>
  <div v-if="state" class="flex flex-col gap-2" data-test="import-remove" :data-state="state">
    <Callout v-if="state === 'removed'" severity="info" data-test="import-remove-result">
      <template #header>
        <span class="font-medium text-primary">{{ result }}</span>
      </template>
      <ul v-if="resultKept.length > 0" class="flex flex-col gap-1 px-4 py-3 text-sm" data-test="import-remove-kept">
        <li v-for="k in resultKept" :key="k.id" class="flex flex-wrap items-baseline gap-x-1.5 min-w-0">
          <RouterLink :to="`/assets/${k.id}`" class="flex flex-wrap items-baseline gap-x-1.5 min-w-0 text-accent hover:underline">
            <span v-if="k.tag" class="font-mono text-xs">{{ k.tag }}</span>
            <span class="break-words min-w-0">{{ k.name }}</span>
          </RouterLink>
          <span class="text-tertiary">· {{ keptReason(k) }}</span>
        </li>
      </ul>
    </Callout>

    <p v-else-if="state === 'closed'" class="text-sm text-tertiary" data-test="import-remove-closed">
      {{ $t('import-remove-window-closed', { date: closesOn }) }}
    </p>

    <div v-else class="flex flex-wrap items-center gap-x-3 gap-y-1">
      <Button
        variant="ghost-danger"
        :loading="checking || waiting"
        data-test="import-remove-button"
        @click="openDialog"
      >
        {{ $t('import-remove-button') }}
      </Button>
      <span class="text-xs text-tertiary" data-test="import-remove-window">
        {{ $t('import-remove-window', { date: closesOn }) }}
      </span>
    </div>

    <p v-if="waiting" role="status" class="text-sm text-secondary" data-test="import-remove-waiting">
      {{ $t('import-remove-still-running') }}
    </p>
    <ErrorBanner v-if="errorMessage" :message="errorMessage" />

    <ConfirmModal
      :show="showDialog"
      variant="danger"
      :title="title"
      :message="message"
      :confirm-label="$t('import-remove-confirm', { count: deletable })"
      :confirm-disabled="deletable === 0"
      :loading="removing"
      @close="showDialog = false"
      @confirm="confirm"
    >
      <template #body>
        <details v-if="planKept.length > 0" class="text-sm" data-test="import-remove-dialog-kept">
          <summary class="cursor-pointer text-accent hover:underline">
            {{ $t('import-remove-which-kept', { count: planKept.length }) }}
          </summary>
          <ul class="mt-2 flex flex-col gap-1 max-h-48 overflow-y-auto">
            <li v-for="k in planKept" :key="k.id" class="flex flex-wrap items-baseline gap-x-1.5 min-w-0">
              <span v-if="k.tag" class="font-mono text-xs text-primary">{{ k.tag }}</span>
              <span class="text-primary break-words min-w-0">{{ k.name }}</span>
              <span class="text-tertiary">· {{ keptReason(k) }}</span>
            </li>
          </ul>
        </details>
      </template>
    </ConfirmModal>
  </div>
</template>
