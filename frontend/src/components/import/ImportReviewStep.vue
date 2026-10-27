<script setup lang="ts">
/**
 * Review step: the checked plan, in the order the admin reads it. The
 * headline and four tiles (New, Updated, Unchanged, Skipped; a tile shows
 * its rows), Top problems with where each is fixed, what the file holds
 * that won't land, catalog entries the import creates, what updates
 * change, and every row's result page by page. Import confirms first.
 *
 * `j` and `k` move a row highlight while focus is outside a field; Enter
 * on a Top problems entry runs its fix. The rules live in
 * `reviewModel.ts`.
 *
 * Done and the import detail show the same cards read-only (`mode`): no
 * headline, problems, fixes or action bar; the catalog card is what the
 * import created; Done leaves out the rows. A past import whose detail
 * was removed shows its tiles only (`counts`).
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQuery } from '@pinia/colada'

import Button from '@/components/common/Button.vue'
import Callout from '@/components/common/Callout.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import Icon from '@/components/common/Icon.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import StatusPill from '@/components/common/StatusPill.vue'
import Tooltip from '@/components/common/Tooltip.vue'
import type { StatusPillTone } from '@/components/common/statusPillTone'
import { useMobileDetection } from '@/composables/useMobileDetection'
import {
  importService,
  type ImportField,
  type ImportJob,
  type ImportRowResult,
  type RowFilter,
} from '@nosdesk/core/services/importService'

import { choiceLabel, localisesChoices } from './choiceLabels'
import {
  PAGE,
  defaultFilter,
  moveHighlight,
  notImported as notImportedOf,
  outcomes,
  reportHasRows,
  problems as problemsOf,
  typingIn,
  type NotImportedLine,
  type Problem,
} from './reviewModel'

const props = defineProps<{
  job: ImportJob
  fields: ImportField[]
  /** The workspace's asset kinds, slug to label. */
  kinds?: { value: string; label: string }[]
  /** `review` (default) is the plan; `done` and `detail` the record;
   *  `counts` the tiles alone. */
  mode?: 'review' | 'done' | 'detail' | 'counts'
  committing?: boolean
  downloading?: boolean
  /** Every value in the mapped columns matched without a question, so
   *  Match values wasn't needed. */
  valuesAllMatched?: boolean
  /** An enabled webhook hears about assets: the confirmation says so. */
  webhooks?: boolean
  /** On a past import, how many fields its updates wrote that were blank
   *  before: What changes says so, for the previous values file. */
  blankBefore?: number
}>()

const emit = defineEmits<{
  (e: 'back'): void
  (e: 'see-values'): void
  (e: 'commit'): void
  (e: 'download-report', only?: { group?: number; dropped?: number[] }): void
  /** Match values, at the card of this field (none: the first). */
  (e: 'fix-values', field: string | null): void
  (e: 'fix-map', field: string | null): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const { isMobile } = useMobileDetection('md')

const lang = typeof document === 'undefined' ? undefined : document.documentElement.lang || undefined
const fmt = (n: number) => new Intl.NumberFormat(lang).format(n)
const list = (items: string[]) =>
  typeof Intl.ListFormat === 'function'
    ? new Intl.ListFormat(lang, { type: 'conjunction' }).format(items)
    : items.join(', ')

const mode = computed(() => props.mode ?? 'review')
const isReview = computed(() => mode.value === 'review')
const showsRows = computed(() => mode.value === 'review' || mode.value === 'detail')
const showsCards = computed(() => mode.value !== 'counts')

const summary = computed(() => props.job.summary!)
const o = computed(() => outcomes(summary.value))
const hasReport = computed(() => reportHasRows(o.value.skipped, summary.value))
const type = computed(() => props.job.job_type)
const isAssets = computed(() => type.value === 'assets')

const fieldById = computed(() => new Map(props.fields.map((f) => [f.id, f])))
const kindLabels = computed(() => new Map((props.kinds ?? []).map((k) => [k.value, k.label])))
const kindLabel = (slug: string) => kindLabels.value.get(slug) ?? slug

const fieldLabel = (f: ImportField) => (f.label_key ? t(f.label_key) : f.label)

/** A field as the admin sees it: their own header first. */
function columnName(column: string | null, source?: string | null): string {
  if (source) return source
  if (!column) return t('import-review-whole-row')
  const field = fieldById.value.get(column)
  return field ? fieldLabel(field) : column
}

/** A field's name in a change, as Review and Done word it. */
function changeName(field: string): string {
  const f = fieldById.value.get(field)
  return f ? fieldLabel(f) : field
}

/** A changed value as the admin reads it; `null` is blank. */
function shown(field: string, value: string | null | undefined): string | null {
  if (value === null || value === undefined || value === '') return null
  if (localisesChoices(type.value, field)) return choiceLabel(t, type.value, field, value, value)
  if (field === 'kind') return kindLabel(value)
  return value
}

// ---- headline ----

const headline = computed(() => {
  const c = o.value
  const parts = [t('import-review-checked', { count: c.total })]
  const bits: string[] = []
  if (c.created > 0) bits.push(t('import-review-created', { count: c.created }))
  if (c.updated > 0) bits.push(t('import-review-updated', { count: c.updated }))
  if (c.imported === 0) parts.push(t('import-review-nothing'))
  else parts.push(t('import-review-will-import', { count: c.imported, parts: bits.join(', ') }))
  if (c.unchanged > 0) parts.push(t('import-review-unchanged', { count: c.unchanged }))
  if (c.skipped > 0) parts.push(t('import-review-skipped', { count: c.skipped }))
  return parts.join(' ')
})

// ---- tiles ----

const tiles = computed<{ filter: RowFilter; key: string; count: number }[]>(() => [
  { filter: 'create', key: 'import-review-tile-new', count: o.value.created },
  { filter: 'update', key: 'import-review-tile-updated', count: o.value.updated },
  { filter: 'unchanged', key: 'import-review-tile-unchanged', count: o.value.unchanged },
  { filter: 'skipped', key: 'import-review-tile-skipped', count: o.value.skipped },
])

// ---- top problems ----

const problems = computed(() => problemsOf(summary.value))

/** What a problem's entry does on Enter: its fix, else its download. */
function runFix(p: Problem) {
  if (p.fix === 'match_values') emit('fix-values', p.column)
  else if (p.fix === 'map') emit('fix-map', p.column)
  else emit('download-report', { group: p.index })
}

/** Notices worded by code; an unknown code shows the server's sentence. The
 *  location snap is What changes' footer. */
const NOTICE_KEYS: Record<string, string> = {
  no_key: 'import-review-notice-no-key',
}
const notices = computed(() =>
  (summary.value.notices ?? [])
    .filter((n) => n.code !== 'location_snapped')
    .map((n) => (NOTICE_KEYS[n.code] ? t(NOTICE_KEYS[n.code], { count: n.count }) : n.message)),
)
const snapped = computed(
  () => (summary.value.notices ?? []).find((n) => n.code === 'location_snapped')?.count ?? 0,
)

// ---- not imported ----

const notImported = computed(() => notImportedOf(props.job, props.fields, fieldLabel))
const notImportedLines = computed(() => notImported.value.lines)

function lineDetail(line: NotImportedLine): string[] {
  switch (line.kind) {
    case 'unmapped':
      return [line.headers.join(', ')]
    case 'ignored':
      return [t('import-review-ni-ignored', { count: line.values })]
    case 'attribute':
      return [
        t('import-review-ni-attribute-rows', { count: line.rows, kind: kindLabel(line.kindSlug) }),
        t('import-review-ni-attribute-reason', { kind: kindLabel(line.kindSlug), header: line.header }),
      ]
    case 'group':
      return [
        t('import-review-ni-group-rows', { value: line.value, count: line.rows }),
        t('import-review-ni-group-reason'),
      ]
    case 'person':
      return [
        t('import-review-ni-person-rows', { values: line.values, count: line.rows }),
        t('import-review-ni-person-reason', { values: line.values }),
      ]
  }
}

function lineTitle(line: NotImportedLine): string {
  return line.kind === 'unmapped'
    ? t('import-review-ni-unmapped', { count: line.columns.length })
    : line.header
}

const dropsRows = (line: NotImportedLine) =>
  line.kind === 'attribute' || line.kind === 'group' || line.kind === 'person'

function downloadLine(line: NotImportedLine) {
  if (line.kind === 'attribute' || line.kind === 'group' || line.kind === 'person') {
    emit('download-report', { dropped: line.dropped })
  }
}

// ---- new in your workspace ----

const catalog = computed(() => summary.value.catalog ?? { manufacturers: [], models: [], groups: [] })
const catalogLines = computed(() => [
  {
    key: 'import-review-new-manufacturers',
    field: 'manufacturer',
    names: catalog.value.manufacturers,
  },
  {
    key: 'import-review-new-models',
    field: 'model',
    names: catalog.value.models.map((m) => (m.manufacturer ? `${m.name} (${m.manufacturer})` : m.name)),
  },
  { key: 'import-review-new-groups', field: 'groups', names: catalog.value.groups },
])
const createsAny = computed(() => catalogLines.value.some((l) => l.names.length > 0))
const firstCreated = computed(() => catalogLines.value.find((l) => l.names.length > 0)?.field ?? null)

// ---- what changes ----

const changes = computed(() => summary.value.changes ?? [])
const newKinds = computed(() =>
  (summary.value.kinds ?? []).map((k) => `${fmt(k.count)} ${kindLabel(k.kind)}`).join(' · '),
)
const showChanges = computed(
  () => o.value.updated > 0 || changes.value.length > 0 || newKinds.value !== '' || snapped.value > 0,
)

// ---- rows ----

const filter = ref<RowFilter>(defaultFilter(o.value))
const offset = ref(0)
const highlight = ref(-1)
watch(filter, () => {
  offset.value = 0
})
watch([filter, offset], () => {
  highlight.value = -1
})

const rowsQuery = useQuery({
  key: () => ['import-rows', props.job.id, props.job.updated_at, filter.value, offset.value],
  query: () =>
    importService.rows(props.job.id, { filter: filter.value, offset: offset.value, limit: PAGE }),
  enabled: () => showsRows.value,
})
const page = computed(() => rowsQuery.data.value)
const rows = computed<ImportRowResult[]>(() => page.value?.rows ?? [])
const total = computed(() => page.value?.total ?? 0)

const filterCounts = computed<Record<RowFilter, number>>(() => ({
  all: o.value.total,
  skipped: o.value.skipped,
  create: o.value.created,
  update: o.value.updated,
  unchanged: o.value.unchanged,
}))
const filterOptions = computed(() =>
  (['all', 'skipped', 'create', 'update', 'unchanged'] as RowFilter[]).map((f) => ({
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

const emptyText = computed(() => {
  if (rowsQuery.isLoading.value) return t('import-review-rows-loading')
  switch (filter.value) {
    case 'skipped':
      return t('import-review-empty-skipped')
    case 'create':
      return t('import-review-empty-new', { type: type.value })
    case 'update':
      return t('import-review-empty-updated')
    default:
      return t('import-review-rows-none')
  }
})

const keyHeader = computed(() => {
  if (type.value === 'users') return t('import-review-col-email')
  return props.job.options?.match_by === 'serial_number'
    ? t('import-review-col-serial')
    : t('import-review-col-tag')
})

function rowProblems(row: ImportRowResult): string {
  return (row.errors ?? [])
    .map((e) => (e.column ? `${columnName(e.column, e.source_column)}: ${e.message}` : e.message))
    .join('; ')
}

/** An updated row's changes, field by field. */
function rowChanges(row: ImportRowResult) {
  return Object.entries(row.fields_written ?? {}).map(([field, c]) => ({
    field,
    name: changeName(field),
    before: shown(field, c.before),
    after: shown(field, c.after),
  }))
}

/** A new row's kind, and the values it leaves out. */
function rowNewDetail(row: ImportRowResult): string {
  const parts: string[] = []
  if (row.kind) parts.push(kindLabel(row.kind))
  const left = [...new Set((row.dropped ?? []).map((d) => columnName(d.column, d.source_column)))]
  if (left.length > 0) parts.push(t('import-review-detail-left-out', { fields: left.join(', ') }))
  return parts.join(' · ')
}

const OUTCOME_TONE: Record<ImportRowResult['outcome'], StatusPillTone> = {
  create: 'positive',
  update: 'info',
  unchanged: 'neutral',
  skipped: 'critical',
}

// ---- keyboard ----

const root = ref<HTMLElement | null>(null)
const showConfirm = ref(false)

async function onKey(e: KeyboardEvent) {
  if (!isReview.value) return
  if (e.key !== 'j' && e.key !== 'k') return
  if (e.metaKey || e.ctrlKey || e.altKey || showConfirm.value) return
  if (typingIn(e.target) || typingIn(document.activeElement)) return
  e.preventDefault()
  highlight.value = moveHighlight(highlight.value, e.key, rows.value.length)
  await nextTick()
  root.value
    ?.querySelector<HTMLElement>(`[data-row-idx="${highlight.value}"]`)
    ?.scrollIntoView?.({ block: 'nearest' })
}

onMounted(() => window.addEventListener('keydown', onKey))
onBeforeUnmount(() => window.removeEventListener('keydown', onKey))

// ---- confirm ----

const confirmMessage = computed(() => {
  const c = o.value
  const parts: string[] = []
  if (c.created > 0 && c.updated > 0) {
    parts.push(t('import-review-confirm-both', { type: type.value, created: c.created, updated: c.updated }))
  } else if (c.created > 0) {
    parts.push(t('import-review-confirm-created', { type: type.value, count: c.created }))
  } else if (c.updated > 0) {
    parts.push(t('import-review-confirm-updated', { type: type.value, count: c.updated }))
  }
  if (c.unchanged > 0) parts.push(t('import-review-confirm-unchanged', { type: type.value, count: c.unchanged }))
  if (c.skipped > 0) parts.push(t('import-review-confirm-skipped', { count: c.skipped }))
  const made = [
    catalog.value.manufacturers.length > 0
      ? t('import-review-count-manufacturers', { count: catalog.value.manufacturers.length })
      : '',
    catalog.value.models.length > 0 ? t('import-review-count-models', { count: catalog.value.models.length }) : '',
    catalog.value.groups.length > 0 ? t('import-review-count-groups', { count: catalog.value.groups.length }) : '',
  ].filter(Boolean)
  if (made.length > 0) parts.push(t('import-review-confirm-catalog', { list: list(made) }))
  if (notImported.value.columns > 0) {
    parts.push(t('import-review-confirm-columns', { count: notImported.value.columns }))
  }
  if (props.webhooks && isAssets.value && c.imported > 0) {
    parts.push(t('import-review-confirm-webhooks', { count: c.imported }))
  }
  return parts.join(' ')
})

function confirm() {
  showConfirm.value = false
  emit('commit')
}

const linkClass =
  'text-accent hover:underline focus:outline-none focus-visible:ring-1 focus-visible:ring-accent rounded'
</script>

<template>
  <section ref="root" class="flex flex-col gap-6" data-test="import-review">
    <p v-if="isReview" class="text-base text-primary" aria-live="polite" data-test="import-review-headline">
      {{ committing ? $t('import-review-importing') : headline }}
    </p>

    <!-- Tiles: the four outcomes; each shows its rows, where there are
         rows. -->
    <div class="grid grid-cols-2 sm:grid-cols-4 gap-3" data-test="import-review-tiles">
      <component
        :is="showsRows ? 'button' : 'div'"
        v-for="tile in tiles"
        :key="tile.filter"
        :type="showsRows ? 'button' : undefined"
        class="bg-surface border rounded-lg p-4 flex flex-col gap-1 text-left transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        :class="showsRows && filter === tile.filter ? 'border-accent ring-1 ring-accent' : showsRows ? 'border-default hover:border-strong' : 'border-default'"
        :aria-pressed="showsRows ? filter === tile.filter : undefined"
        :data-test="`import-tile-${tile.filter}`"
        @click="showsRows && (filter = tile.filter)"
      >
        <span class="text-xs uppercase tracking-wide text-tertiary">{{ $t(tile.key) }}</span>
        <span
          class="text-2xl font-semibold flex items-center gap-2"
          :class="tile.filter === 'skipped' && tile.count > 0 ? 'text-status-error' : 'text-primary'"
        >
          <Icon v-if="tile.filter === 'skipped' && tile.count > 0" name="xCircle" size="md" />
          {{ fmt(tile.count) }}
        </span>
      </component>
    </div>

    <!-- Top problems: each ends with where it is fixed. -->
    <SectionCard v-if="isReview && problems.length > 0" content-padding="" data-test="import-problems">
      <template #title>{{ $t('import-review-top-problems') }}</template>
      <ul class="flex flex-col divide-y divide-default">
        <li
          v-for="p in problems"
          :key="p.index"
          tabindex="0"
          class="flex flex-wrap items-center gap-x-3 gap-y-2 px-3 py-2 text-sm focus:outline-none focus-visible:bg-surface-hover"
          :data-test="`import-problem-${p.index}`"
          :data-fix="p.fix"
          @keydown.enter.self.prevent="runFix(p)"
        >
          <span class="min-w-0 text-primary">
            <span class="font-medium">{{ columnName(p.column, p.source_column) }}:</span>
            {{ p.message }}
            <span class="text-tertiary whitespace-nowrap">· {{ $t('import-review-problem-rows', { count: p.count }) }}</span>
          </span>
          <span class="flex flex-wrap gap-2 sm:ml-auto">
            <Button
              v-if="p.fix === 'match_values'"
              variant="secondary"
              size="sm"
              data-test="import-problem-fix-values"
              @click="emit('fix-values', p.column)"
            >
              {{ $t('import-review-fix-values') }}
            </Button>
            <Button
              v-else-if="p.fix === 'map'"
              variant="secondary"
              size="sm"
              data-test="import-problem-fix-map"
              @click="emit('fix-map', p.column)"
            >
              {{ $t('import-review-fix-map') }}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              icon="download"
              data-test="import-problem-download"
              @click="emit('download-report', { group: p.index })"
            >
              {{ $t('import-review-download-rows') }}
            </Button>
          </span>
        </li>
      </ul>
    </SectionCard>

    <Callout v-if="isReview && notices.length > 0" severity="info">
      <ul class="px-4 py-3 flex flex-col gap-1 text-sm text-secondary">
        <li v-for="(n, i) in notices" :key="i">{{ n }}</li>
      </ul>
    </Callout>

    <!-- Not imported from this file. -->
    <SectionCard v-if="showsCards" content-padding="" data-test="import-not-imported">
      <template #title>
        {{
          notImported.columns > 0
            ? $t('import-review-ni-title-count', { count: notImported.columns })
            : $t('import-review-ni-title')
        }}
      </template>
      <p v-if="notImportedLines.length === 0" class="px-3 py-2 text-sm text-secondary">
        {{ $t('import-review-ni-none') }}
      </p>
      <ul v-else class="flex flex-col divide-y divide-default">
        <li
          v-for="line in notImportedLines"
          :key="line.key"
          class="flex flex-wrap items-center gap-x-3 gap-y-2 px-3 py-2 text-sm"
          :data-test="`import-ni-${line.kind}`"
        >
          <span class="min-w-0 flex flex-wrap gap-x-2">
            <span class="font-medium text-primary">{{ lineTitle(line) }}</span>
            <span v-for="(d, i) in lineDetail(line)" :key="i" class="text-tertiary min-w-0 break-words">· {{ d }}</span>
          </span>
          <Button
            v-if="dropsRows(line)"
            class="sm:ml-auto"
            variant="ghost"
            size="sm"
            icon="download"
            data-test="import-ni-download"
            @click="downloadLine(line)"
          >
            {{ $t('import-review-download-rows') }}
          </Button>
        </li>
      </ul>
    </SectionCard>

    <!-- New in your workspace: catalog entries the import creates. -->
    <SectionCard v-if="showsCards && isAssets" content-padding="" data-test="import-new-in-workspace">
      <template #title>{{ isReview ? $t('import-review-new-title') : $t('import-done-created-title') }}</template>
      <ul class="flex flex-col divide-y divide-default">
        <li v-for="line in catalogLines" :key="line.key" class="px-3 py-2 text-sm break-words">
          <span class="font-medium text-primary">{{ $t(line.key, { count: line.names.length }) }}</span>
          <span v-if="line.names.length > 0" class="text-tertiary"> · {{ line.names.join(', ') }}</span>
        </li>
      </ul>
      <div
        v-if="isReview && createsAny"
        class="flex flex-wrap items-center gap-2 px-3 py-2 border-t border-default bg-surface-alt text-xs text-tertiary"
      >
        <span>{{ $t('import-review-new-not-right') }}</span>
        <button
          type="button"
          :class="linkClass"
          data-test="import-new-change"
          @click="emit('fix-values', firstCreated)"
        >
          {{ $t('import-review-new-change') }}
        </button>
      </div>
    </SectionCard>

    <p
      v-if="isReview && valuesAllMatched && !committing"
      class="text-sm text-secondary flex flex-wrap items-center gap-x-2"
      data-test="import-review-values-matched"
    >
      <span>{{ $t('import-review-values-matched') }}</span>
      <span aria-hidden="true" class="text-tertiary">·</span>
      <button type="button" :class="linkClass" data-test="import-review-see-values" @click="emit('see-values')">
        {{ $t('import-review-values-see') }}
      </button>
    </p>

    <!-- What changes: per field, how many records an update changes. -->
    <SectionCard v-if="showsCards && showChanges" content-padding="" data-test="import-what-changes">
      <template #title>{{ $t('import-review-changes-title') }}</template>
      <ul class="flex flex-col divide-y divide-default">
        <li
          v-for="c in changes"
          :key="c.field"
          class="flex flex-wrap gap-x-2 px-3 py-2 text-sm"
          data-test="import-change"
        >
          <span class="font-medium text-primary">{{ changeName(c.field) }}</span>
          <span class="text-tertiary">· {{ $t('import-review-changes-records', { type: job.job_type, count: c.count }) }}</span>
          <span v-if="c.constant" class="text-tertiary">· {{ $t('import-review-changes-constant') }}</span>
        </li>
        <li v-if="newKinds" class="flex flex-wrap gap-x-2 px-3 py-2 text-sm" data-test="import-change-kinds">
          <span class="font-medium text-primary">{{ $t('import-review-changes-new') }}</span>
          <span class="text-tertiary">· {{ newKinds }}</span>
        </li>
      </ul>
      <p class="px-3 py-2 border-t border-default bg-surface-alt text-xs text-tertiary">
        {{ $t('import-review-changes-blanks') }}
        <template v-if="snapped > 0">{{ $t('import-review-notice-location-snapped', { count: snapped }) }}</template>
        <template v-if="mode === 'detail' && (blankBefore ?? 0) > 0">
          {{ $t('import-review-changes-blank-before', { count: blankBefore ?? 0 }) }}
        </template>
      </p>
    </SectionCard>

    <!-- Rows. -->
    <SectionCard v-if="showsRows" content-padding="" data-test="import-rows">
      <template #title>{{ $t('import-review-rows-heading') }}</template>
      <div class="flex flex-wrap items-center gap-2 px-3 py-2 border-b border-default">
        <div class="min-w-0 max-w-full overflow-x-auto">
          <SegmentedControl
            v-model="filter"
            :options="filterOptions"
            :aria-label="$t('import-review-filter-label')"
            size="sm"
          />
        </div>
        <Tooltip v-if="isReview && !isMobile" :text="$t('import-review-keyboard-help')">
          <button
            type="button"
            class="ml-auto text-xs text-tertiary hover:text-secondary rounded focus:outline-none focus-visible:ring-1 focus-visible:ring-accent"
            data-test="import-review-keyboard"
          >
            {{ $t('import-review-keyboard') }}
          </button>
        </Tooltip>
      </div>

      <p v-if="rows.length === 0" class="px-3 py-4 text-sm text-secondary" data-test="import-rows-empty">
        {{ emptyText }}
      </p>

      <!-- Phone: one card per row. -->
      <ul v-else-if="isMobile" class="flex flex-col gap-2 p-3">
        <li
          v-for="(r, i) in rows"
          :key="r.row"
          :data-row-idx="i"
          class="border rounded-lg p-3 flex flex-col gap-1 text-sm bg-surface"
          :class="highlight === i ? 'border-accent' : 'border-default'"
          :data-highlight="highlight === i ? 'true' : undefined"
          data-test="import-row"
        >
          <div class="flex flex-wrap items-center gap-2">
            <span class="font-mono text-xs text-tertiary">{{ $t('import-review-row-number', { row: r.row }) }}</span>
            <span v-if="r.key" class="font-mono text-xs text-secondary">{{ r.key }}</span>
            <StatusPill :label="$t(`import-review-outcome-${r.outcome}`)" :tone="OUTCOME_TONE[r.outcome]" />
          </div>
          <span v-if="r.label" class="font-medium text-primary break-words">{{ r.label }}</span>
          <span v-if="r.outcome === 'skipped'" class="text-secondary break-words">
            {{ $t('import-review-detail-skipped') }} {{ rowProblems(r) }}
          </span>
          <span v-else-if="r.outcome === 'update'" class="text-secondary break-words">
            {{ $t('import-review-detail-changes') }}
            <template v-for="(c, ci) in rowChanges(r)" :key="c.field">
              <span v-if="ci > 0" class="text-tertiary"> · </span>
              <span class="font-medium text-primary">{{ c.name }}</span>
              {{ ' ' }}<span :class="c.before === null ? 'italic text-tertiary' : ''">{{ c.before ?? $t('import-review-blank') }}</span>
              <span class="text-tertiary" aria-hidden="true"> → </span>
              <span :class="c.after === null ? 'italic text-tertiary' : ''">{{ c.after ?? $t('import-review-blank') }}</span>
            </template>
          </span>
          <span v-else-if="r.outcome === 'unchanged'" class="text-tertiary">{{ $t('import-review-detail-none') }}</span>
          <span v-else-if="rowNewDetail(r)" class="text-tertiary">{{ rowNewDetail(r) }}</span>
        </li>
      </ul>

      <!-- Desktop: a table. -->
      <table v-else class="w-full table-fixed text-sm">
        <thead class="bg-surface-alt">
          <tr class="text-left text-tertiary text-xs uppercase tracking-wide">
            <th scope="col" class="px-3 py-2 w-16 font-medium">{{ $t('import-review-col-row') }}</th>
            <th scope="col" class="px-3 py-2 w-28 font-medium">{{ keyHeader }}</th>
            <th scope="col" class="px-3 py-2 w-36 font-medium">{{ $t('import-review-col-name') }}</th>
            <th scope="col" class="px-3 py-2 w-28 font-medium">{{ $t('import-review-col-outcome') }}</th>
            <th scope="col" class="px-3 py-2 font-medium">{{ $t('import-review-col-detail') }}</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-default">
          <tr
            v-for="(r, i) in rows"
            :key="r.row"
            :data-row-idx="i"
            class="text-primary align-top"
            :class="highlight === i ? 'bg-accent-muted' : ''"
            :data-highlight="highlight === i ? 'true' : undefined"
            data-test="import-row"
          >
            <td class="px-3 py-2 font-mono text-tertiary">{{ r.row }}</td>
            <td class="px-3 py-2 font-mono text-xs break-words">{{ r.key ?? '' }}</td>
            <td class="px-3 py-2 break-words">{{ r.label ?? '' }}</td>
            <td class="px-3 py-2">
              <StatusPill :label="$t(`import-review-outcome-${r.outcome}`)" :tone="OUTCOME_TONE[r.outcome]" />
            </td>
            <td class="px-3 py-2 break-words">
              <span v-if="r.outcome === 'skipped'" class="text-secondary">
                {{ $t('import-review-detail-skipped') }} {{ rowProblems(r) }}
              </span>
              <span v-else-if="r.outcome === 'update'" class="text-secondary">
                {{ $t('import-review-detail-changes') }}
                <template v-for="(c, ci) in rowChanges(r)" :key="c.field">
                  <span v-if="ci > 0" class="text-tertiary"> · </span>
                  <span class="font-medium text-primary">{{ c.name }}</span>
                  {{ ' ' }}<span :class="c.before === null ? 'italic text-tertiary' : ''">{{ c.before ?? $t('import-review-blank') }}</span>
                  <span class="text-tertiary" aria-hidden="true"> → </span>
                  <span :class="c.after === null ? 'italic text-tertiary' : ''">{{ c.after ?? $t('import-review-blank') }}</span>
                </template>
              </span>
              <span v-else-if="r.outcome === 'unchanged'" class="text-tertiary">{{ $t('import-review-detail-none') }}</span>
              <span v-else class="text-tertiary">{{ rowNewDetail(r) }}</span>
            </td>
          </tr>
        </tbody>
      </table>

      <div v-if="total > PAGE" class="flex items-center gap-3 px-3 py-2 border-t border-default text-xs text-tertiary">
        <span>{{ rangeText }}</span>
        <Button
          class="ml-auto"
          variant="ghost"
          size="xs"
          :disabled="offset === 0"
          @click="offset = Math.max(offset - PAGE, 0)"
        >
          {{ $t('import-review-rows-previous') }}
        </Button>
        <Button
          variant="ghost"
          size="xs"
          :disabled="offset + PAGE >= total"
          data-test="import-rows-next"
          @click="offset += PAGE"
        >
          {{ $t('import-review-rows-next') }}
        </Button>
      </div>
    </SectionCard>

    <!-- Phone: the report sits above the bar, which keeps to one row. -->
    <div v-if="isReview && hasReport" class="sm:hidden flex flex-col gap-2">
      <p v-if="o.imported > 0 && o.skipped > 0" class="text-xs text-tertiary">{{ $t('import-review-skip-reason', { count: o.skipped }) }}</p>
      <div>
        <Button
          variant="secondary"
          icon="download"
          :loading="downloading"
          data-test="import-error-report-phone"
          @click="emit('download-report')"
        >
          {{ $t('import-review-download-report') }}
        </Button>
      </div>
    </div>

    <!-- Action bar: sticky, so Import is in reach from any row. -->
    <div v-if="isReview" class="sticky bottom-0 z-10 bg-app border-t border-default py-3 flex items-center gap-3">
      <Button variant="ghost" :disabled="committing" @click="emit('back')">
        {{ $t('import-review-back') }}
      </Button>
      <div class="hidden sm:flex flex-1 min-w-0 items-center justify-end gap-3">
        <span v-if="o.imported > 0 && o.skipped > 0" class="min-w-0 text-right text-xs text-tertiary">
          {{ $t('import-review-skip-reason', { count: o.skipped }) }}
        </span>
        <Button
          v-if="hasReport"
          class="shrink-0"
          variant="secondary"
          icon="download"
          :loading="downloading"
          data-test="import-error-report"
          @click="emit('download-report')"
        >
          {{ $t('import-review-download-report') }}
        </Button>
      </div>
      <Button
        class="ml-auto sm:ml-0 shrink-0"
        :disabled="o.imported === 0 || committing"
        :loading="committing"
        data-test="import-commit"
        @click="showConfirm = true"
      >
        {{
          committing
            ? $t('import-review-importing')
            : o.imported === 0
              ? $t('import-review-nothing-to-import')
              : $t('import-review-import', { count: o.imported })
        }}
      </Button>
    </div>

    <ConfirmModal
      v-if="isReview"
      :show="showConfirm"
      :title="$t('import-review-confirm-title', { count: o.imported })"
      :message="confirmMessage"
      :confirm-label="$t('import-review-import', { count: o.imported })"
      variant="info"
      @close="showConfirm = false"
      @confirm="confirm"
    />
  </section>
</template>
