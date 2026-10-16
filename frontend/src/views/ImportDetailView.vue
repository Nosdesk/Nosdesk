<script setup lang="ts">
/**
 * A past import: who ran it and when, its options and read-only mapping,
 * then Done's cards (the four tiles, Not imported, Created in your
 * workspace, What changes) with every row's result and the error report.
 *
 * Thirty days after the import its file and per-row detail are removed:
 * the page then shows the counts, options and mapping, and says so. A
 * draft opens in the import wizard instead; one past its 30 days can't be
 * resumed and shows its state only.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useRoute, useRouter } from 'vue-router'
import { useQuery, useQueryCache } from '@pinia/colada'

import BackButton from '@/components/common/BackButton.vue'
import Button from '@/components/common/Button.vue'
import ErrorBanner from '@/components/common/ErrorBanner.vue'
import ImportRemoveBlock from '@/components/import/ImportRemoveBlock.vue'
import ImportReviewStep from '@/components/import/ImportReviewStep.vue'
import { useAssetKindsQuery } from '@/composables/useAssetKindsQuery'
import { formatCompactDate } from '@nosdesk/core/utils/dateUtils'
import {
  importFieldsQueryKey,
  importJobQueryKey,
  importService,
  type ImportJob,
} from '@nosdesk/core/services/importService'
import webhookService from '@nosdesk/core/services/webhookService'

import {
  STATE_KEY,
  detailsKept,
  fileLabel,
  optionsLine,
  reportOffered,
  resumeTarget,
} from '@/components/import/historyModel'
import { previousValuesHelp, removalState } from '@/components/import/removeModel'
import { webhookHearsAssets } from '@/components/import/reviewModel'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const route = useRoute()
const router = useRouter()
const id = computed(() => String(route.params.id ?? ''))

const jobQuery = useQuery({
  key: () => importJobQueryKey(id.value),
  query: () => importService.get(id.value),
  enabled: () => !!id.value,
})
const job = computed(() => jobQuery.data.value)

// A draft goes on in the wizard.
watch(job, (j) => {
  if (j?.state === 'draft') router.replace(resumeTarget(j.id))
})

const jobType = computed(() => job.value?.job_type ?? 'assets')
const fieldsQuery = useQuery({
  key: () => importFieldsQueryKey(jobType.value),
  query: () => importService.fields(jobType.value),
  enabled: () => !!job.value,
})
const fields = computed(() => fieldsQuery.data.value ?? [])

const { kinds: assetKinds } = useAssetKindsQuery()
const kindOptions = computed(() => assetKinds.value.map((k) => ({ value: k.slug, label: k.label })))

const queryCache = useQueryCache()

/** The workspace's webhooks, for the removal dialog's line about asset
 *  events. Read while removal is offered. */
const webhooksQuery = useQuery({
  key: ['webhooks'],
  query: () => webhookService.listWebhooks(),
  enabled: () => !!job.value && removalState(job.value) === 'offer',
})
const webhooksHearAssets = computed(() => webhookHearsAssets(webhooksQuery.data.value ?? []))

/** Remove imported assets ran: the page shows its result, and Recent
 *  imports its count. The previous values counts aren't on the removal's
 *  answer and haven't changed. */
function onRemoved(updated: ImportJob) {
  queryCache.setQueryData(importJobQueryKey(updated.id), {
    ...updated,
    previous_values: job.value?.previous_values ?? null,
  })
  queryCache.invalidateQueries({ key: ['import-jobs'] })
}

const imported = computed(() => job.value?.state === 'imported')
const kept = computed(() => !!job.value && detailsKept(job.value))

const title = computed(() => {
  const j = job.value
  if (!j) return ''
  return t('import-detail-title', { date: formatCompactDate(j.completed_at ?? j.created_at) })
})

const subtitle = computed(() => {
  const j = job.value
  if (!j) return ''
  const parts = [fileLabel({ filename: j.filename, sheet: j.source?.sheet })]
  if (j.created_by_name) parts.push(j.created_by_name)
  parts.push(t('import-matches-nouns', { type: j.job_type }))
  return parts.join(' · ')
})

const options = computed(() => (job.value ? optionsLine(t, job.value.job_type, job.value.options) : ''))

/** Each of the file's columns with where it went. */
const mappingRows = computed(() => {
  const j = job.value
  if (!j?.source) return []
  const byIndex = new Map<number, string>()
  for (const [field, index] of Object.entries(j.mapping ?? {})) byIndex.set(index, field)
  const ignored = new Set(j.ignored ?? [])
  const labels = new Map(fields.value.map((f) => [f.id, f.label_key ? t(f.label_key) : f.label]))
  return j.source.headers.map((h) => {
    const field = byIndex.get(h.index)
    return {
      index: h.index,
      header: h.name,
      target: field ? (labels.get(field) ?? field) : null,
      ignored: !field && ignored.has(h.index),
    }
  })
})
const mappedCount = computed(() => mappingRows.value.filter((r) => r.target).length)

const downloading = ref(false)
const errorMessage = ref('')

async function downloadReport(only?: { group?: number; dropped?: number[] }) {
  if (!job.value || downloading.value) return
  downloading.value = true
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
    const err = e as { response?: { status?: number } }
    errorMessage.value =
      err?.response?.status === 410 ? t('import-detail-kept-30-days') : t('import-error-report-failed')
  } finally {
    downloading.value = false
  }
}

const previous = computed(() => (job.value?.job_type === 'assets' ? job.value.previous_values ?? null : null))
const previousHelp = computed(() => previousValuesHelp(t, previous.value?.blank_before ?? 0))
const downloadingPrevious = ref(false)

/** Save the previous values file. */
async function downloadPrevious() {
  if (!job.value || downloadingPrevious.value) return
  downloadingPrevious.value = true
  errorMessage.value = ''
  try {
    const { blob, filename } = await importService.previousValues(job.value.id)
    const link = document.createElement('a')
    link.href = URL.createObjectURL(blob)
    link.download = filename
    document.body.appendChild(link)
    link.click()
    document.body.removeChild(link)
    URL.revokeObjectURL(link.href)
  } catch (e) {
    const err = e as { response?: { status?: number } }
    errorMessage.value =
      err?.response?.status === 410 ? t('import-detail-kept-30-days') : t('import-previous-failed')
  } finally {
    downloadingPrevious.value = false
  }
}

/** "Imported", or "Imported · 398 of 407 assets removed". */
const stateLabel = computed(() => {
  const j = job.value
  if (!j?.state) return ''
  const r = j.remove_result
  if (j.state === 'imported' && r) {
    return t('import-detail-state-removed', { deleted: r.deleted, created: r.created })
  }
  return t(STATE_KEY[j.state])
})

const linkClass =
  'text-accent hover:underline focus:outline-none focus-visible:ring-1 focus-visible:ring-accent rounded'
</script>

<template>
  <div class="flex-1">
    <div class="pt-4 px-4 sm:px-6 flex justify-between items-center">
      <BackButton fallbackRoute="/admin/data-import" :label="$t('import-matches-back-hub')" />
    </div>

    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-4xl">
      <ErrorBanner v-if="jobQuery.error.value" :message="$t('import-detail-load-failed')" />

      <template v-else-if="job && job.state !== 'draft'">
        <div class="flex flex-col gap-2">
          <h1 class="text-2xl font-bold text-primary" data-test="import-detail-title">{{ title }}</h1>
          <p class="text-secondary break-words">{{ subtitle }}</p>
        </div>

        <dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1.5 text-sm" data-test="import-detail-facts">
          <dt class="text-tertiary">{{ $t('import-detail-state') }}</dt>
          <dd class="text-primary" data-test="import-detail-state">
            {{ stateLabel }}
          </dd>
          <template v-if="options">
            <dt class="text-tertiary">{{ $t('import-detail-options') }}</dt>
            <dd class="text-primary break-words">{{ options }}</dd>
          </template>
          <template v-if="mappingRows.length > 0">
            <dt class="text-tertiary">{{ $t('import-detail-mapping') }}</dt>
            <dd class="min-w-0">
              <details class="group">
                <summary class="cursor-pointer list-none text-accent hover:underline">
                  {{ $t('import-map-summary-mapped', { mapped: mappedCount, total: mappingRows.length }) }}
                  · {{ $t('import-map-show-columns') }}
                </summary>
                <table class="mt-2 w-full table-fixed text-xs" data-test="import-detail-mapping">
                  <tbody class="divide-y divide-default">
                    <tr v-for="r in mappingRows" :key="r.index">
                      <td class="py-1.5 pr-3 font-mono text-primary break-words">{{ r.header }}</td>
                      <td class="py-1.5 break-words">
                        <span v-if="r.target" class="text-primary">{{ r.target }}</span>
                        <span v-else-if="r.ignored" class="text-tertiary">{{ $t('import-map-ignore') }}</span>
                        <span v-else class="text-tertiary">{{ $t('import-matches-no-field') }}</span>
                      </td>
                    </tr>
                  </tbody>
                </table>
              </details>
            </dd>
          </template>
        </dl>

        <ErrorBanner v-if="errorMessage" :message="errorMessage" />

        <p v-if="job.state === 'expired'" class="text-sm text-secondary" data-test="import-detail-expired">
          {{ $t('import-detail-expired') }}
        </p>

        <template v-else-if="imported && job.summary && !kept">
          <ImportReviewStep :job="job" :fields="fields" :kinds="kindOptions" mode="counts" />
          <p class="text-sm text-tertiary" data-test="import-detail-kept">{{ $t('import-detail-kept-30-days') }}</p>
          <ImportRemoveBlock :job="job" />
        </template>

        <template v-else-if="imported && job.summary">
          <ImportReviewStep
            :job="job"
            :fields="fields"
            :kinds="kindOptions"
            mode="detail"
            :blank-before="previous?.blank_before ?? 0"
            @download-report="downloadReport"
          />
          <div class="flex flex-col gap-2">
            <div class="flex flex-wrap items-center gap-2" data-test="import-detail-actions">
              <Button
                v-if="reportOffered(job)"
                variant="secondary"
                icon="download"
                :loading="downloading"
                data-test="import-detail-report"
                @click="downloadReport()"
              >
                {{ $t('import-review-download-report') }}
              </Button>
              <Button
                v-if="previous"
                variant="secondary"
                icon="download"
                :disabled="previous.rows === 0"
                :loading="downloadingPrevious"
                data-test="import-detail-previous"
                @click="downloadPrevious"
              >
                {{ $t('import-previous-download') }}
              </Button>
              <span v-if="previous && previous.rows === 0" class="text-xs text-tertiary" data-test="import-detail-previous-none">
                {{ $t('import-previous-none') }}
              </span>
            </div>
            <p v-if="previous && previous.rows > 0" class="text-xs text-tertiary" data-test="import-detail-previous-help">
              {{ previousHelp }}
            </p>
          </div>
          <ImportRemoveBlock :job="job" :webhooks="webhooksHearAssets" @removed="onRemoved" />
        </template>

        <p class="text-sm">
          <RouterLink :class="linkClass" to="/admin/data-import" data-test="import-detail-recent">
            {{ $t('import-history-title') }}
          </RouterLink>
        </p>
      </template>
    </div>
  </div>
</template>
