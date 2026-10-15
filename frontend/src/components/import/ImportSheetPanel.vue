<script setup lang="ts">
/**
 * Sheet panel at the top of Map for a workbook: which sheet to import,
 * which row holds the column names, the top of the sheet to confirm it,
 * and how a workbook is read (merged cells, hidden rows, leading zeros,
 * formulas).
 *
 * Open when the workbook has more than one sheet with rows or its header
 * isn't on row 1, so a guess past a title row is always seen once;
 * otherwise one line with Change. A choice emits `choose` and the view
 * stores it with `PUT source`; a sheet without a row number takes the
 * server's guess.
 */
import { computed, ref, watch } from 'vue'

import Button from '@/components/common/Button.vue'
import FormNumber from '@/components/common/FormNumber.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import Spinner from '@/components/common/Spinner.vue'
import StatusPill from '@/components/common/StatusPill.vue'
import type { ImportSheet, ImportSource } from '@nosdesk/core/services/importService'

/** Rows one import takes; a sheet reporting more is over the cap. */
const MAX_ROWS = 5000

const props = defineProps<{
  source: ImportSource
  busy: boolean
}>()

const emit = defineEmits<{
  (e: 'choose', sheet: string, headerRow: number | null): void
}>()

const sheetsWithRows = computed(() => props.source.sheets.filter((s) => s.rows > 0))
const open = ref(sheetsWithRows.value.length > 1 || props.source.header_row > 1)

const headerRow = ref<number | null>(props.source.header_row)
watch(
  () => props.source.header_row,
  (row) => {
    headerRow.value = row
  },
)

const preview = computed(() => props.source.preview ?? [])
const previewColumns = computed(() =>
  preview.value.reduce((n, r) => Math.max(n, r.cells.length), 0),
)

function rowsKey(sheet: ImportSheet): { key: string; args?: Record<string, number> } {
  if (sheet.rows > MAX_ROWS) return { key: 'import-sheet-rows-over' }
  return { key: 'import-sheet-rows', args: { count: sheet.rows } }
}

function pickSheet(name: string) {
  if (props.busy || name === props.source.sheet) return
  emit('choose', name, null)
}

function useRow(row: number | null) {
  if (props.busy || !props.source.sheet || !row || row === props.source.header_row) return
  emit('choose', props.source.sheet, row)
}

const canApply = computed(
  () => !props.busy && !!headerRow.value && headerRow.value !== props.source.header_row,
)
</script>

<template>
  <SectionCard content-padding="p-4" data-test="import-sheet-panel">
    <template #title>{{ $t('import-sheet-heading') }}</template>

    <!-- Collapsed: the choice in one line. -->
    <div v-if="!open" class="flex flex-wrap items-center gap-3">
      <p class="text-sm text-primary" data-test="import-sheet-summary">
        {{ $t('import-sheet-summary', { sheet: source.sheet ?? '', row: source.header_row }) }}
      </p>
      <Button
        class="ml-auto"
        variant="ghost"
        size="sm"
        data-test="import-sheet-change"
        @click="open = true"
      >
        {{ $t('import-sheet-change') }}
      </Button>
    </div>

    <div v-else class="flex flex-col gap-5">
      <fieldset v-if="source.sheets.length > 1" class="flex flex-col gap-2" :disabled="busy">
        <legend class="text-sm font-medium text-primary pb-2">{{ $t('import-sheet-legend') }}</legend>
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
          <label
            v-for="sheet in source.sheets"
            :key="sheet.name"
            class="relative flex items-center gap-3 rounded-lg border px-3 py-2 cursor-pointer transition-colors"
            :class="sheet.name === source.sheet
              ? 'border-accent bg-accent/5'
              : 'border-default hover:bg-surface-hover'"
            data-test="import-sheet-option"
          >
            <input
              type="radio"
              name="import-sheet"
              class="peer sr-only"
              :value="sheet.name"
              :checked="sheet.name === source.sheet"
              @change="pickSheet(sheet.name)"
            />
            <span
              aria-hidden="true"
              class="pointer-events-none absolute inset-0 rounded-lg peer-focus-visible:ring-2 peer-focus-visible:ring-accent"
            />
            <span class="flex flex-col gap-0.5 min-w-0">
              <span class="text-sm text-primary font-medium truncate">{{ sheet.name }}</span>
              <span class="text-xs text-secondary">{{ $t(rowsKey(sheet).key, rowsKey(sheet).args) }}</span>
            </span>
            <StatusPill
              v-if="!sheet.visible"
              class="ml-auto"
              :label="$t('import-sheet-hidden')"
              data-test="import-sheet-hidden"
            />
          </label>
        </div>
      </fieldset>

      <div class="flex flex-col gap-1">
        <div class="flex flex-wrap items-end gap-3">
          <FormNumber
            v-model="headerRow"
            class="w-40"
            size="sm"
            integer
            :min="1"
            :label="$t('import-sheet-header-label')"
            :disabled="busy"
            data-test="import-sheet-header-row"
          />
          <Button
            variant="secondary"
            size="sm"
            :disabled="!canApply"
            data-test="import-sheet-apply"
            @click="useRow(headerRow)"
          >
            {{ $t('import-sheet-use-row', { row: headerRow ?? '' }) }}
          </Button>
          <p v-if="busy" class="flex items-center gap-2 text-sm text-secondary" aria-live="polite">
            <Spinner size="sm" />
            {{ $t('import-sheet-reading') }}
          </p>
        </div>
        <p class="text-xs text-secondary">{{ $t('import-sheet-header-help') }}</p>
      </div>

      <!-- The sheet's top, so the header row can be checked by eye. The
           table scrolls inside its box on a narrow screen. -->
      <div v-if="preview.length > 0" class="flex flex-col gap-2">
        <p class="text-xs text-secondary">
          {{ $t('import-sheet-preview-caption', { sheet: source.sheet ?? '' }) }}
        </p>
        <div class="overflow-x-auto rounded-lg border border-default">
          <table class="min-w-full text-xs">
            <tbody>
              <tr
                v-for="row in preview"
                :key="row.row"
                class="border-b border-default last:border-b-0"
                :class="row.row === source.header_row ? 'bg-accent/10 font-medium text-primary' : 'text-secondary'"
                :aria-current="row.row === source.header_row ? 'true' : undefined"
                data-test="import-sheet-preview-row"
              >
                <th scope="row" class="w-12 px-2 py-1 text-right font-normal text-tertiary bg-surface-alt">
                  <button
                    type="button"
                    class="w-full text-right hover:underline disabled:no-underline"
                    :disabled="busy || row.row === source.header_row"
                    :aria-label="$t('import-sheet-preview-use', { row: row.row })"
                    @click="useRow(row.row)"
                  >
                    {{ row.row }}
                  </button>
                </th>
                <td
                  v-for="i in previewColumns"
                  :key="i"
                  class="px-2 py-1 whitespace-nowrap max-w-[12rem] truncate"
                >
                  {{ row.cells[i - 1] ?? '' }}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>

      <div class="flex flex-col gap-1">
        <p class="text-sm font-medium text-primary">{{ $t('import-sheet-help-heading') }}</p>
        <ul class="flex flex-col gap-1 text-xs text-secondary list-disc pl-5">
          <li>{{ $t('import-sheet-help-merged') }}</li>
          <li>{{ $t('import-sheet-help-hidden') }}</li>
          <li>{{ $t('import-sheet-help-zeros') }}</li>
          <li>{{ $t('import-sheet-help-formulas') }}</li>
        </ul>
      </div>

      <div class="flex">
        <Button
          class="ml-auto"
          variant="ghost"
          size="sm"
          :disabled="busy"
          data-test="import-sheet-done"
          @click="open = false"
        >
          {{ $t('import-sheet-done') }}
        </Button>
      </div>
    </div>
  </SectionCard>
</template>
