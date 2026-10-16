<script setup lang="ts">
/**
 * Bulk import wizard: Start, Map, Match values (when needed), Review, Done.
 *
 *  - Start  : pick the type and drop or pick a file, which uploads at
 *             once. The server parses the .csv or .xlsx and answers with
 *             the job in `parsed`: the file's columns and a suggested
 *             mapping. A refused file stays on Start with the reason.
 *  - Map    : for a workbook, the sheet panel picks the sheet and header
 *             row (`PUT source`, back to `parsed`); the detection line can
 *             switch the type (`PUT type`); then the admin checks the
 *             mapping and options, and "Check" stores them and runs the
 *             dry run (`dry_run_done`). "Change file" deletes the job and
 *             returns to Start.
 *  - Match values : shown after the check when a value in a mapped column
 *             has no match, is Guessed, or is a saved answer that drops
 *             data; "Check again" stores the answers (`PUT values`) and
 *             checks the rows with them.
 *  - Review : what the dry run found, row by row, and the error report;
 *             Import commits.
 *  - Done   : the receipt: the headline, the four tiles, what wasn't
 *             imported, what it created and changed, the error report
 *             (skipped and partly imported rows), the assets it touched,
 *             and another file of the same type.
 *
 * The job id rides in `?job=` so a reload or a link resumes at the step
 * the job's status implies.
 */
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useRoute, useRouter } from 'vue-router'
import { useQuery, useQueryCache } from '@pinia/colada'

import BackButton from '@/components/common/BackButton.vue'
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import Button from '@/components/common/Button.vue'
import Callout from '@/components/common/Callout.vue'
import ErrorBanner from '@/components/common/ErrorBanner.vue'
import Icon from '@/components/common/Icon.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import Spinner from '@/components/common/Spinner.vue'
import { formatFileSize } from '@nosdesk/core/utils/formatFileSize'
import webhookService from '@nosdesk/core/services/webhookService'
import ImportMapStep from '@/components/import/ImportMapStep.vue'
import ImportReviewStep from '@/components/import/ImportReviewStep.vue'
import ImportSheetPanel from '@/components/import/ImportSheetPanel.vue'
import ImportValuesStep from '@/components/import/ImportValuesStep.vue'
import { detailPath, doneHeadline, reportOffered } from '@/components/import/historyModel'
import { outcomes, planDelta, webhookHearsAssets } from '@/components/import/reviewModel'
import { cards as valueCards, needsValues } from '@/components/import/valuesModel'
import { useAssetKindsQuery } from '@/composables/useAssetKindsQuery'
import {
  importFieldsQueryKey,
  importService,
  importValuesQueryKey,
  type ColumnMapping,
  type ImportJob,
  type ImportJobType,
  type ImportOptions,
  type ImportValues,
  type ValueAnswers,
} from '@nosdesk/core/services/importService'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const route = useRoute()
const router = useRouter()

type Step = 'start' | 'map' | 'values' | 'review' | 'done'
const STEP_KEYS: Record<Step, string> = {
  start: 'import-step-start',
  map: 'import-step-map',
  values: 'import-step-values',
  review: 'import-step-review',
  done: 'import-step-done',
}

// The type Start opens on: the last one this browser imported or
// switched to.
const LAST_TYPE_KEY = 'nosdesk.import.lastType'
const JOB_TYPES: ImportJobType[] = ['assets', 'users', 'tickets']

function readLastType(): ImportJobType {
  try {
    const stored = localStorage.getItem(LAST_TYPE_KEY)
    return JOB_TYPES.find((t) => t === stored) ?? 'assets'
  } catch {
    return 'assets'
  }
}

function rememberType(type: ImportJobType) {
  try {
    localStorage.setItem(LAST_TYPE_KEY, type)
  } catch {
    // Storage blocked: Start opens on Assets next time.
  }
}

const step = ref<Step>('start')
const selectedType = ref<ImportJobType>(readLastType())
const fileInput = ref<HTMLInputElement | null>(null)
const dropZone = ref<HTMLButtonElement | null>(null)
const isDragOver = ref(false)
/** The file being read, named on the drop zone while it uploads. */
const reading = ref<{ name: string; size: string } | null>(null)
/** Why the last file was refused, with its name and size. */
const uploadError = ref<{ message: string; detail: string } | null>(null)

const job = ref<ImportJob | null>(null)
const busy = ref<
  'upload' | 'source' | 'type' | 'discard' | 'check' | 'commit' | 'resume' | null
>(null)
const downloadingReport = ref(false)
const errorMessage = ref('')
/** Something about the commit worth reading that isn't an error: the plan
 *  changed under it, or it is still running. */
const commitNotice = ref('')

const jobType = computed<ImportJobType>(() => job.value?.job_type ?? selectedType.value)

const fieldsQuery = useQuery({
  key: () => importFieldsQueryKey(jobType.value),
  query: () => importService.fields(jobType.value),
  enabled: () =>
    step.value === 'map' || step.value === 'values' || step.value === 'review' || step.value === 'done',
})
const fields = computed(() => fieldsQuery.data.value ?? [])

const queryCache = useQueryCache()

/** The checked job's values: for Match values, and for Review's line when
 *  the step wasn't needed. Keyed on the job's last change. */
const valuesQuery = useQuery({
  key: () => importValuesQueryKey(job.value?.id ?? '', job.value?.updated_at ?? ''),
  query: () => importService.values(job.value!.id),
  enabled: () =>
    !!job.value &&
    job.value.status === 'dry_run_done' &&
    (step.value === 'values' || step.value === 'review'),
})
const valuesView = computed<ImportValues | undefined>(() => valuesQuery.data.value)

/** Whether Match values belongs in this import's steps: some value is
 *  listed there, or the admin is on it. */
const valuesStepShown = computed(
  () =>
    step.value === 'values' ||
    (!!valuesView.value && valueCards(valuesView.value, {}).length > 0),
)

/** Every mapped value matched without a question: Review says so. */
const valuesAllMatched = computed(
  () =>
    !!valuesView.value &&
    valuesView.value.columns.length > 0 &&
    valueCards(valuesView.value, {}).length === 0,
)

/** The workspace's webhooks, for the confirmation's line about asset
 *  events. Read on Review of an asset import. */
const webhooksQuery = useQuery({
  key: ['webhooks'],
  query: () => webhookService.listWebhooks(),
  enabled: () => step.value === 'review' && jobType.value === 'assets',
})
const webhooksHearAssets = computed(() => webhookHearsAssets(webhooksQuery.data.value ?? []))

/** The field whose card Match values opens on: Review's "Fix at Match
 *  values". */
const valuesFocus = ref<string | null>(null)

const STEPS = computed<Step[]>(() =>
  valuesStepShown.value
    ? ['start', 'map', 'values', 'review', 'done']
    : ['start', 'map', 'review', 'done'],
)

const { kinds: assetKinds } = useAssetKindsQuery()
const kindOptions = computed(() =>
  assetKinds.value.map((k) => ({ value: k.slug, label: k.label })),
)

const typeOptions = computed(() => [
  { value: 'assets' as ImportJobType, label: t('csv-import-type-assets') },
  { value: 'users' as ImportJobType, label: t('csv-import-type-users') },
  { value: 'tickets' as ImportJobType, label: t('csv-import-type-tickets') },
])

const stepIndex = computed(() => STEPS.value.indexOf(step.value))

const linkClass =
  'text-accent hover:underline focus:outline-none focus-visible:ring-1 focus-visible:ring-accent rounded'

// ---- job + route ----

function setJobQuery(id: string | null) {
  const query = { ...route.query }
  if (id) query.job = id
  else delete query.job
  router.replace({ query })
}

function stepForStatus(status: ImportJob['status']): Step {
  switch (status) {
    case 'parsed':
      return 'map'
    case 'dry_run_done':
    case 'committing':
      return 'review'
    case 'done':
      return 'done'
    default:
      return 'start'
  }
}

async function resume(id: string) {
  busy.value = 'resume'
  try {
    const found = await importService.get(id)
    // A past import whose detail is gone reads on its own page; a draft
    // past its 30 days can't go on.
    if (found.status === 'done' && found.pruned_at) {
      router.replace(detailPath(found.id))
      return
    }
    if (found.state === 'expired') {
      selectedType.value = found.job_type
      job.value = null
      step.value = 'start'
      errorMessage.value = t('import-history-expired-resume')
      setJobQuery(null)
      return
    }
    // A job checked before column mapping existed can't go on.
    if (stepForStatus(found.status) === 'map' || stepForStatus(found.status) === 'review') {
      if (!found.source) throw new Error('job predates column mapping')
    }
    job.value = found
    selectedType.value = found.job_type
    step.value = stepForStatus(found.status)
    // Back from Saved matches returns to Match values.
    if (route.query.step === 'values' && found.status === 'dry_run_done') step.value = 'values'
    if (route.query.step !== undefined) {
      const query = { ...route.query }
      delete query.step
      router.replace({ query })
    }
    if (found.status === 'failed') {
      errorMessage.value = found.error_message ?? t('import-error-generic')
    }
    if (found.status === 'committing') {
      busy.value = 'commit'
      await waitForCommit(found.id)
    }
  } catch {
    job.value = null
    step.value = 'start'
    errorMessage.value = t('import-resume-failed')
    setJobQuery(null)
  } finally {
    busy.value = null
  }
}

onMounted(() => {
  const id = route.query.job
  if (typeof id === 'string' && id) resume(id)
})

function apiError(e: unknown, fallbackKey: string): string {
  const err = e as { response?: { data?: { error?: string } } }
  return err?.response?.data?.error ?? (e instanceof Error ? e.message : t(fallbackKey))
}

/** A step that failed: a draft whose file is gone goes back to Start to
 *  upload it again, anything else shows the error where it is. */
function stepFailed(e: unknown, fallbackKey: string) {
  const code = (e as { response?: { data?: { code?: string } } })?.response?.data?.code
  if (code === 'import_file_missing') {
    job.value = null
    step.value = 'start'
    setJobQuery(null)
    errorMessage.value = t('import-file-missing')
    return
  }
  errorMessage.value = apiError(e, fallbackKey)
}

// ---- start ----

/** The kind whose attribute columns the asset template carries; empty for
 *  the core columns only. */
const templateKind = ref('')
const templateKindOptions = computed(() => [
  { value: '', label: t('import-template-kind-none') },
  ...kindOptions.value,
])

function downloadTemplate() {
  const kind = selectedType.value === 'assets' ? templateKind.value : ''
  window.location.href = importService.templateUrl(selectedType.value, kind || undefined)
}

function chooseFile() {
  if (busy.value) return
  fileInput.value?.click()
}

function pickFile(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  // Cleared so picking the same file again after an error reads it again.
  input.value = ''
  if (file) acceptFile(file)
}

const XLSX_TYPE = 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet'

/** Upload a dropped or picked file at once; a refusal stays on Start. */
async function acceptFile(file: File) {
  if (busy.value) return
  const name = file.name.toLowerCase()
  const looksImportable =
    name.endsWith('.csv') ||
    name.endsWith('.xlsx') ||
    file.type === 'text/csv' ||
    file.type === XLSX_TYPE ||
    file.type === 'application/vnd.ms-excel' ||
    file.type === ''
  const described = { name: file.name, size: formatFileSize(file.size) }
  const detail = `${described.name} · ${described.size}`
  errorMessage.value = ''
  if (!looksImportable) {
    uploadError.value = { message: t('import-error-not-csv', { name: file.name }), detail }
    return
  }
  uploadError.value = null
  busy.value = 'upload'
  reading.value = described
  try {
    const uploaded = await importService.upload(selectedType.value, file)
    job.value = uploaded
    rememberType(uploaded.job_type)
    setJobQuery(uploaded.id)
    step.value = 'map'
  } catch (e) {
    uploadError.value = { message: apiError(e, 'import-error-generic'), detail }
  } finally {
    busy.value = null
    reading.value = null
  }
}

function onDragOver(event: DragEvent) {
  event.preventDefault()
  if (event.dataTransfer) event.dataTransfer.dropEffect = busy.value ? 'none' : 'copy'
  isDragOver.value = !busy.value
}

function onDrop(event: DragEvent) {
  event.preventDefault()
  isDragOver.value = false
  const file = event.dataTransfer?.files?.[0]
  if (file) acceptFile(file)
}

// A new type clears a refusal that may have been about the old one.
watch(selectedType, () => {
  if (step.value === 'start') uploadError.value = null
})

// ---- map + review ----

/** Read the workbook again from another sheet or header row. */
async function chooseSource(sheet: string, headerRow: number | null) {
  if (!job.value) return
  busy.value = 'source'
  errorMessage.value = ''
  try {
    job.value = await importService.putSource(job.value.id, sheet, headerRow)
    step.value = 'map'
  } catch (e) {
    stepFailed(e, 'import-error-generic')
  } finally {
    busy.value = null
  }
}

/** Remounts the Map step when the columns or the type change under it. */
const mapKey = computed(() => {
  const source = job.value?.source
  return `${job.value?.id}:${job.value?.job_type}:${source?.sheet ?? ''}:${source?.header_row ?? 1}`
})

/** Import the same file as another type: the detection line on Map. */
async function switchType(type: ImportJobType) {
  if (!job.value || busy.value) return
  busy.value = 'type'
  errorMessage.value = ''
  try {
    job.value = await importService.putType(job.value.id, type)
    selectedType.value = type
    rememberType(type)
    step.value = 'map'
  } catch (e) {
    errorMessage.value = apiError(e, 'import-error-generic')
  } finally {
    busy.value = null
  }
}

async function check(mapping: ColumnMapping, options: ImportOptions, ignored: number[]) {
  if (!job.value) return
  busy.value = 'check'
  errorMessage.value = ''
  commitNotice.value = ''
  try {
    const checked = await importService.putMapping(job.value.id, mapping, options, ignored)
    job.value = checked
    step.value = (await valuesNeeded(checked)) ? 'values' : 'review'
  } catch (e) {
    stepFailed(e, 'import-error-generic')
  } finally {
    busy.value = null
  }
}

/** Read the checked job's values into the cache and say whether Match
 *  values has something to ask. Values that can't be read don't hold the
 *  import up: Review shows the check either way. */
async function valuesNeeded(checked: ImportJob): Promise<boolean> {
  try {
    const view = await importService.values(checked.id)
    queryCache.setQueryData(importValuesQueryKey(checked.id, checked.updated_at), view)
    return needsValues(view)
  } catch {
    return false
  }
}

/** "Check again" on Match values: store the answers and check the rows. */
async function checkValues(answers: ValueAnswers) {
  if (!job.value) return
  busy.value = 'check'
  errorMessage.value = ''
  commitNotice.value = ''
  try {
    job.value = await importService.putValues(job.value.id, answers)
    step.value = 'review'
  } catch (e) {
    stepFailed(e, 'import-error-generic')
  } finally {
    busy.value = null
  }
}

/** How often, and for how long, a commit that outlived its request is
 *  checked on. */
const COMMIT_POLL_MS = 2000
const COMMIT_POLL_LIMIT = 300

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

/** A commit request that ended without an answer from the server (a
 *  timeout, a dropped connection, a proxy giving up) may still be running. */
function mayStillRun(e: unknown): boolean {
  const status = (e as { response?: { status?: number } })?.response?.status
  return status === undefined || status === 502 || status === 503 || status === 504
}

/** Wait for a commit the server is still running: check the job every two
 *  seconds until it leaves `committing`, then show where it ended. */
async function waitForCommit(id: string) {
  commitNotice.value = t('import-commit-still-running')
  for (let i = 0; i < COMMIT_POLL_LIMIT; i++) {
    await sleep(COMMIT_POLL_MS)
    let found: ImportJob
    try {
      found = await importService.get(id)
    } catch {
      continue
    }
    if (found.status === 'committing') continue
    commitNotice.value = ''
    job.value = found
    step.value = stepForStatus(found.status)
    if (found.status === 'failed') {
      errorMessage.value = found.error_message ?? t('import-error-commit')
    } else if (found.status === 'dry_run_done') {
      errorMessage.value = t('import-commit-not-finished')
    }
    return
  }
  commitNotice.value = ''
  errorMessage.value = t('import-commit-still-running-later')
}

async function commit() {
  if (!job.value) return
  busy.value = 'commit'
  errorMessage.value = ''
  commitNotice.value = ''
  const id = job.value.id
  try {
    job.value = await importService.commit(id)
    step.value = 'done'
    // The commit saved this import's answers, and Recent imports has it.
    queryCache.invalidateQueries({ key: ['import-answers'] })
    queryCache.invalidateQueries({ key: ['import-jobs'] })
  } catch (e) {
    const data = (e as { response?: { data?: { code?: string; job?: ImportJob } } })?.response
      ?.data
    if (data?.code === 'import_plan_changed' && data.job) {
      // Nothing was imported; the job now holds the plan as it stands.
      const more = planDelta(job.value?.summary ?? null, data.job.summary)
      job.value = data.job
      commitNotice.value =
        more > 0
          ? t('import-review-plan-changed-more', { count: more })
          : more < 0
            ? t('import-review-plan-changed-fewer', { count: -more })
            : t('import-review-plan-changed')
    } else if (data?.code === 'import_commit_running' || mayStillRun(e)) {
      await waitForCommit(id)
    } else {
      stepFailed(e, 'import-error-commit')
    }
  } finally {
    busy.value = null
  }
}

/** Save the error report: the skipped rows in the file's own layout, or
 *  only one problem's rows, or the rows that leave values out. */
async function downloadReport(only?: { group?: number; dropped?: number[] }) {
  if (!job.value || downloadingReport.value) return
  downloadingReport.value = true
  errorMessage.value = ''
  try {
    const { blob, filename } = only
      ? await importService.errorReport(job.value.id, only)
      : await importService.errorReport(job.value.id)
    const link = document.createElement('a')
    link.href = URL.createObjectURL(blob)
    link.download = filename
    document.body.appendChild(link)
    link.click()
    document.body.removeChild(link)
    URL.revokeObjectURL(link.href)
  } catch (e) {
    errorMessage.value = apiError(e, 'import-error-report-failed')
  } finally {
    downloadingReport.value = false
  }
}

/** Back to an empty Start, keeping the type. */
function resetToStart() {
  if (job.value) selectedType.value = job.value.job_type
  job.value = null
  setJobQuery(null)
  uploadError.value = null
  errorMessage.value = ''
  commitNotice.value = ''
  step.value = 'start'
}

/** "Change file" on Map: delete the draft and its file, then Start. */
async function changeFile() {
  if (!job.value || busy.value) return
  busy.value = 'discard'
  errorMessage.value = ''
  try {
    await importService.remove(job.value.id)
  } catch (e) {
    // Already gone is as good as deleted.
    if ((e as { response?: { status?: number } })?.response?.status !== 404) {
      errorMessage.value = apiError(e, 'import-error-generic')
      return
    }
  } finally {
    busy.value = null
  }
  resetToStart()
}

/** Done's "Import another file like this": Start on the same type with
 *  the drop zone focused; the imported job stays, and the next file's Map
 *  step matches its columns from this one. */
async function importAnother() {
  resetToStart()
  await nextTick()
  dropZone.value?.focus()
}

/** Completed steps the job lets the admin go back to. */
function canGoTo(target: Step): boolean {
  if (busy.value) return false
  if (target === 'map') return step.value === 'review' || step.value === 'values'
  if (target === 'values') return step.value === 'review' && valuesStepShown.value
  return false
}

/** Review's "See them": Match values with every value matched. */
function seeValues() {
  fixValues(null)
}

/** Review's "Fix at Match values": Match values, at the field's card. */
function fixValues(field: string | null) {
  if (busy.value) return
  errorMessage.value = ''
  commitNotice.value = ''
  valuesFocus.value = field
  step.value = 'values'
}

function goTo(target: Step) {
  if (!canGoTo(target)) return
  errorMessage.value = ''
  commitNotice.value = ''
  if (target === 'values') valuesFocus.value = null
  step.value = target
}

// ---- done ----

const doneOutcomes = computed(() => (job.value?.summary ? outcomes(job.value.summary) : null))
const headlineOnDone = computed(() => (job.value ? doneHeadline(t, job.value) : ''))
const reportOnDone = computed(() => !!job.value && reportOffered(job.value))

/** The records the import touched: the asset list filtered to them; users
 *  and tickets open their lists. */
function viewImported() {
  switch (job.value?.job_type) {
    case 'users':
      router.push('/admin/users')
      return
    case 'tickets':
      router.push('/tickets')
      return
    default:
      router.push({ path: '/assets', query: { import: job.value?.id } })
  }
}
</script>

<template>
  <div class="flex-1">
    <div class="pt-4 px-4 sm:px-6 flex justify-between items-center">
      <BackButton fallbackRoute="/admin/data-import" :label="$t('csv-import-back')" />
    </div>

    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-4xl">
      <div class="flex flex-col gap-2">
        <h1 class="text-2xl font-bold text-primary">{{ $t('import-title') }}</h1>
        <p class="text-secondary">{{ $t('import-subtitle') }}</p>
      </div>

      <!-- Stepper. Completed steps are buttons when the job allows going back. -->
      <nav :aria-label="$t('import-stepper-label')" class="flex flex-col gap-2">
        <ol class="flex items-center gap-2 sm:gap-3 text-sm">
          <li v-for="(s, i) in STEPS" :key="s" class="flex items-center gap-2">
            <component
              :is="canGoTo(s) ? 'button' : 'span'"
              :type="canGoTo(s) ? 'button' : undefined"
              class="flex items-center gap-2"
              :aria-current="step === s ? 'step' : undefined"
              :data-step="s"
              @click="goTo(s)"
            >
              <span
                class="w-7 h-7 rounded-full flex items-center justify-center font-medium border"
                :class="step === s
                  ? 'bg-accent text-on-accent border-accent'
                  : i < stepIndex
                    ? 'bg-status-success text-on-accent border-status-success'
                    : 'bg-surface-alt text-secondary border-default'"
              >
                <Icon v-if="i < stepIndex" name="check" size="sm" />
                <template v-else>{{ i + 1 }}</template>
              </span>
              <span
                class="hidden sm:inline"
                :class="[
                  step === s ? 'text-primary font-medium' : 'text-secondary',
                  canGoTo(s) ? 'hover:underline' : '',
                ]"
              >
                {{ $t(STEP_KEYS[s]) }}
              </span>
            </component>
            <span v-if="i < STEPS.length - 1" class="w-3 sm:w-6 h-px bg-default" />
          </li>
        </ol>
        <p class="sm:hidden text-xs text-secondary">
          {{ $t('import-step-progress', { current: stepIndex + 1, total: STEPS.length, name: $t(STEP_KEYS[step]) }) }}
        </p>
      </nav>

      <ErrorBanner v-if="errorMessage" :message="errorMessage" />

      <Callout v-if="commitNotice" severity="warning" data-test="import-commit-notice">
        <p class="px-4 py-3 text-sm text-secondary">{{ commitNotice }}</p>
      </Callout>

      <p v-if="busy === 'resume'" class="flex items-center gap-2 text-sm text-secondary">
        <Spinner size="sm" />
        {{ $t('import-resuming') }}
      </p>

      <!-- Start ---------------------------------------------------------- -->
      <section v-else-if="step === 'start'" class="flex flex-col gap-6">
        <SectionCard content-padding="p-4">
          <template #title>{{ $t('import-start-heading') }}</template>
          <div class="flex flex-col gap-4">
            <div class="flex flex-col gap-2">
              <span class="text-sm font-medium text-primary">{{ $t('import-start-type-heading') }}</span>
              <div>
                <SegmentedControl
                  v-model="selectedType"
                  :options="typeOptions"
                  :aria-label="$t('import-start-type-label')"
                />
              </div>
            </div>

            <ErrorBanner
              v-if="uploadError"
              :message="uploadError.message"
              :detail="uploadError.detail"
              role="alert"
              data-test="import-upload-error"
            />

            <!-- Reading: the file is uploading and being parsed. -->
            <div
              v-if="reading"
              class="flex flex-col items-center justify-center gap-3 border-2 border-dashed border-default rounded-xl px-4 py-10 min-h-[176px] text-center bg-surface-alt/60"
              aria-busy="true"
              aria-live="polite"
              data-test="import-reading"
            >
              <Spinner size="md" />
              <div class="flex flex-col items-center gap-1 min-w-0 max-w-full">
                <p class="text-sm text-primary font-medium">{{ $t('import-start-reading') }}</p>
                <p class="text-xs text-tertiary break-all">{{ reading.name }} · {{ reading.size }}</p>
              </div>
            </div>

            <!-- Drop zone: a button, so a keyboard or a tap opens the
                 file picker; dropping a file uploads it at once. -->
            <button
              v-else
              ref="dropZone"
              type="button"
              class="w-full flex flex-col items-center justify-center gap-3 border-2 border-dashed rounded-xl px-4 py-10 min-h-[176px] text-center cursor-pointer transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              :class="isDragOver
                ? 'border-accent bg-accent/10'
                : 'border-default bg-surface-alt/60 hover:border-strong hover:bg-surface-alt'"
              data-test="import-drop"
              @click="chooseFile"
              @dragover="onDragOver"
              @dragleave="isDragOver = false"
              @drop="onDrop"
            >
              <span
                class="w-12 h-12 rounded-full flex items-center justify-center"
                :class="isDragOver ? 'bg-accent/20 text-accent' : 'bg-surface text-tertiary'"
              >
                <Icon name="document" size="md" />
              </span>
              <span class="flex flex-col items-center gap-1">
                <span class="text-sm text-primary font-medium">
                  {{ isDragOver ? $t('import-drop-release') : $t('import-drop-idle') }}
                </span>
                <span class="text-xs text-tertiary">{{ $t('import-drop-hint') }}</span>
              </span>
            </button>
            <input
              ref="fileInput"
              type="file"
              accept=".csv,text/csv,.xlsx,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
              class="hidden"
              tabindex="-1"
              aria-hidden="true"
              data-test="import-file"
              @change="pickFile"
            />
          </div>
        </SectionCard>

        <SectionCard content-padding="p-4">
          <template #title>{{ $t('import-template-heading') }}</template>
          <div class="flex flex-col gap-3">
            <p class="text-sm text-secondary">{{ $t('import-template-help') }}</p>
            <p v-if="selectedType === 'assets'" class="text-xs text-tertiary">
              {{ $t('import-template-kind-help') }}
            </p>
            <div class="flex flex-col gap-3 sm:flex-row sm:items-end">
              <div v-if="selectedType === 'assets'" class="w-full sm:w-64" data-test="import-template-kind">
                <BaseDropdown
                  v-model="templateKind"
                  :options="templateKindOptions"
                  :label="$t('import-template-kind-label')"
                  :placeholder="$t('import-template-kind-none')"
                />
              </div>
              <div>
                <Button variant="secondary" icon="download" @click="downloadTemplate">
                  {{ $t('import-template-download') }}
                </Button>
              </div>
            </div>
          </div>
        </SectionCard>

        <p class="text-sm">
          <RouterLink :class="linkClass" to="/admin/data-import" data-test="import-recent-link">
            {{ $t('import-history-title') }}
          </RouterLink>
        </p>
      </section>

      <!-- Map ------------------------------------------------------------ -->
      <template v-else-if="step === 'map' && job?.source">
        <ImportSheetPanel
          v-if="job.source.format === 'xlsx'"
          :key="job.id"
          :source="job.source"
          :busy="busy === 'source'"
          @choose="chooseSource"
        />
        <ImportMapStep
          v-if="fields.length > 0"
          :key="mapKey"
          :job="job"
          :fields="fields"
          :kinds="kindOptions"
          :checking="busy === 'check'"
          :switching="busy === 'type'"
          :discarding="busy === 'discard'"
          @check="check"
          @switch-type="switchType"
          @change-file="changeFile"
        />
        <p v-else class="flex items-center gap-2 text-sm text-secondary">
          <Spinner size="sm" />
          {{ $t('import-loading-fields') }}
        </p>
      </template>

      <!-- Match values --------------------------------------------------- -->
      <template v-else-if="step === 'values' && job">
        <ImportValuesStep
          v-if="valuesView && fields.length > 0"
          :key="job.id"
          :view="valuesView"
          :fields="fields"
          :checking="busy === 'check'"
          :job-id="job.id"
          :version="job.updated_at"
          :focus-field="valuesFocus"
          :job-type="job.job_type"
          @check="checkValues"
          @back="goTo('map')"
        />
        <ErrorBanner v-else-if="valuesQuery.error.value" :message="$t('import-values-load-failed')" />
        <p v-else class="flex items-center gap-2 text-sm text-secondary">
          <Spinner size="sm" />
          {{ $t('import-values-loading') }}
        </p>
      </template>

      <!-- Review --------------------------------------------------------- -->
      <ImportReviewStep
        v-else-if="step === 'review' && job?.summary"
        :job="job"
        :fields="fields"
        :kinds="kindOptions"
        :committing="busy === 'commit'"
        :downloading="downloadingReport"
        :values-all-matched="valuesAllMatched"
        :webhooks="webhooksHearAssets"
        @back="goTo('map')"
        @see-values="seeValues"
        @fix-values="fixValues"
        @fix-map="goTo('map')"
        @commit="commit"
        @download-report="downloadReport"
      />

      <!-- Done ----------------------------------------------------------- -->
      <section v-else-if="step === 'done' && job" class="flex flex-col gap-6" data-test="import-done">
        <Callout severity="success">
          <template #header>
            <span class="font-medium text-primary">{{ $t('import-done-heading') }}</span>
          </template>
          <div class="px-4 py-3 flex flex-col gap-1">
            <p class="text-sm text-primary" data-test="import-done-headline">{{ headlineOnDone }}</p>
            <p v-if="(doneOutcomes?.skipped ?? 0) > 0" class="text-sm text-secondary" data-test="import-done-fix-hint">
              {{ $t('import-done-fix-hint') }}
            </p>
            <p
              v-if="job.job_type === 'assets' && (doneOutcomes?.created ?? 0) > 0"
              class="text-xs text-tertiary"
              data-test="import-done-imported-event"
            >
              {{ $t('import-done-imported-event') }}
            </p>
          </div>
        </Callout>

        <!-- A13: for users, "These 7 people can't sign in yet." with Send
             invites or the SSO wording goes here. -->

        <ImportReviewStep
          v-if="job.summary"
          :job="job"
          :fields="fields"
          :kinds="kindOptions"
          mode="done"
          @download-report="downloadReport"
        />

        <div class="flex flex-wrap items-center gap-2" data-test="import-done-actions">
          <Button data-test="import-done-view" @click="viewImported">
            {{ $t('import-done-view', { type: job.job_type }) }}
          </Button>
          <Button
            v-if="reportOnDone"
            variant="secondary"
            icon="download"
            :loading="downloadingReport"
            data-test="import-done-report"
            @click="downloadReport()"
          >
            {{ $t('import-review-download-report') }}
          </Button>
          <!-- A12: "Import the next sheet: {sheet}" goes here. -->
          <Button variant="secondary" data-test="import-done-another" @click="importAnother">
            {{ $t('import-done-another') }}
          </Button>
        </div>

        <!-- A10: "Remove imported assets" with its window goes here. -->

        <p class="text-sm">
          <RouterLink :class="linkClass" to="/admin/data-import" data-test="import-done-recent">
            {{ $t('import-history-title') }}
          </RouterLink>
        </p>
      </section>
    </div>
  </div>
</template>
