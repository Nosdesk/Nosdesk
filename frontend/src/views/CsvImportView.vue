<script setup lang="ts">
/**
 * Bulk import wizard: Start, Upload, Map, Review, Done.
 *
 *  - Start  : pick the type, download the template.
 *  - Upload : the server parses the .csv or .xlsx and answers with the
 *             job in `parsed`: the file's columns and a suggested mapping.
 *  - Map    : for a workbook, the sheet panel picks the sheet and header
 *             row (`PUT source`, back to `parsed`); then the admin checks
 *             the mapping and options, and "Check" stores them and runs
 *             the dry run (`dry_run_done`).
 *  - Review : what the dry run found, row by row, and the error report;
 *             Import commits.
 *  - Done   : the committed counts and the report of skipped rows.
 *
 * The job id rides in `?job=` so a reload or a link resumes at the step
 * the job's status implies.
 */
import { computed, onMounted, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import { useRoute, useRouter } from 'vue-router'
import { useQuery } from '@pinia/colada'

import BackButton from '@/components/common/BackButton.vue'
import Button from '@/components/common/Button.vue'
import Callout from '@/components/common/Callout.vue'
import ErrorBanner from '@/components/common/ErrorBanner.vue'
import Icon from '@/components/common/Icon.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import Spinner from '@/components/common/Spinner.vue'
import ImportMapStep from '@/components/import/ImportMapStep.vue'
import ImportReviewStep from '@/components/import/ImportReviewStep.vue'
import ImportSheetPanel from '@/components/import/ImportSheetPanel.vue'
import { useAssetKindsQuery } from '@/composables/useAssetKindsQuery'
import {
  importFieldsQueryKey,
  importService,
  type ColumnMapping,
  type ImportJob,
  type ImportJobType,
  type ImportOptions,
} from '@nosdesk/core/services/importService'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const route = useRoute()
const router = useRouter()

type Step = 'start' | 'upload' | 'map' | 'review' | 'done'
const STEPS: Step[] = ['start', 'upload', 'map', 'review', 'done']
const STEP_KEYS: Record<Step, string> = {
  start: 'import-step-start',
  upload: 'import-step-upload',
  map: 'import-step-map',
  review: 'import-step-review',
  done: 'import-step-done',
}

const step = ref<Step>('start')
const selectedType = ref<ImportJobType>('assets')
const selectedFile = ref<File | null>(null)
const fileInput = ref<HTMLInputElement | null>(null)
const isDragOver = ref(false)

const job = ref<ImportJob | null>(null)
const busy = ref<'upload' | 'source' | 'check' | 'commit' | 'resume' | null>(null)
const downloadingReport = ref(false)
const errorMessage = ref('')

const jobType = computed<ImportJobType>(() => job.value?.job_type ?? selectedType.value)

const fieldsQuery = useQuery({
  key: () => importFieldsQueryKey(jobType.value),
  query: () => importService.fields(jobType.value),
  enabled: () => step.value === 'map' || step.value === 'review',
})
const fields = computed(() => fieldsQuery.data.value ?? [])

const { kinds: assetKinds } = useAssetKindsQuery()
const kindOptions = computed(() =>
  assetKinds.value.map((k) => ({ value: k.slug, label: k.label })),
)

const typeOptions = computed(() => [
  { value: 'assets' as ImportJobType, label: t('csv-import-type-assets') },
  { value: 'users' as ImportJobType, label: t('csv-import-type-users') },
  { value: 'tickets' as ImportJobType, label: t('csv-import-type-tickets') },
])

const stepIndex = computed(() => STEPS.indexOf(step.value))

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
      return 'upload'
  }
}

async function resume(id: string) {
  busy.value = 'resume'
  try {
    const found = await importService.get(id)
    // A job checked before column mapping existed can't go on.
    if (stepForStatus(found.status) === 'map' || stepForStatus(found.status) === 'review') {
      if (!found.source) throw new Error('job predates column mapping')
    }
    job.value = found
    selectedType.value = found.job_type
    step.value = stepForStatus(found.status)
    if (found.status === 'failed') {
      errorMessage.value = found.error_message ?? t('import-error-generic')
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

// ---- start + upload ----

function downloadTemplate() {
  window.location.href = importService.templateUrl(selectedType.value)
}

function pickFile(event: Event) {
  const input = event.target as HTMLInputElement
  if (!input.files || input.files.length === 0) return
  acceptFile(input.files[0])
}

const XLSX_TYPE = 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet'

function acceptFile(file: File) {
  const name = file.name.toLowerCase()
  const looksImportable =
    name.endsWith('.csv') ||
    name.endsWith('.xlsx') ||
    file.type === 'text/csv' ||
    file.type === XLSX_TYPE ||
    file.type === 'application/vnd.ms-excel' ||
    file.type === ''
  if (!looksImportable) {
    errorMessage.value = t('import-error-not-csv', { name: file.name })
    return
  }
  selectedFile.value = file
  errorMessage.value = ''
}

function onDragOver(event: DragEvent) {
  event.preventDefault()
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'copy'
  isDragOver.value = true
}

function onDrop(event: DragEvent) {
  event.preventDefault()
  isDragOver.value = false
  const file = event.dataTransfer?.files?.[0]
  if (file) acceptFile(file)
}

async function upload() {
  if (!selectedFile.value) return
  busy.value = 'upload'
  errorMessage.value = ''
  try {
    const uploaded = await importService.upload(selectedType.value, selectedFile.value)
    job.value = uploaded
    setJobQuery(uploaded.id)
    step.value = 'map'
  } catch (e) {
    errorMessage.value = apiError(e, 'import-error-generic')
  } finally {
    busy.value = null
  }
}

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
    errorMessage.value = apiError(e, 'import-error-generic')
  } finally {
    busy.value = null
  }
}

/** Remounts the Map step when the columns change under it. */
const mapKey = computed(() => {
  const source = job.value?.source
  return `${job.value?.id}:${source?.sheet ?? ''}:${source?.header_row ?? 1}`
})

async function check(mapping: ColumnMapping, options: ImportOptions) {
  if (!job.value) return
  busy.value = 'check'
  errorMessage.value = ''
  try {
    job.value = await importService.putMapping(job.value.id, mapping, options)
    step.value = 'review'
  } catch (e) {
    errorMessage.value = apiError(e, 'import-error-generic')
  } finally {
    busy.value = null
  }
}

async function commit() {
  if (!job.value) return
  busy.value = 'commit'
  errorMessage.value = ''
  try {
    job.value = await importService.commit(job.value.id)
    step.value = 'done'
  } catch (e) {
    errorMessage.value = apiError(e, 'import-error-commit')
  } finally {
    busy.value = null
  }
}

/** Save the error report: the skipped rows in the file's own layout. */
async function downloadReport() {
  if (!job.value || downloadingReport.value) return
  downloadingReport.value = true
  errorMessage.value = ''
  try {
    const { blob, filename } = await importService.errorReport(job.value.id)
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

function changeFile() {
  job.value = null
  setJobQuery(null)
  selectedFile.value = null
  if (fileInput.value) fileInput.value.value = ''
  errorMessage.value = ''
  step.value = 'upload'
}

function startOver() {
  changeFile()
  step.value = 'start'
}

/** Completed steps the job lets the admin go back to. */
function canGoTo(target: Step): boolean {
  if (busy.value) return false
  if (target === 'start') return step.value === 'upload'
  if (target === 'map') return step.value === 'review'
  return false
}

function goTo(target: Step) {
  if (!canGoTo(target)) return
  errorMessage.value = ''
  step.value = target
}

// ---- done ----

const skippedOnDone = computed(() => {
  const s = job.value?.summary
  if (!s) return 0
  return (
    s.rows_with_errors ??
    Math.max(s.row_count - (job.value?.records_committed ?? 0) - (s.unchanged ?? 0), 0)
  )
})

function viewImported() {
  switch (job.value?.job_type) {
    case 'users':
      router.push('/admin/users')
      return
    case 'tickets':
      router.push('/tickets')
      return
    default:
      router.push('/assets')
  }
}

const viewImportedLabelKey = computed(() => {
  switch (job.value?.job_type) {
    case 'users':
      return 'csv-import-action-view-users'
    case 'tickets':
      return 'csv-import-action-view-tickets'
    default:
      return 'csv-import-action-view-assets'
  }
})
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

      <p v-if="busy === 'resume'" class="flex items-center gap-2 text-sm text-secondary">
        <Spinner size="sm" />
        {{ $t('import-resuming') }}
      </p>

      <!-- Start ---------------------------------------------------------- -->
      <section v-else-if="step === 'start'" class="flex flex-col gap-6">
        <SectionCard content-padding="p-4">
          <template #title>{{ $t('import-start-type-heading') }}</template>
          <SegmentedControl
            v-model="selectedType"
            :options="typeOptions"
            :aria-label="$t('import-start-type-label')"
          />
        </SectionCard>
        <SectionCard content-padding="p-4">
          <template #title>{{ $t('import-template-heading') }}</template>
          <div class="flex flex-col gap-3">
            <p class="text-sm text-secondary">{{ $t('import-template-help') }}</p>
            <div>
              <Button variant="secondary" icon="download" @click="downloadTemplate">
                {{ $t('import-template-download') }}
              </Button>
            </div>
          </div>
        </SectionCard>
        <div class="sticky bottom-0 bg-app border-t border-default py-3 flex flex-wrap items-center gap-3">
          <Button class="ml-auto" data-test="import-continue" @click="step = 'upload'">
            {{ $t('import-action-continue') }}
          </Button>
        </div>
      </section>

      <!-- Upload --------------------------------------------------------- -->
      <section v-else-if="step === 'upload'" class="flex flex-col gap-6">
        <SectionCard content-padding="p-4">
          <template #title>{{ $t('import-upload-heading') }}</template>
          <div
            class="flex flex-col items-center justify-center gap-3 border-2 border-dashed rounded-xl px-6 py-12 min-h-[200px] text-center cursor-pointer transition-colors"
            :class="isDragOver
              ? 'border-accent bg-accent/10'
              : 'border-default bg-surface-alt/60 hover:border-strong hover:bg-surface-alt'"
            role="button"
            tabindex="0"
            @click="fileInput?.click()"
            @keydown.enter.prevent="fileInput?.click()"
            @keydown.space.prevent="fileInput?.click()"
            @dragover="onDragOver"
            @dragleave="isDragOver = false"
            @drop="onDrop"
          >
            <div
              class="w-12 h-12 rounded-full flex items-center justify-center"
              :class="isDragOver ? 'bg-accent/20 text-accent' : 'bg-surface text-tertiary'"
            >
              <Icon name="document" size="md" />
            </div>
            <div class="flex flex-col items-center gap-1">
              <p v-if="!selectedFile" class="text-sm text-primary font-medium">
                {{ isDragOver ? $t('import-drop-release') : $t('import-drop-idle') }}
              </p>
              <p v-else class="text-sm text-primary">
                <span class="font-medium">{{ selectedFile.name }}</span>
                <span class="text-tertiary"> · {{ Math.max(1, Math.round(selectedFile.size / 1024)) }} KB</span>
              </p>
              <p class="text-xs text-tertiary">
                {{ selectedFile ? $t('import-drop-replace') : $t('import-drop-hint') }}
              </p>
            </div>
          </div>
          <input
            ref="fileInput"
            type="file"
            accept=".csv,text/csv,.xlsx,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            class="hidden"
            data-test="import-file"
            @change="pickFile"
          />
        </SectionCard>
        <div class="sticky bottom-0 bg-app border-t border-default py-3 flex flex-wrap items-center gap-3">
          <Button variant="ghost" @click="step = 'start'">{{ $t('import-action-back') }}</Button>
          <Button
            class="ml-auto"
            :disabled="!selectedFile"
            :loading="busy === 'upload'"
            data-test="import-upload"
            @click="upload"
          >
            {{ busy === 'upload' ? $t('import-action-uploading') : $t('import-action-upload') }}
          </Button>
        </div>
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
          @check="check"
          @change-file="changeFile"
        />
        <p v-else class="flex items-center gap-2 text-sm text-secondary">
          <Spinner size="sm" />
          {{ $t('import-loading-fields') }}
        </p>
      </template>

      <!-- Review --------------------------------------------------------- -->
      <ImportReviewStep
        v-else-if="step === 'review' && job?.summary"
        :job="job"
        :fields="fields"
        :committing="busy === 'commit'"
        :downloading="downloadingReport"
        @back="goTo('map')"
        @commit="commit"
        @download-report="downloadReport"
      />

      <!-- Done ----------------------------------------------------------- -->
      <section v-else-if="step === 'done' && job" class="flex flex-col gap-6">
        <Callout severity="success">
          <template #header>
            <span class="font-medium text-primary">{{ $t('import-done-heading') }}</span>
          </template>
          <p class="px-4 py-3 text-sm text-secondary">
            {{ $t('import-done-body', { count: job.records_committed ?? 0 }) }}
            <template v-if="skippedOnDone > 0">
              {{ $t('import-done-skipped', { count: skippedOnDone }) }}
            </template>
          </p>
        </Callout>
        <div class="sticky bottom-0 bg-app border-t border-default py-3 flex flex-wrap items-center gap-3">
          <Button variant="ghost" @click="startOver">{{ $t('import-done-new') }}</Button>
          <Button
            v-if="skippedOnDone > 0"
            class="ml-auto"
            variant="secondary"
            icon="download"
            :loading="downloadingReport"
            data-test="import-done-report"
            @click="downloadReport"
          >
            {{ $t('import-review-download-report') }}
          </Button>
          <Button :class="skippedOnDone > 0 ? '' : 'ml-auto'" @click="viewImported">{{ $t(viewImportedLabelKey) }}</Button>
        </div>
      </section>
    </div>
  </div>
</template>
