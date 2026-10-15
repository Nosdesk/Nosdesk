<script setup lang="ts">
/**
 * Review step: what the dry run found, the most common problems, the
 * first errors row by row, and the import button with its confirmation.
 * The full row list and the error report download arrive with the
 * preview endpoints.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import Callout from '@/components/common/Callout.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import Icon from '@/components/common/Icon.vue'
import type { ImportField, ImportJob } from '@nosdesk/core/services/importService'

const props = defineProps<{
  job: ImportJob
  fields: ImportField[]
  committing: boolean
}>()

const emit = defineEmits<{
  (e: 'back'): void
  (e: 'commit'): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const summary = computed(() => props.job.summary!)
const created = computed(() => summary.value.would_create)
const updated = computed(() => summary.value.would_update)
const imported = computed(() => created.value + updated.value)
const skipped = computed(
  () => summary.value.rows_with_errors ?? Math.max(summary.value.row_count - imported.value, 0),
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
  else if (skipped.value === 0)
    parts.push(t('import-review-all-import', { count: imported.value, parts: changes.join(', ') }))
  else parts.push(t('import-review-will-import', { count: imported.value, parts: changes.join(', ') }))
  if (skipped.value > 0) parts.push(t('import-review-skipped', { count: skipped.value }))
  return parts.join(' ')
})

const topProblems = computed(() => (summary.value.error_groups ?? []).slice(0, 5))

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

    <div class="grid grid-cols-2 sm:grid-cols-3 gap-3">
      <div class="bg-surface border border-default rounded-lg p-4 flex flex-col gap-1">
        <p class="text-xs uppercase tracking-wide text-tertiary">{{ $t('import-review-tile-create') }}</p>
        <p class="text-2xl font-semibold text-primary">{{ created }}</p>
      </div>
      <div class="bg-surface border border-default rounded-lg p-4 flex flex-col gap-1">
        <p class="text-xs uppercase tracking-wide text-tertiary">{{ $t('import-review-tile-update') }}</p>
        <p class="text-2xl font-semibold text-primary">{{ updated }}</p>
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

    <Callout v-if="summary.errors.length > 0" severity="error" icon="xCircle">
      <template #header>
        <span class="font-medium text-primary">{{ $t('import-review-errors-heading') }}</span>
        <span v-if="summary.errors_truncated" class="text-tertiary ml-1">
          {{ $t('import-review-errors-truncated', { shown: summary.errors.length, total: errorsTotal }) }}
        </span>
      </template>
      <table class="w-full table-fixed text-sm">
        <thead class="bg-surface-alt">
          <tr class="text-left text-tertiary text-xs uppercase tracking-wide">
            <th scope="col" class="px-3 sm:px-4 py-2 w-14 sm:w-16 font-medium">{{ $t('import-review-col-row') }}</th>
            <th scope="col" class="px-3 sm:px-4 py-2 w-24 sm:w-40 font-medium">{{ $t('import-review-col-column') }}</th>
            <th scope="col" class="px-3 sm:px-4 py-2 font-medium">{{ $t('import-review-col-message') }}</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-default">
          <tr v-for="(e, i) in summary.errors" :key="i" class="text-primary align-top">
            <td class="px-3 sm:px-4 py-2 font-mono text-tertiary">{{ e.row }}</td>
            <td class="px-3 sm:px-4 py-2 text-secondary break-words">{{ columnName(e.column, e.source_column) }}</td>
            <td class="px-3 sm:px-4 py-2 break-words">{{ e.message }}</td>
          </tr>
        </tbody>
      </table>
    </Callout>

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
