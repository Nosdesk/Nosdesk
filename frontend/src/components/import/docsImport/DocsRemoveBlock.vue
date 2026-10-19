<script setup lang="ts">
/**
 * Remove imported pages, on Done and on a documentation import's page: the
 * ghost danger button with its rule, the confirmation with what goes to
 * Trash and which pages are kept and why, the state while it runs, then
 * the result in place of the button. Past the 30 days it says the window
 * closed.
 *
 * The counts come from the server when the button is pressed, so the
 * dialog never shows a stale plan; the removal checks again under its
 * locks before it moves anything. A request that times out or finds the
 * removal still running is followed until the job says it ran.
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
  docsImportService,
  type DocsImportJob,
  type DocsKeptPage,
  type DocsRemovalPlan,
} from '@nosdesk/core/services/docsImportService'

import { docsActionRefusalText, errorBody } from '../refusals'
import {
  docsRemovalState,
  keptLine,
  removeMessage,
  resultCollections,
  resultHeadline,
  type Destination,
} from './docsRemoveModel'

const props = defineProps<{
  job: DocsImportJob
  dest: Destination
}>()

const emit = defineEmits<{
  /** The job after removal ran, or as it is now after a refusal. */
  updated: [job: DocsImportJob]
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const date = (iso: string) => formatCompactDate(iso)

const state = computed(() => docsRemovalState(props.job))
const closesOn = computed(() => (props.job.removal_closes_at ? date(props.job.removal_closes_at) : ''))

const plan = ref<DocsRemovalPlan | null>(null)
const showDialog = ref(false)
const checking = ref(false)
const removing = ref(false)
const errorMessage = ref('')

const POLL_MS = 2000
const POLL_LIMIT = 150
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

function statusOf(e: unknown): number | undefined {
  return (e as { response?: { status?: number } })?.response?.status
}

async function reread(): Promise<DocsImportJob | null> {
  try {
    return await docsImportService.get(props.job.id)
  } catch {
    return null
  }
}

const hasRun = (j: DocsImportJob | null): j is DocsImportJob => !!j && (!!j.removed_at || !!j.remove_result)

/** The request may have reached a server still removing. */
function mayStillRun(e: unknown): boolean {
  const status = statusOf(e)
  return status === undefined || status === 502 || status === 503 || status === 504 || errorBody(e)?.code === 'import_busy'
}

/** A refusal: say why, and show the job as it is now. */
async function refused(e: unknown): Promise<boolean> {
  const worded = docsActionRefusalText(t, errorBody(e), closesOn.value)
  if (!worded) return false
  errorMessage.value = worded
  const found = await reread()
  if (found) emit('updated', found)
  return true
}

async function openDialog() {
  if (checking.value || removing.value) return
  checking.value = true
  errorMessage.value = ''
  try {
    plan.value = await docsImportService.removalPlan(props.job.id)
    showDialog.value = true
  } catch (e) {
    if (!(await refused(e))) errorMessage.value = t('docs-import-remove-load-failed')
  } finally {
    checking.value = false
  }
}

async function confirm() {
  if (removing.value) return
  removing.value = true
  showDialog.value = false
  errorMessage.value = ''
  try {
    emit('updated', await docsImportService.removePages(props.job.id))
  } catch (e) {
    if (errorBody(e)?.code !== 'import_busy' && (await refused(e))) return
    if (mayStillRun(e)) {
      for (let i = 0; i < POLL_LIMIT; i++) {
        await sleep(POLL_MS)
        const found = await reread()
        if (hasRun(found)) {
          emit('updated', found)
          return
        }
      }
      errorMessage.value = t('docs-import-remove-still-running-later')
      return
    }
    const found = await reread()
    if (hasRun(found)) emit('updated', found)
    else errorMessage.value = t('docs-import-remove-failed')
  } finally {
    removing.value = false
  }
}

const deletable = computed(() => plan.value?.deletable ?? 0)
const title = computed(() =>
  deletable.value > 0 ? t('docs-import-remove-title', { count: deletable.value }) : t('docs-import-remove-title-none'),
)
const message = computed(() => (plan.value ? removeMessage(t, plan.value, props.dest, closesOn.value) : ''))
const planKept = computed(() => plan.value?.kept ?? [])

const result = computed(() => props.job.remove_result ?? null)
const resultKept = computed(() => result.value?.kept ?? [])

const keptReason = (k: DocsKeptPage) => keptLine(t, k, date)
</script>

<template>
  <div v-if="state" class="flex flex-col gap-2" data-test="docs-remove" :data-state="state">
    <Callout v-if="state === 'removed' && result" severity="info" data-test="docs-remove-result">
      <template #header>
        <span class="font-medium text-primary">{{ resultHeadline(t, result) }}</span>
        <span class="block text-xs text-secondary mt-0.5">
          {{ $t('docs-import-remove-result-restore') }} {{ resultCollections(t, result, dest) }}
        </span>
      </template>
      <ul v-if="resultKept.length > 0" class="flex flex-col gap-1 px-4 py-3 text-sm" data-test="docs-remove-kept">
        <li v-for="k in resultKept" :key="k.id" class="flex flex-wrap items-baseline gap-x-1.5 min-w-0">
          <RouterLink
            :to="{ name: 'documentation-page', params: { path: String(k.id) } }"
            class="break-words min-w-0 text-accent hover:underline"
          >{{ k.title }}</RouterLink>
          <span class="text-tertiary">· {{ keptReason(k) }}</span>
        </li>
      </ul>
    </Callout>

    <p v-else-if="state === 'closed'" class="text-sm text-tertiary" data-test="docs-remove-closed">
      {{ $t('import-remove-window-closed', { date: closesOn }) }}
    </p>

    <div v-else-if="state === 'offer'" class="flex flex-wrap items-center gap-x-3 gap-y-1">
      <Button variant="ghost-danger" :loading="checking || removing" data-test="docs-remove-button" @click="openDialog">
        {{ $t('docs-import-remove-button') }}
      </Button>
      <span class="text-xs text-tertiary" data-test="docs-remove-window">
        {{ $t('docs-import-remove-window', { date: closesOn }) }}
      </span>
    </div>

    <p v-if="removing" role="status" class="text-sm text-secondary" data-test="docs-remove-running">
      {{ $t('docs-import-remove-running') }}
    </p>
    <ErrorBanner v-if="errorMessage" :message="errorMessage" />

    <ConfirmModal
      :show="showDialog"
      variant="danger"
      :title="title"
      :message="message"
      :confirm-label="$t('docs-import-remove-confirm', { count: deletable })"
      :confirm-disabled="deletable === 0"
      :loading="removing"
      @close="showDialog = false"
      @confirm="confirm"
    >
      <template #body>
        <details v-if="planKept.length > 0" class="text-sm" data-test="docs-remove-dialog-kept">
          <summary class="cursor-pointer text-accent hover:underline">
            {{ $t('docs-import-remove-which-kept', { count: planKept.length }) }}
          </summary>
          <ul class="mt-2 flex flex-col gap-1 max-h-48 overflow-y-auto">
            <li v-for="k in planKept" :key="k.id" class="flex flex-wrap items-baseline gap-x-1.5 min-w-0">
              <span class="text-primary break-words min-w-0">{{ k.title }}</span>
              <span class="text-tertiary">· {{ keptReason(k) }}</span>
            </li>
          </ul>
        </details>
      </template>
    </ConfirmModal>
  </div>
</template>
