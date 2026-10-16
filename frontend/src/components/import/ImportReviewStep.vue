<script setup lang="ts">
/**
 * Review step: what the dry run found, the most common problems, every
 * row's result page by page, the error report download, and the import
 * button with its confirmation.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQuery } from '@pinia/colada'

import Button from '@/components/common/Button.vue'
import Callout from '@/components/common/Callout.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import Icon from '@/components/common/Icon.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import {
  importService,
  type ImportField,
  type ImportJob,
  type ImportRowResult,
  type RowFilter,
} from '@nosdesk/core/services/importService'

const props = defineProps<{
  job: ImportJob
  fields: ImportField[]
  committing: boolean
  downloading?: boolean
  /** Every value in the mapped columns matched without a question, so
   *  Match values wasn't needed. */
  valuesAllMatched?: boolean
}>()

const emit = defineEmits<{
  (e: 'back'): void
  (e: 'see-values'): void
  (e: 'commit'): void
  (e: 'download-report'): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const summary = computed(() => props.job.summary!)
const created = computed(() => summary.value.would_create)
const updated = computed(() => summary.value.would_update)
const unchanged = computed(() => summary.value.unchanged ?? 0)
const imported = computed(() => created.value + updated.value)
const skipped = computed(
  () =>
    summary.value.rows_with_errors ??
    Math.max(summary.value.row_count - imported.value - unchanged.value, 0),
)
const errorsTotal = computed(() => summary.value.errors_total ?? summary.value.errors.length)

const fieldById = computed(() => new Map(props.fields.map((f) => [f.id, f])))

/** A field id as the admin sees it: their own header first. */
function columnName(column: string | null, source?: string | null): string {
  if (source) return source
  if (!column) return t('import-review-whole-row')
  const field = fieldById.value.get(column)
  if (!field) return column
  return field.label_key ? t(field.label_key) : field.label
}

const headline = computed(() => {
  const parts: string[] = [t('import-review-checked', { count: summary.value.row_count })]
  const changes: string[] = []
  if (created.value > 0) changes.push(t('import-review-created', { count: created.value }))
  if (updated.value > 0) changes.push(t('import-review-updated', { count: updated.value }))
  if (imported.value === 0) parts.push(t('import-review-nothing'))
  else if (skipped.value === 0 && unchanged.value === 0)
    parts.push(t('import-review-all-import', { count: imported.value, parts: changes.join(', ') }))
  else parts.push(t('import-review-will-import', { count: imported.value, parts: changes.join(', ') }))
  if (unchanged.value > 0) parts.push(t('import-review-unchanged', { count: unchanged.value }))
  if (skipped.value > 0) parts.push(t('import-review-skipped', { count: skipped.value }))
  return parts.join(' ')
})

const topProblems = computed(() => (summary.value.error_groups ?? []).slice(0, 5))

/** Notices worded by code; an unknown code shows the server's sentence. */
const NOTICE_KEYS: Record<string, string> = {
  no_key: 'import-review-notice-no-key',
  location_snapped: 'import-review-notice-location-snapped',
}
const notices = computed(() =>
  (summary.value.notices ?? []).map((n) =>
    NOTICE_KEYS[n.code] ? t(NOTICE_KEYS[n.code], { count: n.count }) : n.message,
  ),
)

// ---- rows ----

const PAGE = 50
const filter = ref<RowFilter>(skipped.value > 0 ? 'skipped' : 'all')
const offset = ref(0)
watch(filter, () => {
  offset.value = 0
})

const rowsQuery = useQuery({
  key: () => ['import-rows', props.job.id, props.job.updated_at, filter.value, offset.value],
  query: () =>
    importService.rows(props.job.id, { filter: filter.value, offset: offset.value, limit: PAGE }),
})
const page = computed(() => rowsQuery.data.value)
const rows = computed<ImportRowResult[]>(() => page.value?.rows ?? [])
const total = computed(() => page.value?.total ?? 0)

const filterCounts = computed<Record<RowFilter, number>>(() => ({
  all: summary.value.row_count,
  skipped: skipped.value,
  create: created.value,
  update: updated.value,
  unchanged: unchanged.value,
}))
const filterOptions = computed(() =>
  (['all', 'skipped', 'create', 'update', 'unchanged'] as RowFilter[])
    .filter((f) => f === 'all' || f === filter.value || filterCounts.value[f] > 0)
    .map((f) => ({
      value: f,
      label: t(`import-review-filter-${f}`, { count: filterCounts.value[f] }),
    })),
)

const rangeText = computed(() =>
  t('import-review-rows-range', {
    from: total.value === 0 ? 0 : offset.value + 1,
    to: Math.min(offset.value + PAGE, total.value),
    total: total.value,
  }),
)

function rowProblems(row: ImportRowResult): string {
  return (row.errors ?? [])
    .map((e) => (e.column ? `${columnName(e.column, e.source_column)}: ${e.message}` : e.message))
    .join('; ')
}

const OUTCOME_CLASS: Record<ImportRowResult['outcome'], string> = {
  create: 'text-status-success',
  update: 'text-accent',
  unchanged: 'text-tertiary',
  skipped: 'text-status-error',
}

const showConfirm = ref(false)
const confirmMessage = computed(() => {
  const parts = [
    t('import-review-confirm-changes', {
      type: props.job.job_type,
      created: created.value,
      updated: updated.value,
    }),
  ]
  if (skipped.value > 0) parts.push(t('import-review-confirm-skipped', { count: skipped.value }))
  return parts.join(' ')
})

function confirm() {
  showConfirm.value = false
  emit('commit')
}
</script>

<template>
  <section class="flex flex-col gap-6">
    <p class="text-base text-primary" aria-live="polite">
      {{ committing ? $t('import-review-importing') : headline }}
    </p>

    <p
      v-if="valuesAllMatched && !committing"
      class="text-sm text-secondary flex flex-wrap items-center gap-x-2"
      data-test="import-review-values-matched"
    >
      <span>{{ $t('import-review-values-matched') }}</span>
      <span aria-hidden="true" class="text-tertiary">·</span>
      <button
        type="button"
        class="text-accent hover:underline focus:outline-none focus-visible:ring-1 focus-visible:ring-accent rounded"
        data-test="import-review-see-values"
        @click="emit('see-values')"
      >
        {{ $t('import-review-values-see') }}
      </button>
    </p>

    <div class="grid grid-cols-2 gap-3" :class="unchanged > 0 ? 'sm:grid-cols-4' : 'sm:grid-cols-3'">
      <div class="bg-surface border border-default rounded-lg p-4 flex flex-col gap-1">
        <p class="text-xs uppercase tracking-wide text-tertiary">{{ $t('import-review-tile-create') }}</p>
        <p class="text-2xl font-semibold text-primary">{{ created }}</p>
      </div>
      <div class="bg-surface border border-default rounded-lg p-4 flex flex-col gap-1">
        <p class="text-xs uppercase tracking-wide text-tertiary">{{ $t('import-review-tile-update') }}</p>
        <p class="text-2xl font-semibold text-primary">{{ updated }}</p>
      </div>
      <div v-if="unchanged > 0" class="bg-surface border border-default rounded-lg p-4 flex flex-col gap-1">
        <p class="text-xs uppercase tracking-wide text-tertiary">{{ $t('import-review-tile-unchanged') }}</p>
        <p class="text-2xl font-semibold text-primary">{{ unchanged }}</p>
      </div>
      <div class="bg-surface border border-default rounded-lg p-4 flex flex-col gap-1">
        <p class="text-xs uppercase tracking-wide text-tertiary">{{ $t('import-review-tile-errors') }}</p>
        <p class="text-2xl font-semibold text-status-error flex items-center gap-2">
          <Icon v-if="skipped > 0" name="xCircle" size="md" />
          {{ skipped }}
        </p>
        <p v-if="errorsTotal !== skipped" class="text-xs text-tertiary">
          {{ $t('import-review-tile-errors-sub', { count: errorsTotal }) }}
        </p>
      </div>
    </div>

    <div v-if="topProblems.length > 0" class="flex flex-col gap-2">
      <h2 class="text-sm font-medium text-primary">{{ $t('import-review-top-problems') }}</h2>
      <ul class="flex flex-col gap-1">
        <li v-for="(g, i) in topProblems" :key="i" class="text-sm text-primary">
          <span class="font-medium">{{ columnName(g.column, g.source_column) }}:</span>
          {{ g.message }}
          <span class="text-tertiary whitespace-nowrap">· {{ $t('import-review-problem-rows', { count: g.count }) }}</span>
        </li>
      </ul>
    </div>

    <Callout v-if="notices.length > 0" severity="info">
      <ul class="px-4 py-3 flex flex-col gap-1 text-sm text-secondary">
        <li v-for="(n, i) in notices" :key="i">{{ n }}</li>
      </ul>
    </Callout>

    <div class="flex flex-col gap-3" data-test="import-rows">
      <div class="flex flex-wrap items-center gap-3">
        <h2 class="text-sm font-medium text-primary">{{ $t('import-review-rows-heading') }}</h2>
        <Button
          v-if="skipped > 0"
          class="ml-auto"
          variant="secondary"
          size="sm"
          icon="download"
          :loading="downloading"
          data-test="import-error-report"
          @click="emit('download-report')"
        >
          {{ $t('import-review-download-report') }}
        </Button>
      </div>
      <SegmentedControl
        v-model="filter"
        :options="filterOptions"
        :aria-label="$t('import-review-filter-label')"
        size="sm"
      />
      <div class="border border-default rounded-lg overflow-hidden">
        <table class="w-full table-fixed text-sm">
          <thead class="bg-surface-alt">
            <tr class="text-left text-tertiary text-xs uppercase tracking-wide">
              <th scope="col" class="px-3 sm:px-4 py-2 w-14 sm:w-16 font-medium">{{ $t('import-review-col-row') }}</th>
              <th scope="col" class="px-3 sm:px-4 py-2 w-32 sm:w-56 font-medium">{{ $t('import-review-col-name') }}</th>
              <th scope="col" class="px-3 sm:px-4 py-2 font-medium">{{ $t('import-review-col-result') }}</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-default">
            <tr v-for="r in rows" :key="r.row" class="text-primary align-top" data-test="import-row">
              <td class="px-3 sm:px-4 py-2 font-mono text-tertiary">{{ r.row }}</td>
              <td class="px-3 sm:px-4 py-2 break-words">
                <span>{{ r.label || r.key || '' }}</span>
                <span v-if="r.label && r.key" class="block text-xs text-tertiary">{{ r.key }}</span>
              </td>
              <td class="px-3 sm:px-4 py-2 break-words">
                <span class="font-medium" :class="OUTCOME_CLASS[r.outcome]">{{ $t(`import-review-outcome-${r.outcome}`) }}</span>
                <span v-if="r.errors?.length" class="block text-secondary">{{ rowProblems(r) }}</span>
              </td>
            </tr>
            <tr v-if="rows.length === 0">
              <td colspan="3" class="px-3 sm:px-4 py-3 text-secondary">
                {{ rowsQuery.isLoading.value ? $t('import-review-rows-loading') : $t('import-review-rows-none') }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      <div v-if="total > PAGE" class="flex items-center gap-3 text-sm text-secondary">
        <span>{{ rangeText }}</span>
        <Button
          class="ml-auto"
          variant="ghost"
          size="sm"
          :disabled="offset === 0"
          @click="offset = Math.max(offset - PAGE, 0)"
        >
          {{ $t('import-review-rows-previous') }}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          :disabled="offset + PAGE >= total"
          data-test="import-rows-next"
          @click="offset += PAGE"
        >
          {{ $t('import-review-rows-next') }}
        </Button>
      </div>
    </div>

    <div
      class="sticky bottom-0 bg-app border-t border-default py-3 flex flex-wrap items-center gap-3"
    >
      <Button variant="ghost" :disabled="committing" @click="emit('back')">
        {{ $t('import-review-back') }}
      </Button>
      <span class="ml-auto text-xs text-tertiary">
        <template v-if="imported === 0">{{ $t('import-review-nothing-to-import') }}</template>
        <template v-else-if="skipped > 0">{{ $t('import-review-skip-reason', { count: skipped }) }}</template>
      </span>
      <Button
        :disabled="imported === 0 || committing"
        :loading="committing"
        data-test="import-commit"
        @click="showConfirm = true"
      >
        {{ committing ? $t('import-review-importing') : $t('import-review-import', { count: imported }) }}
      </Button>
    </div>

    <ConfirmModal
      :show="showConfirm"
      :title="$t('import-review-confirm-title', { count: imported })"
      :message="confirmMessage"
      :confirm-label="$t('import-review-import', { count: imported })"
      variant="info"
      @close="showConfirm = false"
      @confirm="confirm"
    />
  </section>
</template>
