<script setup lang="ts">
/**
 * Match values step: what each distinct value in a mapped column means.
 * One card per column with something to read, in source order: values
 * with no match first, then Guessed ones, then the rest; values matched
 * without a question are a count behind "Show". Every value is pre-filled,
 * so the step is for confirming the uncertain few. "Check again" stores
 * the answers and checks the rows with them. The rules live in
 * `valuesModel.ts`.
 *
 * Lookup cards link values to the catalog (manufacturer, model, groups) or
 * to members (primary user). Nothing is created unless the admin picks
 * Create on a value or a card. A model is answered under the row's
 * manufacturer, so a manufacturer answer reads the Model card again
 * (`POST values/preview`) before the rows are checked.
 */
import { computed, nextTick, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQuery } from '@pinia/colada'

import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'
import SearchableDropdown from '@/components/common/SearchableDropdown.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import StatusPill from '@/components/common/StatusPill.vue'
import Tooltip from '@/components/common/Tooltip.vue'
import type { DropdownOption } from '@/components/common/dropdownOption'
import { useMobileDetection } from '@/composables/useMobileDetection'
import { useAuthStore } from '@/stores/auth'
import { formatCompactDate } from '@nosdesk/core/utils/dateUtils'
import { assetStatusLabel } from '@/utils/assetStatusMeta'
import {
  importService,
  importValuesPreviewQueryKey,
  type ImportField,
  type ImportValue,
  type ImportValueColumn,
  type ImportValues,
  type ValueAnswer,
  type ValueAnswers,
} from '@nosdesk/core/services/importService'

import {
  PAGE,
  answerFromKey,
  answerKey,
  answersToSend,
  applyBulk,
  applyScale,
  bulkActions,
  bulkKinds,
  canCreate,
  cardMatched,
  cardMatchedCount,
  cards,
  columnsNeeding,
  consequence,
  currentAnswer,
  displayOrder,
  emptyReplacements,
  entryId,
  filterValues,
  groupColumns,
  isAnswered,
  isModel,
  leavesAsIs,
  makerEditsKey,
  makerName,
  makersAnswered,
  nextProblem as nextProblemId,
  previewAnswers,
  problemIds,
  scaleChoice,
  scaleKinds,
  settledColumns,
  skippedNow,
  idOf,
  valuesCsv,
  withModels,
  type BulkAction,
  type Card,
  type Edits,
  type Entry,
  type ValueFilter,
} from './valuesModel'

const props = defineProps<{
  view: ImportValues
  fields: ImportField[]
  checking: boolean
  jobId: string
  /** The job's `updated_at`: the check the view was read from. */
  version: string
}>()

const emit = defineEmits<{
  (e: 'check', answers: ValueAnswers): void
  (e: 'back'): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const { isMobile } = useMobileDetection('md')
const authStore = useAuthStore()

const lang = typeof document === 'undefined' ? undefined : document.documentElement.lang || undefined
const fmt = (n: number) => new Intl.NumberFormat(lang).format(n)

const edits = ref<Edits>({})
const filters = ref<Record<string, ValueFilter>>({})
const expanded = ref<Record<string, boolean>>({})
const showMatched = ref<Record<string, boolean>>({})
const tried = ref(false)

// ---- the Model card under this screen's manufacturer answers ----

/** Manufacturer edits the server hasn't matched models under, settled for
 *  a moment so a run of picks reads the card once. */
const makerKey = ref('')
let makerTimer: ReturnType<typeof setTimeout> | undefined
watch(
  () => (props.view.columns.some(isModel) ? makerEditsKey(props.view, edits.value) : ''),
  (key) => {
    clearTimeout(makerTimer)
    if (!key) {
      makerKey.value = ''
      return
    }
    makerTimer = setTimeout(() => {
      makerKey.value = key
    }, 300)
  },
)

const previewQuery = useQuery({
  key: () => importValuesPreviewQueryKey(props.jobId, props.version, makerKey.value),
  query: () => importService.previewValues(props.jobId, previewAnswers(props.view, edits.value)),
  enabled: () => makerKey.value !== '',
  // The card as it was stays up while the next answers are read.
  placeholderData: (previous) => previous,
})

/** The view with its Model card read under this screen's manufacturer
 *  answers, once they are. */
const current = computed<ImportValues>(() =>
  makerKey.value ? withModels(props.view, previewQuery.data.value) : props.view,
)

const shown = computed<Card[]>(() => cards(current.value, edits.value))
const settled = computed(() => settledColumns(current.value, edits.value))
const needing = computed(() => columnsNeeding(current.value, edits.value))
const skipped = computed(() => skippedNow(current.value, edits.value))
const order = computed(() => displayOrder(shown.value))
const problems = computed(() => problemIds(shown.value, edits.value))
const unfinished = computed(() => emptyReplacements(current.value, edits.value))

const fieldById = computed(() => new Map(props.fields.map((f) => [f.id, f])))

function fieldLabel(id: string): string {
  const field = fieldById.value.get(id)
  if (!field) return id
  return field.label_key ? t(field.label_key) : field.label
}

function title(col: ImportValueColumn): string {
  const args = { header: col.header, field: fieldLabel(col.field) }
  return col.context_label && !isModel(col)
    ? t('import-values-card-title-kind', { ...args, kind: col.context_label })
    : t('import-values-card-title', args)
}

/** The Model card's line saying it follows the manufacturer answers. */
const recomputedNote = computed(() => {
  if (!makersAnswered(current.value, edits.value)) return ''
  const makers = current.value.columns.find((c) => c.lookup === 'manufacturer')
  return makers ? t('import-values-recomputed', { header: makers.header }) : ''
})

function cardNote(card: Card): string {
  return isModel(card.column) ? recomputedNote.value : ''
}

const isParsed = (col: ImportValueColumn) => !col.lookup && col.choices.length === 0

/** A choice's label: an asset status in the viewer's language, else the
 *  server's. */
function choiceLabel(col: ImportValueColumn, value: string, label: string): string {
  return col.field === 'status' ? assetStatusLabel(t, value) : label
}

/** Why a choice can't be picked, in the viewer's language for a status. */
function disabledNote(col: ImportValueColumn, note: string): string {
  return col.field === 'status' ? t('import-values-status-after-import') : note
}

/** The manufacturer a model value is answered under, for its "under" line;
 *  empty for rows without one. */
function underOf(col: ImportValueColumn): string {
  return isModel(col) ? makerName(current.value, col) : ''
}

// ---- answers ----

function answerOf(col: ImportValueColumn, v: ImportValue): ValueAnswer | undefined {
  return currentAnswer(col, v, edits.value)
}

function setAnswer(col: ImportValueColumn, v: ImportValue, key: string) {
  const id = idOf(col, v)
  edits.value = { ...edits.value, [id]: answerFromKey(key, answerOf(col, v)) }
}

function replacementOf(col: ImportValueColumn, v: ImportValue): string {
  const a = answerOf(col, v)
  return a?.action === 'replace' ? a.value : ''
}

function setReplacement(col: ImportValueColumn, v: ImportValue, value: string) {
  edits.value = { ...edits.value, [idOf(col, v)]: { action: 'replace', value } }
}

/** The Create entry's label: under the model's manufacturer, or "anyway"
 *  beside a close existing name. */
function createLabel(col: ImportValueColumn, v: ImportValue): string {
  const under = underOf(col)
  if (under && canCreate(col, v)) return t('import-values-create-under', { value: v.text, manufacturer: under })
  if (v.reason?.code === 'close_to') return t('import-values-create-anyway', { value: v.text })
  return t('import-values-create', { value: v.text })
}

function lookupOptions(col: ImportValueColumn, v: ImportValue): DropdownOption[] {
  const or = t('import-values-group-or')
  const options: DropdownOption[] = col.choices.map((c) => ({
    value: `match:${c.value}`,
    label: c.label,
    description: c.disabled ?? c.detail,
    disabled: !!c.disabled,
  }))
  const now = answerOf(col, v)?.action
  if (col.lookup !== 'person' && v.reason?.code !== 'ambiguous') {
    const needsMaker = isModel(col) && !canCreate(col, v)
    options.push({
      value: 'create',
      label: createLabel(col, v),
      description: needsMaker ? t('import-values-needs-maker') : undefined,
      disabled: needsMaker,
      group: or,
    })
  }
  if (col.lookup === 'manufacturer' || col.lookup === 'model') {
    options.push({ value: 'keep_text', label: t('import-values-keep-text'), group: or })
    // Not offered, but kept readable when the job holds it.
    if (now === 'leave_blank') {
      options.push({ value: 'leave_blank', label: t('import-values-leave-blank'), group: or })
    }
  } else {
    options.push({
      value: 'leave_blank',
      label: col.lookup === 'person' ? t('import-values-leave-unassigned') : t('import-values-leave-off'),
      group: or,
    })
  }
  options.push({ value: 'skip', label: t('import-values-skip'), group: or })
  return options
}

function pickerOptions(col: ImportValueColumn, v: ImportValue): DropdownOption[] {
  if (col.lookup) return lookupOptions(col, v)
  const options: DropdownOption[] = col.choices.map((c) => ({
    value: `match:${c.value}`,
    label: choiceLabel(col, c.value, c.label),
    description: c.disabled ? disabledNote(col, c.disabled) : c.detail,
    disabled: !!c.disabled,
  }))
  const or = t('import-values-group-or')
  if (isParsed(col)) options.push({ value: 'replace', label: t('import-values-replace') })
  if (v.blank) {
    options.push({
      value: 'leave_blank',
      label: t('import-values-leave-as-is'),
      description: t('import-values-leave-as-is-help'),
      group: or,
    })
  } else if (leavesAsIs(col)) {
    options.push({
      value: 'leave_blank',
      label: t('import-values-leave-as-is'),
      description: t('import-values-status-leave-help'),
      group: or,
    })
  } else if (!col.required) {
    options.push({ value: 'leave_blank', label: t('import-values-leave-blank'), group: or })
  }
  options.push({ value: 'skip', label: t('import-values-skip'), group: or })
  return options
}

/** What a matched value became, for the read-only list. */
function answerLabel(col: ImportValueColumn, v: ImportValue): string {
  const a = answerOf(col, v)
  if (!a) return v.text
  switch (a.action) {
    case 'match': {
      const choice = col.choices.find((c) => c.value === String(a.target))
      return choice ? choiceLabel(col, choice.value, choice.label) : String(a.target)
    }
    case 'leave_blank':
      if (col.lookup === 'group') return t('import-values-leave-off')
      if (col.lookup === 'person') return t('import-values-leave-unassigned')
      return v.blank || leavesAsIs(col) ? t('import-values-leave-as-is') : t('import-values-leave-blank')
    case 'keep_text':
      return t('import-values-keep-text')
    case 'create':
      return createLabel(col, v)
    case 'skip':
      return t('import-values-skip')
    case 'replace':
      return a.value
    default:
      return ''
  }
}

type Pill = 'guessed' | 'saved' | 'no_match' | null

function pillOf(col: ImportValueColumn, v: ImportValue): Pill {
  if (isAnswered(col, v, edits.value)) return null
  if (v.match === 'guessed' || v.match === 'saved' || v.match === 'no_match') return v.match
  return null
}

function savedText(v: ImportValue): string {
  const p = v.saved
  if (!p) return ''
  const args = { date: formatCompactDate(p.at) }
  if (p.by && p.by === authStore.user?.uuid) return t('import-map-saved-you', args)
  if (p.by_name) return t('import-map-saved-from', { ...args, name: p.by_name })
  return t('import-map-saved-unknown', args)
}

/** Whether another value of the card would be created under the same
 *  spelling rule as this one: both answered Create. */
function bothCreated(card: Card, e: Entry): boolean {
  if (e.v.reason?.code !== 'both_new' || answerOf(e.col, e.v)?.action !== 'create') return false
  const twin = e.v.reason.text
  return card.listed.some((o) => o.v.text === twin && answerOf(o.col, o.v)?.action === 'create')
}

function notesOf(card: Card, e: Entry): string[] {
  const { col, v } = e
  const reason = v.reason
  switch (reason?.code) {
    case 'close_to':
      return [t('import-values-close-to', { label: reason.label })]
    case 'disabled':
      return [disabledNote(col, reason.note)]
    case 'ambiguous':
      return [t('import-values-ambiguous', { labels: reason.labels.join(', ') })]
    case 'both_new':
      return bothCreated(card, e)
        ? [t('import-values-close-to', { label: reason.text }), t('import-values-both-new')]
        : [t('import-values-close-to', { label: reason.text })]
    case 'needs_manufacturer':
      return [t('import-values-needs-maker')]
    default:
      return []
  }
}

const CONSEQUENCE_KEYS = {
  skipped: 'import-values-consequence-skipped',
  blank: 'import-values-consequence-blank',
  as_is: 'import-values-consequence-as-is',
  text: 'import-values-consequence-text',
  off: 'import-values-consequence-off',
  unassigned: 'import-values-consequence-unassigned',
  created: 'import-values-consequence-created',
} as const

function consequenceText(col: ImportValueColumn, v: ImportValue): string {
  const c = consequence(col, v, edits.value)
  return c ? t(CONSEQUENCE_KEYS[c]) : ''
}

function valueText(v: ImportValue): string {
  return v.blank ? t('import-values-blank') : v.text
}

// ---- cards ----

function filterOf(card: Card): ValueFilter {
  return filters.value[card.id] ?? 'all'
}

function setFilter(card: Card, value: ValueFilter) {
  filters.value = { ...filters.value, [card.id]: value }
}

function filterOptions(card: Card) {
  const count = (m: ImportValue['match']) => card.listed.filter((e) => e.v.match === m).length
  return [
    { value: 'no_match' as ValueFilter, label: t('import-values-filter-no-match', { count: count('no_match') }) },
    { value: 'guessed' as ValueFilter, label: t('import-values-filter-guessed', { count: count('guessed') }) },
    { value: 'all' as ValueFilter, label: t('import-values-filter-all') },
  ]
}

/** Values above which a card offers the filter. */
const FILTER_MIN = 4

function filtered(card: Card): Entry[] {
  return filterValues(card.listed, filterOf(card))
}

function visible(card: Card): Entry[] {
  const list = filtered(card)
  return expanded.value[card.id] ? list : list.slice(0, PAGE)
}

function hiddenCount(card: Card): number {
  return filtered(card).length - visible(card).length
}

function bulkFor(card: Card) {
  return bulkActions(card, edits.value)
}

function bulkLabel(col: ImportValueColumn, action: BulkAction, count: number): string {
  switch (action) {
    case 'skip':
      return t('import-values-bulk-skip', { count })
    case 'keep_text':
      return t('import-values-bulk-keep-text', { count })
    case 'create':
      if (col.lookup === 'model') return t('import-values-bulk-create-models', { count })
      if (col.lookup === 'group') return t('import-values-bulk-create-groups', { count })
      return t('import-values-bulk-create-manufacturers', { count })
    case 'leave_blank':
      if (col.lookup === 'group') return t('import-values-bulk-off', { count })
      if (col.lookup === 'person') return t('import-values-bulk-unassigned', { count })
      return leavesAsIs(col)
        ? t('import-values-bulk-as-is', { count })
        : t('import-values-bulk-blank', { count })
  }
}

function bulk(card: Card, action: BulkAction) {
  edits.value = applyBulk(card, edits.value, action)
}

/** A scaled card's choice in words. */
function scaleLabel(col: ImportValueColumn, action: BulkAction): string {
  switch (action) {
    case 'skip':
      return t('import-values-skip')
    case 'keep_text':
      return t('import-values-keep-text')
    case 'create':
      return t('import-values-scale-create')
    case 'leave_blank':
      if (col.lookup === 'group') return t('import-values-leave-off')
      if (col.lookup === 'person') return t('import-values-leave-unassigned')
      return leavesAsIs(col) ? t('import-values-leave-as-is') : t('import-values-leave-blank')
  }
}

function scaleOptions(card: Card) {
  return scaleKinds(card.column).map((value) => ({ value, label: scaleLabel(card.column, value) }))
}

function setScaleChoice(card: Card, action: BulkAction | '') {
  if (!action) return
  edits.value = applyScale(card, edits.value, action)
}

function download(card: Card) {
  const csv = valuesCsv(
    [t('import-values-col-value'), t('import-values-col-rows')],
    card.listed.map((e) => e.v),
  )
  const link = document.createElement('a')
  link.href = URL.createObjectURL(new Blob([csv], { type: 'text/csv;charset=utf-8' }))
  const stem = card.column.header || card.column.field
  link.download =
    card.column.context && !isModel(card.column)
      ? `${stem}-${card.column.context}-values.csv`
      : `${stem}-values.csv`
  document.body.appendChild(link)
  link.click()
  document.body.removeChild(link)
  URL.revokeObjectURL(link.href)
}

function toggleMatched(id: string) {
  showMatched.value = { ...showMatched.value, [id]: !showMatched.value[id] }
}

/** Settled columns as cards: when nothing asks to be read, the step still
 *  shows what every value became. */
const settledCards = computed<Card[]>(() =>
  shown.value.length === 0
    ? groupColumns(settled.value).map((columns) => ({
        id: `settled:${columns[0].field}:${columns[0].index}`,
        column: columns[0],
        columns,
        listed: [],
        scaled: false,
      }))
    : [],
)

const settledText = computed(() =>
  t('import-values-all-matched', {
    headers: [...new Set(settled.value.map((c) => c.header))].join(', '),
  }),
)

// ---- next problem ----

const root = ref<HTMLElement | null>(null)
let lastProblem: string | null = null

async function nextProblem() {
  const id = nextProblemId(order.value, problems.value, lastProblem)
  if (!id) return
  lastProblem = id
  const card = shown.value.find((c) => c.listed.some((e) => entryId(e) === id))
  if (card) {
    const index = filtered(card).findIndex((e) => entryId(e) === id)
    if (index < 0) setFilter(card, 'all')
    if (index < 0 || index >= PAGE) expanded.value = { ...expanded.value, [card.id]: true }
    await nextTick()
  }
  const row = [...(root.value?.querySelectorAll<HTMLElement>('[data-value-id]') ?? [])].find(
    (el) => el.dataset.valueId === id,
  )
  const trigger = row?.querySelector<HTMLElement>('[data-value-picker]')
  row?.scrollIntoView?.({ block: 'center', behavior: 'smooth' })
  trigger?.focus({ preventScroll: true })
}

// ---- action bar ----

function checkAgain() {
  tried.value = true
  if (unfinished.value.length > 0) return
  emit('check', answersToSend(current.value, edits.value))
}

const linkClass =
  'text-accent hover:underline focus:outline-none focus-visible:ring-1 focus-visible:ring-accent rounded'
const domId = (col: ImportValueColumn, v: ImportValue) =>
  `import-value-${col.index}-${encodeURIComponent(col.context ?? '')}-${encodeURIComponent(v.key)}`.replace(/%/g, '_')
</script>

<template>
  <section ref="root" class="flex flex-col gap-6" data-test="import-values">
    <!-- Strip: live counts and the next value to read. Opaque, so cards
         scrolled under it don't show through. -->
    <div
      data-test="import-values-strip"
      class="sticky top-0 z-10 bg-app border-b border-default py-2 sm:py-3 flex flex-wrap items-center gap-x-3 gap-y-2"
    >
      <p class="text-sm text-secondary flex flex-wrap items-center gap-x-2" aria-live="polite">
        <span>{{ $t('import-values-strip-checked', { count: view.rows_checked }) }}</span>
        <span aria-hidden="true">·</span>
        <span data-test="import-values-needing">{{ $t('import-values-strip-columns', { count: needing }) }}</span>
        <span aria-hidden="true">·</span>
        <span
          data-test="import-values-skipped"
          :class="skipped > 0 ? 'text-status-error' : ''"
        >{{ skipped > 0
          ? $t('import-values-strip-skipped', { count: skipped })
          : $t('import-values-strip-none-skipped') }}</span>
      </p>
      <Button
        v-if="problems.length > 0"
        class="ml-auto"
        variant="ghost"
        size="sm"
        data-test="import-values-next"
        @click="nextProblem"
      >
        {{ $t('import-map-next-problem') }}
      </Button>
    </div>

    <template v-for="card in shown" :key="card.id">
      <!-- Too many values to list: counts and one answer for the rest. -->
      <SectionCard
        v-if="card.scaled"
        content-padding="p-4"
        data-test="import-values-scale"
        :data-field="card.column.field"
      >
        <template #title>{{ title(card.column) }}</template>
        <div class="flex flex-col gap-3">
          <p class="text-sm text-primary">
            {{ $t('import-values-scale', { total: card.columns.reduce((n, c) => n + c.distinct, 0), rest: card.listed.length }) }}
          </p>
          <SegmentedControl
            class="self-start"
            :model-value="scaleChoice(card, edits)"
            :options="scaleOptions(card)"
            size="sm"
            :aria-label="$t('import-values-scale-label')"
            @update:model-value="setScaleChoice(card, $event)"
          />
          <div>
            <Button variant="secondary" size="sm" icon="download" @click="download(card)">
              {{ $t('import-values-scale-download') }}
            </Button>
          </div>
        </div>
      </SectionCard>

      <SectionCard
        v-else
        content-padding=""
        data-test="import-values-card"
        :data-field="card.column.field"
      >
        <template #title>{{ title(card.column) }}</template>
        <template v-if="cardNote(card) && !isMobile" #headerActions>
          <span class="text-xs text-tertiary" data-test="import-values-card-note">{{ cardNote(card) }}</span>
        </template>
        <div class="flex flex-col">
          <p
            v-if="cardNote(card) && isMobile"
            class="px-3 pt-3 text-xs text-tertiary"
            data-test="import-values-card-note"
          >
            {{ cardNote(card) }}
          </p>
          <div v-if="card.listed.length >= FILTER_MIN" class="px-3 pt-3 flex">
            <SegmentedControl
              :model-value="filterOf(card)"
              :options="filterOptions(card)"
              size="sm"
              :aria-label="$t('import-values-filter-label')"
              @update:model-value="setFilter(card, $event)"
            />
          </div>

          <!-- Desktop: a semantic table. -->
          <table v-if="!isMobile && visible(card).length > 0" class="w-full table-fixed text-sm">
            <thead>
              <tr class="text-left text-xs uppercase tracking-wide text-tertiary">
                <th scope="col" class="px-3 py-2 font-medium">{{ $t('import-values-col-value') }}</th>
                <th scope="col" class="px-3 py-2 w-20 font-medium text-right">{{ $t('import-values-col-rows') }}</th>
                <th scope="col" class="px-3 py-2 w-[240px] font-medium">{{ $t('import-values-col-becomes') }}</th>
                <th scope="col" class="px-3 py-2 w-[200px] font-medium"><span class="sr-only">{{ $t('import-values-col-status') }}</span></th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="e in visible(card)"
                :key="entryId(e)"
                :data-value-id="entryId(e)"
                data-test="import-value-row"
                class="border-t border-default"
              >
                <th scope="row" class="px-3 py-2 align-top text-left font-normal">
                  <span
                    class="block truncate"
                    :class="e.v.blank ? 'text-tertiary italic' : 'font-mono text-primary'"
                    :title="e.v.text"
                  >{{ valueText(e.v) }}</span>
                  <span
                    v-if="underOf(e.col)"
                    class="block truncate text-xs text-tertiary"
                    data-test="import-value-under"
                  >{{ $t('import-values-under', { manufacturer: underOf(e.col) }) }}</span>
                </th>
                <td class="px-3 py-2 align-top text-right text-tertiary tabular-nums">{{ fmt(e.v.rows.length) }}</td>
                <td class="px-3 py-2 align-top">
                  <div class="flex flex-col gap-1.5">
                    <SearchableDropdown
                      :model-value="answerKey(answerOf(e.col, e.v))"
                      :options="pickerOptions(e.col, e.v)"
                      size="sm"
                      :aria-label="$t('import-values-picker-label', { value: valueText(e.v) })"
                      :described-by="domId(e.col, e.v)"
                      data-value-picker
                      @update:model-value="setAnswer(e.col, e.v, $event)"
                    />
                    <FormInput
                      v-if="answerOf(e.col, e.v)?.action === 'replace'"
                      :model-value="replacementOf(e.col, e.v)"
                      :inputmode="e.col.value_type === 'number' ? 'decimal' : undefined"
                      size="sm"
                      :placeholder="$t(e.col.value_type === 'date' ? 'import-values-replace-date' : 'import-values-replace-number')"
                      :aria-label="$t('import-values-replace-label', { value: valueText(e.v) })"
                      :error="tried && !replacementOf(e.col, e.v).trim() ? $t('import-values-replace-needed') : undefined"
                      data-test="import-value-replace"
                      @update:model-value="setReplacement(e.col, e.v, String($event ?? ''))"
                    />
                  </div>
                </td>
                <td :id="domId(e.col, e.v)" class="px-3 py-2 align-top">
                  <div class="flex flex-col gap-1 text-xs">
                    <div class="flex flex-wrap items-center gap-2 min-h-[1.5rem]">
                      <StatusPill
                        v-if="pillOf(e.col, e.v) === 'no_match'"
                        data-test="pill-no-match"
                        :label="$t('import-values-pill-no-match')"
                        tone="critical"
                      />
                      <StatusPill
                        v-else-if="pillOf(e.col, e.v) === 'guessed'"
                        data-test="pill-guessed"
                        :label="$t('import-map-pill-guessed')"
                        tone="caution"
                      />
                      <Tooltip v-else-if="pillOf(e.col, e.v) === 'saved'" :text="savedText(e.v)">
                        <span
                          tabindex="0"
                          class="inline-flex rounded focus:outline-none focus-visible:ring-1 focus-visible:ring-accent"
                          data-test="pill-saved"
                        >
                          <StatusPill :label="$t('import-map-pill-saved')" tone="neutral" :dot="false" />
                          <span class="sr-only">{{ savedText(e.v) }}</span>
                        </span>
                      </Tooltip>
                      <span
                        v-if="consequenceText(e.col, e.v)"
                        data-test="import-value-consequence"
                        :class="consequence(e.col, e.v, edits) === 'skipped' ? 'text-status-error' : 'text-tertiary'"
                      >{{ consequenceText(e.col, e.v) }}</span>
                    </div>
                    <span
                      v-for="note in notesOf(card, e)"
                      :key="note"
                      class="text-tertiary"
                      data-test="import-value-note"
                    >{{ note }}</span>
                  </div>
                </td>
              </tr>
            </tbody>
          </table>

          <!-- Phone: one item per value. -->
          <ul v-else-if="visible(card).length > 0" class="flex flex-col">
            <li
              v-for="e in visible(card)"
              :key="entryId(e)"
              :data-value-id="entryId(e)"
              data-test="import-value-row"
              class="border-t border-default first:border-t-0 p-3 flex flex-col gap-2"
            >
              <div class="flex flex-wrap items-center gap-x-2 gap-y-1 min-w-0">
                <span
                  class="text-sm truncate"
                  :class="e.v.blank ? 'text-tertiary italic' : 'font-mono font-medium text-primary'"
                >{{ valueText(e.v) }}</span>
                <span class="text-xs text-tertiary">
                  {{ $t('import-values-row-count', { count: e.v.rows.length }) }}<template v-if="underOf(e.col)"> · {{ $t('import-values-under', { manufacturer: underOf(e.col) }) }}</template>
                </span>
                <StatusPill
                  v-if="pillOf(e.col, e.v) === 'no_match'"
                  data-test="pill-no-match"
                  :label="$t('import-values-pill-no-match')"
                  tone="critical"
                />
                <StatusPill
                  v-else-if="pillOf(e.col, e.v) === 'guessed'"
                  data-test="pill-guessed"
                  :label="$t('import-map-pill-guessed')"
                  tone="caution"
                />
                <StatusPill
                  v-else-if="pillOf(e.col, e.v) === 'saved'"
                  data-test="pill-saved"
                  :label="$t('import-map-pill-saved')"
                  tone="neutral"
                  :dot="false"
                />
              </div>
              <SearchableDropdown
                :model-value="answerKey(answerOf(e.col, e.v))"
                :options="pickerOptions(e.col, e.v)"
                size="md"
                :aria-label="$t('import-values-picker-label', { value: valueText(e.v) })"
                :described-by="domId(e.col, e.v)"
                data-value-picker
                @update:model-value="setAnswer(e.col, e.v, $event)"
              />
              <FormInput
                v-if="answerOf(e.col, e.v)?.action === 'replace'"
                :model-value="replacementOf(e.col, e.v)"
                :inputmode="e.col.value_type === 'number' ? 'decimal' : undefined"
                size="md"
                :placeholder="$t(e.col.value_type === 'date' ? 'import-values-replace-date' : 'import-values-replace-number')"
                :aria-label="$t('import-values-replace-label', { value: valueText(e.v) })"
                :error="tried && !replacementOf(e.col, e.v).trim() ? $t('import-values-replace-needed') : undefined"
                data-test="import-value-replace"
                @update:model-value="setReplacement(e.col, e.v, String($event ?? ''))"
              />
              <div :id="domId(e.col, e.v)" class="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
                <span
                  v-if="consequenceText(e.col, e.v)"
                  :class="consequence(e.col, e.v, edits) === 'skipped' ? 'text-status-error' : 'text-tertiary'"
                >{{ consequenceText(e.col, e.v) }}</span>
                <span v-for="note in notesOf(card, e)" :key="note" class="text-tertiary">{{ note }}</span>
                <span v-if="pillOf(e.col, e.v) === 'saved'" class="text-tertiary">{{ savedText(e.v) }}</span>
              </div>
            </li>
          </ul>

          <p v-else class="px-3 py-6 text-sm text-tertiary text-center">
            {{ $t('import-values-empty-filter') }}
          </p>

          <div
            v-if="hiddenCount(card) > 0"
            class="px-3 py-2 border-t border-default text-sm"
          >
            <button
              type="button"
              :class="linkClass"
              data-test="import-values-more"
              @click="expanded = { ...expanded, [card.id]: true }"
            >
              {{ $t('import-values-show-more', { count: hiddenCount(card) }) }}
            </button>
          </div>

          <!-- Matched without a question: a count, the list behind Show. -->
          <div
            v-if="cardMatchedCount(card, edits) > 0"
            class="px-3 py-2 border-t border-default text-sm text-secondary flex flex-wrap items-center gap-x-2"
            data-test="import-values-matched"
          >
            <span>{{ $t('import-values-matched', { count: cardMatchedCount(card, edits) }) }}</span>
            <template v-if="cardMatched(card, edits).length > 0">
              <span aria-hidden="true" class="text-tertiary">·</span>
              <button
                type="button"
                :class="linkClass"
                :aria-expanded="!!showMatched[card.id]"
                @click="toggleMatched(card.id)"
              >
                {{ showMatched[card.id] ? $t('import-values-hide') : $t('import-values-show') }}
              </button>
            </template>
          </div>
          <ul
            v-if="showMatched[card.id]"
            class="px-3 pb-3 flex flex-col gap-1 text-sm"
            data-test="import-values-matched-list"
          >
            <li
              v-for="e in cardMatched(card, edits)"
              :key="entryId(e)"
              class="flex flex-wrap items-center gap-x-3 gap-y-1"
            >
              <span class="font-mono text-primary">{{ valueText(e.v) }}</span>
              <span class="text-xs text-tertiary">{{ $t('import-values-row-count', { count: e.v.rows.length }) }}<template v-if="underOf(e.col)"> · {{ $t('import-values-under', { manufacturer: underOf(e.col) }) }}</template></span>
              <span class="text-secondary">→ {{ answerLabel(e.col, e.v) }}</span>
              <StatusPill v-if="e.v.match === 'saved'" :label="$t('import-map-pill-saved')" tone="neutral" :dot="false" />
            </li>
          </ul>

          <!-- Bulk answers: the card's main one stands out. -->
          <div
            v-if="bulkFor(card).length > 0"
            class="px-3 py-2 border-t border-default bg-surface-alt flex flex-wrap items-center gap-2"
            data-test="import-values-bulk"
          >
            <Button
              v-for="b in bulkFor(card)"
              :key="b.action"
              :variant="b.action === bulkKinds(card.column)[0] ? 'secondary' : 'ghost'"
              size="sm"
              :data-bulk="b.action"
              @click="bulk(card, b.action)"
            >
              {{ bulkLabel(card.column, b.action, b.count) }}
            </Button>
          </div>
        </div>
      </SectionCard>
    </template>

    <!-- Nothing to read: what every value became, column by column. -->
    <SectionCard
      v-for="card in settledCards"
      :key="card.id"
      content-padding=""
      data-test="import-values-settled-card"
    >
      <template #title>{{ title(card.column) }}</template>
      <div class="px-3 py-2 text-sm text-secondary flex flex-wrap items-center gap-x-2">
        <span>{{ $t('import-values-matched', { count: cardMatchedCount(card, edits) }) }}</span>
        <template v-if="cardMatched(card, edits).length > 0">
          <span aria-hidden="true" class="text-tertiary">·</span>
          <button type="button" :class="linkClass" @click="toggleMatched(card.id)">
            {{ showMatched[card.id] ? $t('import-values-hide') : $t('import-values-show') }}
          </button>
        </template>
      </div>
      <ul v-if="showMatched[card.id]" class="px-3 pb-3 flex flex-col gap-1 text-sm">
        <li v-for="e in cardMatched(card, edits)" :key="entryId(e)" class="flex flex-wrap items-center gap-x-3 gap-y-1">
          <span class="font-mono text-primary">{{ valueText(e.v) }}</span>
          <span class="text-xs text-tertiary">{{ $t('import-values-row-count', { count: e.v.rows.length }) }}</span>
          <span class="text-secondary">→ {{ answerLabel(e.col, e.v) }}</span>
          <StatusPill v-if="e.v.match === 'saved'" :label="$t('import-map-pill-saved')" tone="neutral" :dot="false" />
        </li>
      </ul>
    </SectionCard>

    <p
      v-if="shown.length > 0 && settled.length > 0"
      class="text-sm text-secondary"
      data-test="import-values-settled"
    >
      {{ settledText }}
    </p>

    <!-- Action bar. -->
    <div class="sticky bottom-0 bg-app border-t border-default py-3 flex flex-wrap items-center gap-3">
      <Button variant="ghost" :disabled="checking" data-test="import-values-back" @click="emit('back')">
        {{ $t('import-values-back') }}
      </Button>
      <span v-if="tried && unfinished.length > 0" class="ml-auto text-xs text-status-error">
        {{ $t('import-values-replace-missing', { count: unfinished.length }) }}
      </span>
      <Button
        :class="tried && unfinished.length > 0 ? '' : 'ml-auto'"
        :loading="checking"
        :disabled="checking"
        data-test="import-values-check"
        @click="checkAgain"
      >
        {{ checking ? $t('import-map-checking') : $t('import-values-check-again') }}
      </Button>
    </div>
  </section>
</template>
