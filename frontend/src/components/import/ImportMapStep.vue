<script setup lang="ts">
/**
 * Map step: one row per source column, in source order, with up to three
 * sample values and a target picker pre-filled from the server's
 * suggestions. Blocking problems disable the check; the options card
 * holds the choices the dry run reads. The rules live in `mapModel.ts`.
 */
import { computed, nextTick, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'

import BaseDropdown from '@/components/common/BaseDropdown.vue'
import Button from '@/components/common/Button.vue'
import Callout from '@/components/common/Callout.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import Icon from '@/components/common/Icon.vue'
import SearchableDropdown from '@/components/common/SearchableDropdown.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import StatusPill from '@/components/common/StatusPill.vue'
import type { DropdownOption } from '@/components/common/dropdownOption'
import type { StatusPillTone } from '@/components/common/statusPillTone'
import { useMobileDetection } from '@/composables/useMobileDetection'
import type {
  ColumnMapping,
  DateOrder,
  DecimalMark,
  ImportField,
  ImportJob,
  ImportOptions,
  MatchBy,
  SuggestionReason,
} from '@nosdesk/core/services/importService'

import {
  IGNORE,
  buildMapping,
  clashingColumns,
  counts,
  initialRows,
  isNotImported,
  mapsValueType,
  matchesFilter,
  pillReason,
  problems,
  type MapFilter,
  type MapRow,
  type Problem,
} from './mapModel'

const props = defineProps<{
  job: ImportJob
  fields: ImportField[]
  /** Asset kinds for the default kind picker, as slug and label. */
  kinds: { value: string; label: string }[]
  checking: boolean
}>()

const emit = defineEmits<{
  (e: 'check', mapping: ColumnMapping, options: ImportOptions): void
  (e: 'change-file'): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const { isMobile } = useMobileDetection('md')

const source = computed(() => props.job.source!)
const isAssets = computed(() => props.job.job_type === 'assets')

const rows = ref<MapRow[]>(
  initialRows(source.value, props.job.suggested_mapping ?? [], props.job.mapping),
)

const stored = props.job.options ?? {}
const matchBy = ref<MatchBy>(stored.match_by ?? 'asset_tag')
const defaultKind = ref<string>(stored.default_kind ?? '')
const dateOrder = ref<DateOrder>(stored.date_order ?? 'iso')
const decimal = ref<DecimalMark>(
  stored.decimal ?? (source.value.delimiter === ';' ? ',' : '.'),
)

const filter = ref<MapFilter>('all')

const fieldById = computed(() => new Map(props.fields.map((f) => [f.id, f])))

function fieldLabel(id: string): string {
  const field = fieldById.value.get(id)
  if (!field) return id
  return field.label_key ? t(field.label_key) : field.label
}

function headerName(index: number): string {
  return rows.value.find((r) => r.index === index)?.name ?? ''
}

const found = computed<Problem[]>(() =>
  problems(rows.value, props.fields, props.job.job_type, defaultKind.value || null),
)
const tally = computed(() => counts(rows.value, found.value))
const kindMapped = computed(() => rows.value.some((r) => r.target === 'kind'))

const GROUP_KEYS: Record<ImportField['group'], string> = {
  core: 'import-map-group-core',
  links: 'import-map-group-links',
  lifecycle: 'import-map-group-lifecycle',
  attribute: 'import-map-group-attribute',
}

/** Picker options for one row: every field under its group, then
 *  "Don't import". A field mapped from another column says so. */
function optionsFor(row: MapRow): DropdownOption[] {
  const usedBy = new Map<string, string>()
  for (const r of rows.value) {
    if (r.index !== row.index && r.target && r.target !== IGNORE) usedBy.set(r.target, r.name)
  }
  const options: DropdownOption[] = props.fields.map((f) => {
    let description: string | undefined
    const other = usedBy.get(f.id)
    if (other !== undefined) description = t('import-map-mapped-from', { header: other })
    else if (isAssets.value && f.id === 'kind') description = t('import-map-kind-required')
    else if (f.required) description = t('import-map-required')
    return {
      value: f.id,
      label: fieldLabel(f.id),
      description,
      group: t(GROUP_KEYS[f.group]),
    }
  })
  options.push({
    value: IGNORE,
    label: t('import-map-ignore'),
    description: t('import-map-ignore-description'),
    group: t('import-map-group-ignore'),
  })
  return options
}

const PILL: Record<SuggestionReason, { key: string; tone: StatusPillTone }> = {
  previous: { key: 'import-map-pill-previous', tone: 'neutral' },
  exact: { key: 'import-map-pill-exact', tone: 'neutral' },
  label: { key: 'import-map-pill-label', tone: 'neutral' },
  synonym: { key: 'import-map-pill-synonym', tone: 'caution' },
  report: { key: 'import-map-pill-exact', tone: 'neutral' },
}

function pillFor(row: MapRow) {
  const reason = pillReason(row)
  return reason ? PILL[reason] : null
}

function clashText(row: MapRow): string {
  const others = clashingColumns(row, found.value)
  if (others.length === 0) return ''
  return t('import-map-clash', { headers: others.map((i) => `'${headerName(i)}'`).join(', ') })
}

function problemText(p: Problem): string {
  switch (p.kind) {
    case 'duplicate':
      return t('import-map-problem-duplicate', {
        count: p.columns.length,
        field: fieldLabel(p.field),
        headers: p.columns.map((i) => `'${headerName(i)}'`).join(', '),
      })
    case 'required':
      return t('import-map-problem-required', { field: fieldLabel(p.field) })
    case 'kind':
      return t('import-map-problem-kind')
  }
}

const visibleRows = computed(() =>
  rows.value.filter((r) => matchesFilter(r, filter.value, found.value)),
)

const filterOptions = computed(() => [
  { value: 'all' as MapFilter, label: t('import-map-filter-all') },
  { value: 'attention' as MapFilter, label: t('import-map-filter-attention') },
  { value: 'ignored' as MapFilter, label: t('import-map-filter-ignored') },
])

// ---- focus helpers ----

const tableRoot = ref<HTMLElement | null>(null)

function triggerFor(index: number): HTMLElement | null {
  return tableRoot.value?.querySelector<HTMLElement>(`[data-map-column="${index}"] button[aria-haspopup]`) ?? null
}

async function focusColumn(index: number) {
  if (!rows.value.some((r) => r.index === index && matchesFilter(r, filter.value, found.value))) {
    filter.value = 'all'
    await nextTick()
  }
  const el = triggerFor(index)
  el?.scrollIntoView({ block: 'center', behavior: 'smooth' })
  el?.focus()
}

function problemColumn(p: Problem): number | null {
  return p.kind === 'duplicate' ? p.columns[0] : null
}

const clashColumns = computed(() =>
  rows.value.filter((r) => clashingColumns(r, found.value).length > 0).map((r) => r.index),
)

let lastProblem = -1
function nextProblem() {
  const list = clashColumns.value
  if (list.length === 0) return
  const next = list.find((i) => i > lastProblem) ?? list[0]
  lastProblem = next
  focusColumn(next)
}

// ---- options ----

const matchOptions = computed(() => [
  { value: 'asset_tag' as MatchBy, label: t('import-options-match-asset-tag') },
  { value: 'serial_number' as MatchBy, label: t('import-options-match-serial') },
  { value: 'none' as MatchBy, label: t('import-options-match-none') },
])
const matchHelp = computed(() => {
  switch (matchBy.value) {
    case 'serial_number':
      return t('import-options-match-serial-help')
    case 'none':
      return t('import-options-match-none-help')
    default:
      return t('import-options-match-asset-tag-help')
  }
})
const fixedMatchText = computed(() =>
  props.job.job_type === 'users'
    ? t('import-options-match-users')
    : t('import-options-match-tickets'),
)
const kindOptions = computed<DropdownOption[]>(() => [
  { value: '', label: t('import-options-default-kind-none') },
  ...props.kinds,
])
const dateOptions = computed(() => [
  { value: 'iso' as DateOrder, label: t('import-options-date-iso') },
  { value: 'dmy' as DateOrder, label: t('import-options-date-dmy') },
  { value: 'mdy' as DateOrder, label: t('import-options-date-mdy') },
])
const decimalOptions = computed(() => [
  { value: '.' as DecimalMark, label: t('import-options-decimal-dot') },
  { value: ',' as DecimalMark, label: t('import-options-decimal-comma') },
])
const showsDefaultKind = computed(() => isAssets.value && !kindMapped.value)
const showsDateOrder = computed(() => mapsValueType(rows.value, props.fields, ['date', 'date_time']))
const showsDecimal = computed(() => mapsValueType(rows.value, props.fields, ['number']))

// A kind column makes the default moot; forget it so it isn't sent.
watch(kindMapped, (mapped) => {
  if (mapped) defaultKind.value = ''
})

// ---- action bar ----

const blocked = computed(() => found.value.length > 0)
const showChangeFile = ref(false)

function check() {
  if (blocked.value) return
  const mapping = buildMapping(rows.value)
  const options: ImportOptions = {
    date_order: dateOrder.value,
    decimal: decimal.value,
  }
  if (isAssets.value) {
    options.match_by = matchBy.value
    if (!kindMapped.value && defaultKind.value) options.default_kind = defaultKind.value
  }
  emit('check', mapping, options)
}

const samplesId = (row: MapRow) => `import-map-samples-${row.index}`
const messageId = (row: MapRow) => `import-map-message-${row.index}`
</script>

<template>
  <section class="flex flex-col gap-6">
    <!-- Summary strip: counts, filter, next problem. -->
    <div
      class="sticky top-0 z-10 bg-app/80 backdrop-blur border-b border-default py-3 flex flex-wrap items-center gap-3"
    >
      <p class="text-sm text-secondary flex flex-wrap items-center gap-x-2" aria-live="polite">
        <span>{{ $t('import-map-summary-mapped', { mapped: tally.mapped, total: tally.total }) }}</span>
        <span aria-hidden="true">·</span>
        <span>{{ $t('import-map-summary-ignored', { count: tally.notImported }) }}</span>
        <template v-if="tally.problems > 0">
          <span aria-hidden="true">·</span>
          <span class="text-status-error">{{ $t('import-map-summary-problems', { count: tally.problems }) }}</span>
        </template>
      </p>
      <div class="ml-auto flex items-center gap-2">
        <Button
          v-if="clashColumns.length > 0"
          variant="ghost"
          size="sm"
          @click="nextProblem"
        >
          {{ $t('import-map-next-problem') }}
        </Button>
        <div v-if="isMobile" class="w-44">
          <BaseDropdown
            v-model="filter"
            :options="filterOptions"
            size="sm"
            :aria-label="$t('import-map-filter-label')"
          />
        </div>
        <SegmentedControl
          v-else
          v-model="filter"
          :options="filterOptions"
          size="sm"
          :aria-label="$t('import-map-filter-label')"
        />
      </div>
    </div>

    <!-- Blocking problems. -->
    <Callout v-if="found.length > 0" severity="error" icon="xCircle">
      <template #header>
        <span class="font-medium text-primary">
          {{ $t('import-map-problems-heading', { count: found.length }) }}
        </span>
      </template>
      <ul class="px-4 py-3 flex flex-col gap-3 text-sm text-primary">
        <li v-for="(p, i) in found" :key="i" class="flex flex-col gap-2">
          <button
            v-if="problemColumn(p) !== null"
            type="button"
            class="text-left hover:underline focus:outline-none focus-visible:ring-1 focus-visible:ring-accent rounded"
            @click="focusColumn(problemColumn(p)!)"
          >
            {{ problemText(p) }}
          </button>
          <span v-else>{{ problemText(p) }}</span>
          <div v-if="p.kind === 'kind'" class="max-w-xs">
            <BaseDropdown
              v-model="defaultKind"
              :options="kindOptions"
              size="sm"
              :label="$t('import-options-default-kind-label')"
              :placeholder="$t('import-options-default-kind-placeholder')"
            />
          </div>
        </li>
      </ul>
    </Callout>

    <!-- Warnings: the import can go ahead. -->
    <Callout v-if="tally.notImported > 0" severity="warning">
      <template #header>
        <span class="font-medium text-primary">{{ $t('import-map-warnings-heading') }}</span>
      </template>
      <p class="px-4 py-3 text-sm text-primary">
        {{ $t('import-map-warning-ignored', { count: tally.notImported }) }}
      </p>
    </Callout>

    <div ref="tableRoot">
      <!-- Desktop: a semantic table. -->
      <table v-if="!isMobile" class="w-full table-fixed text-sm">
        <caption class="sr-only">
          {{ $t('import-map-caption', { count: rows.length, file: job.filename }) }}
        </caption>
        <thead>
          <tr class="text-left text-xs uppercase tracking-wide text-tertiary">
            <th scope="col" class="px-3 py-2 w-[260px] font-medium">{{ $t('import-map-col-source') }}</th>
            <th scope="col" class="px-3 py-2 font-medium">{{ $t('import-map-col-target') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="row in visibleRows"
            :key="row.index"
            :data-map-column="row.index"
            class="border-t border-default"
          >
            <th scope="row" class="px-3 py-2 align-top text-left font-normal">
              <div class="flex flex-col gap-0.5 min-w-0">
                <span
                  class="text-sm font-medium truncate"
                  :class="isNotImported(row) ? 'text-tertiary' : 'text-primary'"
                  :title="row.name"
                >{{ row.name }}</span>
                <span :id="samplesId(row)" class="flex flex-col">
                  <span class="sr-only">{{ $t('import-map-samples-label') }}</span>
                  <span
                    v-for="(sample, i) in row.samples"
                    :key="i"
                    class="text-xs text-tertiary font-mono truncate"
                    :title="sample"
                  >{{ sample }}</span>
                  <span v-if="row.samples.length === 0" class="text-xs text-tertiary italic">
                    {{ $t('import-map-blank-sample') }}
                  </span>
                </span>
              </div>
            </th>
            <td class="px-3 py-2 align-top">
              <div class="flex flex-col gap-1.5">
                <SearchableDropdown
                  v-model="row.target"
                  :options="optionsFor(row)"
                  size="sm"
                  :placeholder="$t('import-map-placeholder')"
                  :invalid="!!clashText(row)"
                  :aria-label="$t('import-map-select-label', { header: row.name })"
                  :described-by="`${samplesId(row)} ${messageId(row)}`"
                />
                <div :id="messageId(row)" class="text-xs flex items-center gap-2 min-h-[1rem]">
                  <StatusPill
                    v-if="pillFor(row)"
                    :label="$t(pillFor(row)!.key)"
                    :tone="pillFor(row)!.tone"
                  />
                  <span v-if="clashText(row)" class="flex items-center gap-1 text-status-error">
                    <Icon name="xCircle" size="sm" />
                    {{ clashText(row) }}
                  </span>
                </div>
              </div>
            </td>
          </tr>
        </tbody>
      </table>

      <!-- Phone: a card per column. -->
      <ul v-else class="flex flex-col gap-3">
        <li
          v-for="row in visibleRows"
          :key="row.index"
          :data-map-column="row.index"
          class="bg-surface border border-default rounded-lg p-3 flex flex-col gap-2"
        >
          <div class="flex flex-col gap-0.5 min-w-0">
            <span
              class="text-sm font-medium truncate"
              :class="isNotImported(row) ? 'text-tertiary' : 'text-primary'"
            >{{ row.name }}</span>
            <span :id="samplesId(row)" class="text-xs text-tertiary font-mono truncate">
              <span class="sr-only">{{ $t('import-map-samples-label') }}</span>
              {{ row.samples.length ? row.samples.join(' · ') : $t('import-map-blank-sample') }}
            </span>
          </div>
          <SearchableDropdown
            v-model="row.target"
            :options="optionsFor(row)"
            size="md"
            :placeholder="$t('import-map-placeholder')"
            :invalid="!!clashText(row)"
            :aria-label="$t('import-map-select-label', { header: row.name })"
            :described-by="`${samplesId(row)} ${messageId(row)}`"
          />
          <div :id="messageId(row)" class="text-xs flex flex-wrap items-center gap-2">
            <StatusPill
              v-if="pillFor(row)"
              :label="$t(pillFor(row)!.key)"
              :tone="pillFor(row)!.tone"
            />
            <span v-if="clashText(row)" class="flex items-center gap-1 text-status-error">
              <Icon name="xCircle" size="sm" />
              {{ clashText(row) }}
            </span>
          </div>
        </li>
      </ul>

      <p v-if="visibleRows.length === 0" class="px-3 py-6 text-sm text-tertiary text-center">
        {{ $t('import-map-empty-filter') }}
      </p>
    </div>

    <!-- Options. -->
    <SectionCard content-padding="p-4">
      <template #title>{{ $t('import-options-heading') }}</template>
      <div class="flex flex-col gap-5">
        <div class="flex flex-col gap-2">
          <span class="text-sm font-medium text-primary">{{ $t('import-options-match-label') }}</span>
          <template v-if="isAssets">
            <SegmentedControl
              v-model="matchBy"
              class="self-start"
              :options="matchOptions"
              size="sm"
              :aria-label="$t('import-options-match-label')"
            />
            <p class="text-xs text-tertiary">{{ matchHelp }}</p>
          </template>
          <p v-else class="text-xs text-tertiary">{{ fixedMatchText }}</p>
        </div>

        <div v-if="showsDefaultKind" class="flex flex-col gap-2 max-w-xs">
          <BaseDropdown
            v-model="defaultKind"
            :options="kindOptions"
            size="sm"
            :label="$t('import-options-default-kind-label')"
            :placeholder="$t('import-options-default-kind-placeholder')"
          />
          <p class="text-xs text-tertiary">{{ $t('import-options-default-kind-help') }}</p>
        </div>

        <div v-if="showsDateOrder" class="flex flex-col gap-2">
          <span class="text-sm font-medium text-primary">{{ $t('import-options-date-label') }}</span>
          <SegmentedControl
            v-model="dateOrder"
            class="self-start"
            :options="dateOptions"
            size="sm"
            :aria-label="$t('import-options-date-label')"
          />
        </div>

        <div v-if="showsDecimal" class="flex flex-col gap-2">
          <span class="text-sm font-medium text-primary">{{ $t('import-options-decimal-label') }}</span>
          <SegmentedControl
            v-model="decimal"
            class="self-start"
            :options="decimalOptions"
            size="sm"
            :aria-label="$t('import-options-decimal-label')"
          />
        </div>
      </div>
    </SectionCard>

    <!-- Action bar. -->
    <div
      class="sticky bottom-0 bg-app/80 backdrop-blur border-t border-default py-3 flex flex-wrap items-center gap-3"
    >
      <Button variant="ghost" @click="showChangeFile = true">
        {{ $t('import-map-change-file') }}
      </Button>
      <span v-if="blocked" class="ml-auto text-xs text-tertiary">
        {{ $t('import-map-blocked', { count: found.length }) }}
      </span>
      <Button
        :class="blocked ? '' : 'ml-auto'"
        :disabled="blocked || checking"
        :loading="checking"
        data-test="import-check"
        @click="check"
      >
        {{ checking
          ? $t('import-map-checking')
          : $t('import-map-check', { count: source.row_count }) }}
      </Button>
    </div>

    <ConfirmModal
      :show="showChangeFile"
      :title="$t('import-map-change-file-title')"
      :message="$t('import-map-change-file-message')"
      :confirm-label="$t('import-map-change-file-confirm')"
      variant="warning"
      @close="showChangeFile = false"
      @confirm="showChangeFile = false; emit('change-file')"
    />
  </section>
</template>
