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

const isWorking = ref(false)
const errorMessage = ref('')

const job = ref<ImportJob | null>(null)
const summary = computed(() => job.value?.summary ?? null)

function pickFile(event: Event) {
  const input = event.target as HTMLInputElement
  if (!input.files || input.files.length === 0) return
  selectedFile.value = input.files[0]
  errorMessage.value = ''
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
            <input
              ref="fileInput"
              type="file"
              accept=".csv,text/csv"
              class="text-sm text-secondary file:mr-3 file:py-1.5 file:px-3 file:rounded-lg file:border file:border-default file:bg-surface-alt file:text-primary file:cursor-pointer hover:file:border-strong"
              @change="pickFile"
            />
            <p v-if="selectedFile" class="text-xs text-tertiary">
              {{ selectedFile.name }} ({{ Math.round(selectedFile.size / 1024) }} KB)
            </p>
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
        <div
          v-if="summary.row_count === 0"
          class="bg-status-warning/10 border border-status-warning/40 rounded-lg p-4 text-sm text-status-warning"
        >
          {{ $t('csv-import-empty-file') }}
        </div>

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

        <div v-if="summary.errors.length > 0" class="bg-surface border border-default rounded-lg overflow-hidden">
          <div class="px-4 py-3 bg-status-error/10 border-b border-status-error/40 flex items-center gap-2">
            <Icon name="warning" class="text-status-error" />
            <span class="text-sm font-medium text-status-error">
              {{ $t('csv-import-errors-heading', { count: summary.errors.length }) }}
              <span v-if="summary.errors_truncated" class="text-tertiary font-normal">
                ({{ $t('csv-import-errors-truncated') }})
              </span>
            </span>
          </div>
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
                <td class="px-4 py-2 font-mono text-secondary">{{ e.column ?? '—' }}</td>
                <td class="px-4 py-2">{{ e.message }}</td>
              </tr>
            </tbody>
          </table>
        </div>

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
