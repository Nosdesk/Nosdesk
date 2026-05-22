<script setup lang="ts">
/**
 * Three-step CSV bulk import wizard.
 *
 *  - Upload    : pick a type, download the template, upload the
 *                file. The backend parses + validates and the
 *                response is a job row with status=dry_run_done
 *                and a populated `summary`.
 *  - Review    : render the summary cards (will-create /
 *                will-update / error count) and per-row errors.
 *                Admin clicks Apply to commit or Discard to
 *                start over.
 *  - Done      : final card with records_committed and a New
 *                Import button to reset.
 *
 * Assets is the only type that has a working backend in Phase 1.
 * Users and tickets render as disabled "coming soon" choices so
 * the UI surface is in place when their parsers ship.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import { useRouter } from 'vue-router'

import BackButton from '@/components/common/BackButton.vue'
import Callout from '@/components/common/Callout.vue'
import Icon from '@/components/common/Icon.vue'
import Spinner from '@/components/common/Spinner.vue'

import {
  importService,
  type ImportJob,
  type ImportJobType,
} from '@/services/importService'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const router = useRouter()

type Step = 'upload' | 'review' | 'done'
const step = ref<Step>('upload')

const supportedTypes: { value: ImportJobType; available: boolean }[] = [
  { value: 'assets', available: true },
  { value: 'users', available: true },
  { value: 'tickets', available: true },
]

const selectedType = ref<ImportJobType>('assets')
const selectedFile = ref<File | null>(null)
const fileInput = ref<HTMLInputElement | null>(null)
const isDragOver = ref(false)

const isWorking = ref(false)
const errorMessage = ref('')

const job = ref<ImportJob | null>(null)
const summary = computed(() => job.value?.summary ?? null)

function pickFile(event: Event) {
  const input = event.target as HTMLInputElement
  if (!input.files || input.files.length === 0) return
  acceptFile(input.files[0])
}

function acceptFile(file: File) {
  // Lightly gate on extension/type so the user finds out
  // immediately rather than at validate-time. The backend
  // re-checks anyway.
  const looksLikeCsv =
    file.name.toLowerCase().endsWith('.csv') ||
    file.type === 'text/csv' ||
    file.type === 'application/vnd.ms-excel' ||
    file.type === ''
  if (!looksLikeCsv) {
    errorMessage.value = t('csv-import-error-not-csv', { name: file.name })
    return
  }
  selectedFile.value = file
  errorMessage.value = ''
}

function triggerFilePicker() {
  fileInput.value?.click()
}

function onDragOver(event: DragEvent) {
  event.preventDefault()
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'copy'
  isDragOver.value = true
}

function onDragLeave() {
  isDragOver.value = false
}

function onDrop(event: DragEvent) {
  event.preventDefault()
  isDragOver.value = false
  const file = event.dataTransfer?.files?.[0]
  if (file) acceptFile(file)
}

function downloadTemplate() {
  // Trigger a browser download via an anchor; apiClient would
  // buffer the response which means we'd have to decode + save
  // manually. The /api prefix makes this go through the same
  // cookie auth as the API.
  const url = importService.templateUrl(selectedType.value)
  window.location.href = url
}

async function uploadAndDryRun() {
  if (!selectedFile.value) return
  isWorking.value = true
  errorMessage.value = ''
  try {
    const result = await importService.upload(selectedType.value, selectedFile.value)
    job.value = result
    step.value = 'review'
  } catch (e) {
    const err = e as { response?: { data?: { error?: string } } }
    errorMessage.value =
      err?.response?.data?.error ?? (e instanceof Error ? e.message : t('csv-import-error-generic'))
  } finally {
    isWorking.value = false
  }
}

async function applyImport() {
  if (!job.value) return
  isWorking.value = true
  errorMessage.value = ''
  try {
    const result = await importService.commit(job.value.id)
    job.value = result
    step.value = 'done'
  } catch (e) {
    const err = e as { response?: { data?: { error?: string } } }
    errorMessage.value =
      err?.response?.data?.error ??
      (e instanceof Error ? e.message : t('csv-import-error-commit-failed'))
  } finally {
    isWorking.value = false
  }
}

function startOver() {
  step.value = 'upload'
  selectedFile.value = null
  if (fileInput.value) fileInput.value.value = ''
  job.value = null
  errorMessage.value = ''
}

function viewImported() {
  if (!job.value) return
  switch (job.value.job_type) {
    case 'assets':
      router.push('/assets')
      return
    case 'users':
      router.push('/admin/users')
      return
    case 'tickets':
      router.push('/tickets')
      return
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
    <div class="pt-4 px-6 flex justify-between items-center">
      <BackButton fallbackRoute="/admin/data-import" :label="$t('csv-import-back')" />
    </div>

    <div class="flex flex-col gap-6 px-6 py-4 mx-auto w-full max-w-4xl">
      <div>
        <h1 class="text-2xl font-bold text-primary">{{ $t('csv-import-title') }}</h1>
        <p class="text-secondary mt-2">{{ $t('csv-import-subtitle') }}</p>
      </div>

      <!-- Step indicator -->
      <ol class="flex items-center gap-3 text-sm">
        <li
          v-for="(s, i) in (['upload', 'review', 'done'] as Step[])"
          :key="s"
          class="flex items-center gap-2"
        >
          <span
            class="w-7 h-7 rounded-full flex items-center justify-center font-medium border"
            :class="step === s
              ? 'bg-accent text-on-accent border-accent'
              : i < (['upload', 'review', 'done'] as Step[]).indexOf(step)
                ? 'bg-status-success text-on-accent border-status-success'
                : 'bg-surface-alt text-secondary border-default'"
          >
            {{ i + 1 }}
          </span>
          <span :class="step === s ? 'text-primary font-medium' : 'text-secondary'">
            {{ $t(`csv-import-step-${s}`) }}
          </span>
          <span v-if="i < 2" class="w-6 h-px bg-default mx-1" />
        </li>
      </ol>

      <p v-if="errorMessage" class="px-4 py-3 bg-status-error/10 border border-status-error/40 text-status-error rounded-lg text-sm">
        {{ errorMessage }}
      </p>

      <!-- Step 1: Upload ----------------------------------------------- -->
      <section v-if="step === 'upload'" class="flex flex-col gap-6">
        <div class="bg-surface border border-default rounded-lg p-6 flex flex-col gap-4">
          <h2 class="text-lg font-semibold text-primary">{{ $t('csv-import-step-upload-heading') }}</h2>

          <div class="flex flex-col gap-2">
            <label class="text-xs font-medium text-secondary uppercase tracking-wide">
              {{ $t('csv-import-type-label') }}
            </label>
            <div class="flex flex-wrap gap-2">
              <button
                v-for="opt in supportedTypes"
                :key="opt.value"
                type="button"
                :disabled="!opt.available"
                class="px-4 py-2 rounded-lg text-sm border transition-colors"
                :class="selectedType === opt.value && opt.available
                  ? 'bg-accent text-on-accent border-accent'
                  : opt.available
                    ? 'bg-surface-alt text-primary border-default hover:border-strong'
                    : 'bg-surface-alt text-tertiary border-default opacity-50 cursor-not-allowed'"
                @click="opt.available && (selectedType = opt.value)"
              >
                {{ $t(`csv-import-type-${opt.value}`) }}
                <span v-if="!opt.available" class="ml-1 text-xs">
                  ({{ $t('csv-import-type-coming-soon') }})
                </span>
              </button>
            </div>
          </div>

          <div class="flex flex-col gap-2">
            <label class="text-xs font-medium text-secondary uppercase tracking-wide">
              {{ $t('csv-import-template-label') }}
            </label>
            <p class="text-sm text-secondary">{{ $t('csv-import-template-help') }}</p>
            <button
              type="button"
              class="self-start inline-flex items-center gap-2 px-3 py-1.5 text-sm rounded-lg border border-default hover:border-strong text-primary"
              @click="downloadTemplate"
            >
              <Icon name="download" />
              {{ $t('csv-import-template-button') }}
            </button>
          </div>

          <div class="flex flex-col gap-2">
            <label class="text-xs font-medium text-secondary uppercase tracking-wide">
              {{ $t('csv-import-file-label') }}
            </label>
            <!--
              Drop zone. Clicking anywhere on the card opens the
              file picker; dragging a file over highlights the
              border and dropping accepts the first file in the
              data transfer. The hidden <input> below is what
              the click handler triggers — keeping the input in
              the DOM (rather than synthesising one) means the
              native file dialog opens with the right `accept`
              filter.
            -->
            <div
              class="flex flex-col items-center justify-center gap-3 border-2 border-dashed rounded-xl px-6 py-12 min-h-[200px] text-center cursor-pointer transition-colors"
              :class="isDragOver
                ? 'border-accent bg-accent/10'
                : 'border-default bg-surface-alt/60 hover:border-strong hover:bg-surface-alt'"
              role="button"
              tabindex="0"
              @click="triggerFilePicker"
              @keydown.enter.prevent="triggerFilePicker"
              @keydown.space.prevent="triggerFilePicker"
              @dragover="onDragOver"
              @dragleave="onDragLeave"
              @drop="onDrop"
            >
              <div
                class="w-12 h-12 rounded-full flex items-center justify-center"
                :class="isDragOver
                  ? 'bg-accent/20 text-accent'
                  : 'bg-surface text-tertiary'"
              >
                <Icon name="document" size="md" />
              </div>
              <div class="flex flex-col items-center gap-1">
                <p v-if="!selectedFile" class="text-sm text-primary font-medium">
                  {{ isDragOver ? $t('csv-import-drop-here') : $t('csv-import-drop-zone-idle') }}
                </p>
                <p v-else class="text-sm text-primary">
                  <span class="font-medium">{{ selectedFile.name }}</span>
                  <span class="text-tertiary"> · {{ Math.round(selectedFile.size / 1024) }} KB</span>
                </p>
                <p v-if="!selectedFile" class="text-xs text-tertiary">
                  {{ $t('csv-import-drop-zone-hint') }}
                </p>
                <p v-else class="text-xs text-tertiary">
                  {{ $t('csv-import-drop-zone-replace') }}
                </p>
              </div>
            </div>
            <input
              ref="fileInput"
              type="file"
              accept=".csv,text/csv"
              class="hidden"
              @change="pickFile"
            />
          </div>

          <div class="flex justify-end gap-2 pt-2">
            <button
              type="button"
              :disabled="!selectedFile || isWorking"
              class="px-4 py-2 rounded-lg bg-accent text-on-accent hover:bg-accent-strong disabled:opacity-50 disabled:cursor-not-allowed inline-flex items-center gap-2"
              @click="uploadAndDryRun"
            >
              <Spinner v-if="isWorking" class="text-on-accent" />
              {{ $t('csv-import-action-validate') }}
            </button>
          </div>
        </div>
      </section>

      <!-- Step 2: Review ----------------------------------------------- -->
      <section v-if="step === 'review' && summary" class="flex flex-col gap-6">
        <Callout v-if="summary.row_count === 0" severity="warning">
          <template #header>
            <span class="text-primary">{{ $t('csv-import-empty-file') }}</span>
          </template>
        </Callout>

        <div class="grid grid-cols-1 sm:grid-cols-3 gap-3">
          <div class="bg-surface border border-default rounded-lg p-4">
            <p class="text-xs text-tertiary uppercase tracking-wide">{{ $t('csv-import-summary-rows') }}</p>
            <p class="text-2xl font-semibold text-primary mt-1">{{ summary.row_count }}</p>
          </div>
          <div class="bg-surface border border-default rounded-lg p-4">
            <p class="text-xs text-tertiary uppercase tracking-wide">{{ $t('csv-import-summary-create') }}</p>
            <p class="text-2xl font-semibold text-status-success mt-1">{{ summary.would_create }}</p>
          </div>
          <div class="bg-surface border border-default rounded-lg p-4">
            <p class="text-xs text-tertiary uppercase tracking-wide">{{ $t('csv-import-summary-update') }}</p>
            <p class="text-2xl font-semibold text-accent mt-1">{{ summary.would_update }}</p>
          </div>
        </div>

        <Callout v-if="summary.errors.length > 0" severity="error">
          <template #header>
            <span class="font-medium text-primary">
              {{ $t('csv-import-errors-heading', { count: summary.errors.length }) }}
            </span>
            <span v-if="summary.errors_truncated" class="text-tertiary ml-1">
              ({{ $t('csv-import-errors-truncated') }})
            </span>
          </template>
          <table class="w-full text-sm">
            <thead class="bg-surface-alt">
              <tr class="text-left text-tertiary text-xs uppercase tracking-wide">
                <th class="px-4 py-2 w-20">{{ $t('csv-import-errors-row') }}</th>
                <th class="px-4 py-2 w-40">{{ $t('csv-import-errors-column') }}</th>
                <th class="px-4 py-2">{{ $t('csv-import-errors-message') }}</th>
              </tr>
            </thead>
            <tbody class="divide-y divide-default">
              <tr v-for="(e, i) in summary.errors" :key="i" class="text-primary">
                <td class="px-4 py-2 font-mono text-tertiary">{{ e.row }}</td>
                <td class="px-4 py-2 font-mono text-secondary">{{ e.column ?? '-' }}</td>
                <td class="px-4 py-2">{{ e.message }}</td>
              </tr>
            </tbody>
          </table>
        </Callout>

        <div class="flex justify-end gap-2">
          <button
            type="button"
            class="px-4 py-2 rounded-lg border border-default text-secondary hover:text-primary"
            @click="startOver"
          >
            {{ $t('csv-import-action-discard') }}
          </button>
          <button
            type="button"
            :disabled="isWorking || summary.would_create + summary.would_update === 0"
            class="px-4 py-2 rounded-lg bg-accent text-on-accent hover:bg-accent-strong disabled:opacity-50 disabled:cursor-not-allowed inline-flex items-center gap-2"
            @click="applyImport"
          >
            <Spinner v-if="isWorking" class="text-on-accent" />
            {{ $t('csv-import-action-apply', {
              count: summary.would_create + summary.would_update,
            }) }}
          </button>
        </div>
      </section>

      <!-- Step 3: Done ------------------------------------------------- -->
      <section v-if="step === 'done' && job" class="flex flex-col gap-6">
        <div class="bg-status-success/10 border border-status-success/40 rounded-lg p-6 flex items-start gap-4">
          <Icon name="check" class="text-status-success mt-1" size="md" />
          <div class="flex-1">
            <h2 class="text-lg font-semibold text-status-success">{{ $t('csv-import-done-heading') }}</h2>
            <p class="text-sm text-secondary mt-1">
              {{ $t('csv-import-done-body', { count: job.records_committed ?? 0 }) }}
            </p>
          </div>
        </div>
        <div class="flex justify-end gap-2">
          <button
            type="button"
            class="px-4 py-2 rounded-lg border border-default text-secondary hover:text-primary"
            @click="startOver"
          >
            {{ $t('csv-import-action-new') }}
          </button>
          <button
            type="button"
            class="px-4 py-2 rounded-lg bg-accent text-on-accent hover:bg-accent-strong"
            @click="viewImported"
          >
            {{ $t(viewImportedLabelKey) }}
          </button>
        </div>
      </section>
    </div>
  </div>
</template>
