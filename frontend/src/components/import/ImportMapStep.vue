<script setup lang="ts">
/**
 * Map step: one row per source column, in source order, with up to three
 * sample values and a target picker pre-filled from the server's
 * suggestions. Only a Guessed pick asks for attention; a saved match from
 * the last import is quiet and says where it came from. "Set for every
 * row" fills fields the file lacks; the options card holds the match key,
 * which rows write, and the date order and decimal mark as read from the
 * samples. When every column is a saved match or an exact name and
 * nothing blocks, the step folds to two lines with the check focused.
 * The rules live in `mapModel.ts`. When the headers look like another
 * import type, a line above the columns offers to switch (`detectType.ts`).
 */
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'

import BaseDropdown from '@/components/common/BaseDropdown.vue'
import Button from '@/components/common/Button.vue'
import Callout from '@/components/common/Callout.vue'
import Checkbox from '@/components/common/Checkbox.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import FormInput from '@/components/common/FormInput.vue'
import Icon from '@/components/common/Icon.vue'
import SearchableDropdown from '@/components/common/SearchableDropdown.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import StatusPill from '@/components/common/StatusPill.vue'
import Tooltip from '@/components/common/Tooltip.vue'
import type { DropdownOption } from '@/components/common/dropdownOption'
import { useMobileDetection } from '@/composables/useMobileDetection'
import { useAuthStore } from '@/stores/auth'
import { formatCompactDate } from '@nosdesk/core/utils/dateUtils'
import type {
  ColumnMapping,
  DateOrder,
  DecimalMark,
  ImportField,
  ImportJob,
  ImportJobType,
  ImportMode,
  ImportOptions,
  MatchBy,
  SuggestionProvenance,
} from '@nosdesk/core/services/importService'

import { choiceLabel } from './choiceLabels'
import { detectOtherType, listHeaders } from './detectType'
import {
  IGNORE,
  attributeKinds,
  buildConstants,
  buildIgnored,
  buildMapping,
  clashingColumns,
  counts,
  createNeeds,
  detectDateOrder,
  detectDecimal,
  freeFields,
  initialRows,
  isMapped,
  isNotImported,
  isSettled,
  kindsInFile,
  mapsValueType,
  matchesFilter,
  pill,
  problems,
  requiredAttributeWarnings,
  requiredOnCreate,
  samplesOf,
  savedFrom,
  type ConstantRow,
  type MapFilter,
  type MapRow,
  type Problem,
} from './mapModel'

const props = defineProps<{
  job: ImportJob
  fields: ImportField[]
  /** Asset kinds for the kind pickers, as slug and label. */
  kinds: { value: string; label: string }[]
  checking: boolean
  /** The type is being switched from the detection line. */
  switching?: boolean
  /** "Change file" is deleting the job. */
  discarding?: boolean
  /** A field the step opens on, its picker focused: Review's "Fix at Map
   *  columns". */
  focusField?: string | null
}>()

const emit = defineEmits<{
  (e: 'check', mapping: ColumnMapping, options: ImportOptions, ignored: number[]): void
  (e: 'switch-type', type: ImportJobType): void
  (e: 'change-file'): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const { isMobile } = useMobileDetection('md')
const authStore = useAuthStore()

const source = computed(() => props.job.source!)
const isAssets = computed(() => props.job.job_type === 'assets')

const rows = ref<MapRow[]>(
  initialRows(
    source.value,
    props.job.suggested_mapping ?? [],
    props.job.mapping,
    props.job.ignored ?? [],
  ),
)

// Options stored on a job resumed or back from Review; a fresh job starts
// from the defaults and the samples, since options are never carried
// from one import to the next. The legacy "Don't match" reads as
// matching by tag, creating new assets only.
const stored: ImportOptions = props.job.options ?? {}
const legacyNone = (stored.match_by as string | undefined) === 'none'
const matchBy = ref<MatchBy>(legacyNone ? 'asset_tag' : (stored.match_by ?? 'asset_tag'))
const mode = ref<ImportMode>(legacyNone ? 'create_only' : (stored.mode ?? 'create_update'))

// ---- dates and decimals, as the samples show them ----

const showsDateOrder = computed(() => mapsValueType(rows.value, props.fields, ['date', 'date_time']))
const showsDecimal = computed(() => mapsValueType(rows.value, props.fields, ['number']))
const detectedDates = computed(() =>
  detectDateOrder(samplesOf(rows.value, props.fields, ['date', 'date_time'])),
)
const fileDecimal: DecimalMark = source.value.delimiter === ';' ? ',' : '.'
const detectedDecimal = computed<DecimalMark>(
  () => detectDecimal(samplesOf(rows.value, props.fields, ['number'])) ?? fileDecimal,
)
/** The admin's own choice; null follows the samples. */
const chosenDates = ref<DateOrder | null>(props.job.options ? (stored.date_order ?? null) : null)
const chosenDecimal = ref<DecimalMark | null>(props.job.options ? (stored.decimal ?? null) : null)
const dateOrder = computed<DateOrder>(() => chosenDates.value ?? detectedDates.value.order ?? 'iso')
const decimal = computed<DecimalMark>(() => chosenDecimal.value ?? detectedDecimal.value)
const datesUnresolved = computed(
  () => showsDateOrder.value && detectedDates.value.ambiguous && chosenDates.value === null,
)
const changingDates = ref(false)
const changingDecimal = ref(false)

// ---- constants ----

const fieldById = computed(() => new Map(props.fields.map((f) => [f.id, f])))

function initialConstants(): ConstantRow[] {
  if (props.job.options) {
    const out: ConstantRow[] = (stored.constants ?? []).map((c) => ({
      field: c.field,
      value: c.value,
      applyAll: c.apply_to === 'all',
      guessed: false,
    }))
    // A job stored with a default kind and no Kind column: the same
    // thing as a kind for every new row.
    const kindMapped = rows.value.some((r) => r.target === 'kind')
    if (stored.default_kind && !kindMapped && !out.some((c) => c.field === 'kind')) {
      out.push({ field: 'kind', value: stored.default_kind, applyAll: false, guessed: false })
    }
    return out
  }
  // People get the least access when nothing says otherwise.
  const role = fieldById.value.get('role')
  const roleMapped = rows.value.some((r) => r.target === 'role')
  if (
    props.job.job_type === 'users' &&
    role?.required &&
    role.enum?.includes('user') &&
    !roleMapped
  ) {
    return [{ field: 'role', value: 'user', applyAll: false, guessed: true }]
  }
  return []
}

const constants = ref<ConstantRow[]>(initialConstants())

// A column now feeds the field, so its value for every row goes.
const mappedFields = computed(() => new Set(rows.value.filter(isMapped).map((r) => r.target)))
watch(mappedFields, (mapped) => {
  constants.value = constants.value.filter((c) => !c.field || !mapped.has(c.field))
})

function addConstant() {
  constants.value.push({ field: '', value: '', applyAll: false, guessed: false })
}

function removeConstant(i: number) {
  constants.value.splice(i, 1)
}

function setConstantField(c: ConstantRow, field: string) {
  if (c.field === field) return
  c.field = field
  c.value = ''
  c.guessed = false
}

function setConstantValue(c: ConstantRow, value: string) {
  c.value = value
  c.guessed = false
}

function constantFieldOptions(c: ConstantRow): DropdownOption[] {
  return freeFields(rows.value, props.fields, props.job.job_type, constants.value, c.field).map(
    (f) => ({ value: f.id, label: fieldLabel(f.id) }),
  )
}

const canAddConstant = computed(
  () => freeFields(rows.value, props.fields, props.job.job_type, constants.value).length > 0,
)

/** The picker a constant's value takes: kinds for Kind, yes or no, the
 *  options of an enum (statuses by their labels), else typed text. */
function valueOptions(c: ConstantRow): DropdownOption[] | null {
  if (isAssets.value && c.field === 'kind') return props.kinds
  const field = fieldById.value.get(c.field)
  if (field?.value_type === 'boolean') {
    return [
      { value: 'true', label: t('import-constants-yes') },
      { value: 'false', label: t('import-constants-no') },
    ]
  }
  const values = field?.enum
  if (!values || values.length === 0) return null
  const label = (v: string) => choiceLabel(t, props.job.job_type, c.field, v, v)
  return values.map((v) => ({ value: v, label: label(v) }))
}

function valueInputType(c: ConstantRow): string {
  switch (fieldById.value.get(c.field)?.value_type) {
    case 'number':
      return 'number'
    case 'date':
      return 'date'
    default:
      return 'text'
  }
}

/** The kind for every row, set from the problem callout. */
const kindConstant = computed<string>({
  get: () => constants.value.find((c) => c.field === 'kind')?.value ?? '',
  set: (value) => {
    const found = constants.value.find((c) => c.field === 'kind')
    if (found) setConstantValue(found, value)
    else constants.value.push({ field: 'kind', value, applyAll: false, guessed: false })
  },
})

// ---- type detection ----

const typeHint = computed(() =>
  detectOtherType(
    props.job.job_type,
    source.value.headers.map((h) => h.name),
  ),
)
const typeHintText = computed(() => {
  const hint = typeHint.value
  if (!hint) return ''
  return t(`import-map-detect-${hint.type}`, { headers: listHeaders(hint.headers, lang) })
})
const busyElsewhere = computed(() => props.checking || !!props.switching || !!props.discarding)
const lang = typeof document === 'undefined' ? undefined : document.documentElement.lang || undefined

function fieldLabel(id: string): string {
  const field = fieldById.value.get(id)
  if (!field) return id
  return field.label_key ? t(field.label_key) : field.label
}

function headerName(index: number): string {
  return rows.value.find((r) => r.index === index)?.name ?? ''
}

const found = computed<Problem[]>(() =>
  problems(
    rows.value,
    props.fields,
    props.job.job_type,
    constants.value,
    datesUnresolved.value,
    mode.value,
  ),
)

/** Fields new records need that nothing gives, when rows that match can
 *  update without them: a warning, not a block. */
const createNeeded = computed(() =>
  createNeeds(rows.value, props.fields, props.job.job_type, constants.value, mode.value),
)
const createNeededText = computed(() => {
  const fields = createNeeded.value
  const args = { fields: listHeaders(fields.map(fieldLabel), lang), count: fields.length }
  return {
    heading: t(`import-map-create-heading-${props.job.job_type}`, args),
    body: t(`import-map-create-${props.job.job_type}`, args),
  }
})
const tally = computed(() => counts(rows.value, found.value))

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
    else if (f.required) {
      description = t(
        requiredOnCreate(f.id, props.job.job_type) ? 'import-map-required-on-create' : 'import-map-required',
      )
    }
    else if (f.required_for?.length) {
      description = t('import-map-required-for', { kinds: kindList(f.required_for) })
    }
    // Attributes sit under the kinds that define them.
    const group =
      f.group === 'attribute' && f.kinds?.length
        ? t('import-map-group-attribute-kinds', {
            kinds: listHeaders(attributeKinds(f, props.kinds), lang),
          })
        : t(GROUP_KEYS[f.group])
    return {
      value: f.id,
      label: fieldLabel(f.id),
      description,
      group,
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

/** "...Sam's import on 12 Jul", or "your import" for the admin's own;
 *  `prefix` picks the sentence. */
function provenanceText(
  p: SuggestionProvenance | null | undefined,
  prefix: string,
  count?: number,
): string {
  if (!p) return ''
  const args: Record<string, string | number> = { date: formatCompactDate(p.at) }
  if (count !== undefined) args.count = count
  if (p.by && p.by === authStore.user?.uuid) return t(`${prefix}-you`, args)
  if (p.by_name) return t(`${prefix}-from`, { ...args, name: p.by_name })
  return t(`${prefix}-unknown`, args)
}

function savedText(row: MapRow): string {
  return provenanceText(row.suggestion?.provenance, 'import-map-saved')
}

function clashText(row: MapRow): string {
  const others = clashingColumns(row, found.value)
  if (others.length === 0) return ''
  return t('import-map-clash', { headers: others.map((i) => `'${headerName(i)}'`).join(', ') })
}

/** Kind slugs as their labels, in a list. */
function kindList(slugs: string[]): string {
  return listHeaders(
    slugs.map((slug) => props.kinds.find((k) => k.value === slug)?.label ?? slug),
    lang,
  )
}

/** Kinds in the file whose new rows need attributes nothing gives. Only
 *  the Kind column's samples and the kind for every row are known here. */
const requiredWarnings = computed(() =>
  isAssets.value
    ? requiredAttributeWarnings(
        rows.value,
        props.fields,
        constants.value,
        kindsInFile(rows.value, constants.value, props.kinds),
      )
    : [],
)

function requiredText(w: { kind: string; needed: string[]; missing: string[] }): string {
  const kind = kindList([w.kind])
  const needed = listHeaders(w.needed.map(fieldLabel), lang)
  if (w.missing.length === w.needed.length) {
    return t('import-map-warning-required-none', { kind, needed, count: w.missing.length })
  }
  return t('import-map-warning-required-some', {
    kind,
    needed,
    missing: listHeaders(w.missing.map(fieldLabel), lang),
  })
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
    case 'dates':
      return t('import-map-problem-dates')
  }
}

/** Columns above which the summary strip and its filter earn their place. */
const STRIP_MIN_COLUMNS = 9
const showsStrip = computed(() => rows.value.length >= STRIP_MIN_COLUMNS)

const filter = ref<MapFilter>('all')

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

/** Columns "Next problem" steps through: clashes and guesses. */
const attentionColumns = computed(() =>
  rows.value
    .filter((r) => clashingColumns(r, found.value).length > 0 || pill(r) === 'guessed')
    .map((r) => r.index),
)

let lastProblem = -1
function nextProblem() {
  const list = attentionColumns.value
  if (list.length === 0) return
  const next = list.find((i) => i > lastProblem) ?? list[0]
  lastProblem = next
  focusColumn(next)
}

// ---- folded state ----

/** Folded when the job opens with every column settled: a fresh job only,
 *  so coming back from Review shows the columns that were checked. */
// A kind whose required attributes nothing gives, or a field new records
// need that nothing gives, keeps the step open, so its warning is read.
const collapsed = ref(
  props.job.mapping === null &&
    isSettled(rows.value, found.value, constants.value) &&
    requiredWarnings.value.length === 0 &&
    createNeeded.value.length === 0,
)
const collapsedText = computed(() => {
  const from = savedFrom(rows.value)
  return from
    ? provenanceText(from, 'import-map-collapsed', rows.value.length)
    : t('import-map-collapsed-names', { count: rows.value.length })
})

const checkButton = ref<InstanceType<typeof Button> | null>(null)

async function expand() {
  collapsed.value = false
  await nextTick()
  tableRoot.value?.querySelector<HTMLElement>('button[aria-haspopup]')?.focus()
}

const sectionRoot = ref<HTMLElement | null>(null)

/** Bring where `field` is set into view, focused: the column mapped to it,
 *  else its value for every row, else the kind for every row, else the
 *  button that adds a value for every row. */
async function focusOnField(field: string) {
  if (collapsed.value) {
    collapsed.value = false
    await nextTick()
  }
  const row = rows.value.find((r) => r.target === field)
  if (row) {
    await focusColumn(row.index)
    return
  }
  const root = sectionRoot.value
  if (!root) return
  const control = (el: Element | null | undefined) =>
    el?.querySelector<HTMLElement>('button[aria-haspopup], [role="combobox"], input, select, button') ?? null
  const at = constants.value.findIndex((c) => c.field === field)
  const constant = at >= 0 ? root.querySelectorAll('[data-test="import-constant"]')[at] : undefined
  const target =
    control(constant?.querySelector('[data-test="import-constant-value"]')) ??
    (field === 'kind' ? control(root.querySelector('[data-test="import-kind-for-every-row"]')) : null) ??
    root.querySelector<HTMLElement>('[data-test="import-constant-add"]')
  target?.scrollIntoView?.({ block: 'center', behavior: 'smooth' })
  target?.focus({ preventScroll: true })
}

onMounted(async () => {
  if (props.focusField) {
    await nextTick()
    await focusOnField(props.focusField)
    return
  }
  if (!collapsed.value) return
  await nextTick()
  ;(checkButton.value?.$el as HTMLElement | undefined)?.focus()
})

// ---- options ----

const matchOptions = computed(() => [
  { value: 'asset_tag' as MatchBy, label: t('import-options-match-asset-tag') },
  { value: 'serial_number' as MatchBy, label: t('import-options-match-serial') },
])
const matchHelp = computed(() =>
  matchBy.value === 'serial_number'
    ? t('import-options-match-serial-help')
    : t('import-options-match-asset-tag-help'),
)
const fixedMatchText = computed(() =>
  props.job.job_type === 'users'
    ? t('import-options-match-users')
    : t('import-options-match-tickets'),
)

const MODES: ImportMode[] = ['create_update', 'create_only', 'update_only']
const modeKey = (m: ImportMode) => m.replace('_', '-')
function modeLabel(m: ImportMode): string {
  return t(`import-options-mode-${modeKey(m)}`)
}
const modeOptions = computed(() => MODES.map((m) => ({ value: m, label: modeLabel(m) })))
const modeHelp = computed(() => t(`import-options-mode-${modeKey(mode.value)}-help`))

const dateOptions = computed(() => [
  { value: 'dmy' as DateOrder, label: t('import-options-date-dmy') },
  { value: 'mdy' as DateOrder, label: t('import-options-date-mdy') },
  { value: 'iso' as DateOrder, label: t('import-options-date-iso') },
])
const ambiguousDateOptions = computed(() => dateOptions.value.slice(0, 2))
const decimalOptions = computed(() => [
  { value: '.' as DecimalMark, label: t('import-options-decimal-dot') },
  { value: ',' as DecimalMark, label: t('import-options-decimal-comma') },
])

const datesModel = computed<DateOrder>({
  get: () => dateOrder.value,
  set: (value) => {
    chosenDates.value = value
  },
})
const decimalModel = computed<DecimalMark>({
  get: () => decimal.value,
  set: (value) => {
    chosenDecimal.value = value
  },
})
const decimalName = computed(() => (decimal.value === ',' ? 'comma' : 'dot'))

const optionsSummary = computed(() => {
  const parts: string[] = []
  if (isAssets.value) {
    parts.push(t('import-map-summary-match-assets', { key: matchBy.value }))
    parts.push(modeLabel(mode.value))
  } else {
    parts.push(t(`import-map-summary-match-${props.job.job_type}`))
  }
  if (showsDateOrder.value) parts.push(t('import-map-summary-dates', { order: dateOrder.value }))
  if (showsDecimal.value) parts.push(t('import-map-summary-decimal', { mark: decimalName.value }))
  return parts.join(' · ')
})

// ---- action bar ----

const blocked = computed(() => found.value.length > 0)
const showChangeFile = ref(false)

function check() {
  if (blocked.value) return
  const options: ImportOptions = {
    date_order: dateOrder.value,
    decimal: decimal.value,
    constants: buildConstants(constants.value, rows.value, props.job.job_type),
  }
  if (isAssets.value) {
    options.match_by = matchBy.value
    options.mode = mode.value
  }
  emit('check', buildMapping(rows.value), options, buildIgnored(rows.value))
}

const samplesId = (row: MapRow) => `import-map-samples-${row.index}`
const messageId = (row: MapRow) => `import-map-message-${row.index}`
const linkClass =
  'text-accent hover:underline focus:outline-none focus-visible:ring-1 focus-visible:ring-accent rounded'
</script>

<template>
  <section ref="sectionRoot" class="flex flex-col gap-6">
    <!-- Type detection: the headers look like another type. -->
    <Callout v-if="typeHint" severity="info" data-test="import-type-hint">
      <div class="px-4 py-3 flex flex-col gap-2">
        <p class="text-sm text-primary">{{ typeHintText }}</p>
        <div class="flex flex-wrap items-center gap-x-3 gap-y-2">
          <Button
            variant="secondary"
            size="sm"
            :loading="switching"
            :disabled="busyElsewhere"
            data-test="import-switch-type"
            @click="emit('switch-type', typeHint.type)"
          >
            {{ $t(`import-map-switch-${typeHint.type}`) }}
          </Button>
          <span class="text-xs text-tertiary">{{ $t(`import-map-switch-${typeHint.type}-help`) }}</span>
        </div>
      </div>
    </Callout>

    <!-- Folded: every column settled, nothing to decide. -->
    <SectionCard v-if="collapsed" content-padding="p-4" data-test="import-map-collapsed">
      <template #title>{{ $t('import-step-map') }}</template>
      <div class="flex flex-col gap-2 text-sm">
        <p class="flex flex-wrap items-center gap-x-2 gap-y-1 text-primary">
          <span>{{ collapsedText }}</span>
          <span aria-hidden="true" class="text-tertiary">·</span>
          <button type="button" :class="linkClass" data-test="import-map-expand" @click="expand">
            {{ $t('import-map-show-columns') }}
          </button>
        </p>
        <p class="flex flex-wrap items-center gap-x-2 gap-y-1 text-secondary">
          <span data-test="import-map-options-summary">{{ optionsSummary }}</span>
          <span aria-hidden="true" class="text-tertiary">·</span>
          <button type="button" :class="linkClass" @click="expand">
            {{ $t('import-map-change') }}
          </button>
        </p>
      </div>
    </SectionCard>

    <template v-else>
      <!-- Summary strip: counts, filter, next problem. A short file reads at
           a glance without it. Opaque, so cards scrolled under it don't show
           through. -->
      <div
        v-if="showsStrip"
        data-test="import-map-strip"
        class="sticky top-0 z-10 bg-app border-b border-default py-2 sm:py-3 flex flex-wrap items-center gap-x-3 gap-y-2"
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
            v-if="attentionColumns.length > 0"
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
            <div v-if="p.kind === 'kind'" class="max-w-xs" data-test="import-kind-for-every-row">
              <BaseDropdown
                v-model="kindConstant"
                :options="kinds"
                size="sm"
                :label="$t('import-constants-kind-label')"
                :placeholder="$t('import-constants-kind-placeholder')"
              />
            </div>
            <SegmentedControl
              v-if="p.kind === 'dates'"
              v-model="datesModel"
              class="self-start"
              :options="ambiguousDateOptions"
              size="sm"
              :aria-label="$t('import-options-date-label')"
            />
          </li>
        </ul>
      </Callout>

      <!-- Fields new records need that nothing gives: rows that match
           update, and the check reports the rows that would create. -->
      <Callout v-if="createNeeded.length > 0" severity="warning" data-test="import-create-needs">
        <template #header>
          <span class="font-medium text-primary">{{ createNeededText.heading }}</span>
        </template>
        <div class="px-4 py-3 flex flex-col gap-2 text-sm text-primary">
          <p>{{ createNeededText.body }}</p>
          <div
            v-if="isAssets && createNeeded.includes('kind')"
            class="max-w-xs"
            data-test="import-kind-for-every-row"
          >
            <BaseDropdown
              v-model="kindConstant"
              :options="kinds"
              size="sm"
              :label="$t('import-constants-kind-label')"
              :placeholder="$t('import-constants-kind-placeholder')"
            />
          </div>
        </div>
      </Callout>

      <!-- Kinds whose new rows need attributes nothing gives: the check
           goes ahead and skips those rows. -->
      <Callout v-if="requiredWarnings.length > 0" severity="warning" data-test="import-required-attributes">
        <template #header>
          <span class="font-medium text-primary">{{ $t('import-map-required-heading') }}</span>
        </template>
        <ul class="px-4 py-3 flex flex-col gap-2 text-sm text-primary">
          <li v-for="w in requiredWarnings" :key="w.kind">{{ requiredText(w) }}</li>
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
                      v-if="pill(row) === 'guessed'"
                      data-test="pill-guessed"
                      :label="$t('import-map-pill-guessed')"
                      tone="caution"
                    />
                    <!-- Quiet, with where it came from on hover or focus. -->
                    <Tooltip v-else-if="pill(row) === 'saved'" :text="savedText(row)">
                      <span
                        tabindex="0"
                        class="inline-flex rounded focus:outline-none focus-visible:ring-1 focus-visible:ring-accent"
                        data-test="pill-saved"
                        :data-provenance="savedText(row)"
                      >
                        <StatusPill :label="$t('import-map-pill-saved')" tone="neutral" :dot="false" />
                        <span class="sr-only">{{ savedText(row) }}</span>
                      </span>
                    </Tooltip>
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
                v-if="pill(row) === 'guessed'"
                data-test="pill-guessed"
                :label="$t('import-map-pill-guessed')"
                tone="caution"
              />
              <!-- Touch has no hover: where it came from reads beside the pill. -->
              <template v-else-if="pill(row) === 'saved'">
                <StatusPill
                  data-test="pill-saved"
                  :label="$t('import-map-pill-saved')"
                  tone="neutral"
                  :dot="false"
                />
                <span class="text-tertiary">{{ savedText(row) }}</span>
              </template>
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

      <!-- Set for every row: fields the file lacks. -->
      <SectionCard content-padding="p-4" data-test="import-constants">
        <template #title>{{ $t('import-constants-heading') }}</template>
        <div class="flex flex-col gap-4">
          <p class="text-xs text-tertiary">{{ $t('import-constants-help') }}</p>
          <div
            v-for="(c, i) in constants"
            :key="i"
            data-test="import-constant"
            class="grid gap-3 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] md:items-start"
          >
            <div class="flex flex-col gap-1.5 min-w-0">
              <BaseDropdown
                :model-value="c.field"
                :options="constantFieldOptions(c)"
                size="sm"
                :label="$t('import-constants-field-label')"
                :placeholder="$t('import-constants-field-placeholder')"
                @update:model-value="setConstantField(c, $event as string)"
              />
              <div v-if="c.guessed" class="flex flex-wrap items-center gap-2 text-xs">
                <StatusPill :label="$t('import-map-pill-guessed')" tone="caution" />
                <span class="text-tertiary">{{ $t('import-constants-guessed-role') }}</span>
              </div>
            </div>
            <div class="min-w-0" data-test="import-constant-value">
              <p v-if="!c.field" class="text-xs text-tertiary md:pt-7">
                {{ $t('import-constants-pick-field') }}
              </p>
              <BaseDropdown
                v-else-if="valueOptions(c)"
                :model-value="c.value"
                :options="valueOptions(c)!"
                size="sm"
                :label="$t('import-constants-value-label')"
                :placeholder="$t('import-constants-value-placeholder')"
                @update:model-value="setConstantValue(c, $event as string)"
              />
              <FormInput
                v-else
                :model-value="c.value"
                :type="valueInputType(c)"
                size="sm"
                :label="$t('import-constants-value-label')"
                data-test="import-constant-text"
                @update:model-value="setConstantValue(c, String($event ?? ''))"
              />
            </div>
            <div class="flex flex-wrap items-center gap-3 md:pt-7">
              <Checkbox
                v-if="isAssets"
                v-model="c.applyAll"
                size="sm"
                :label="$t('import-constants-apply-assets')"
              />
              <Button variant="ghost" size="sm" data-test="import-constant-remove" @click="removeConstant(i)">
                {{ $t('import-constants-remove') }}
              </Button>
            </div>
          </div>
          <div>
            <Button
              variant="secondary"
              size="sm"
              icon="add"
              :disabled="!canAddConstant"
              data-test="import-constant-add"
              @click="addConstant"
            >
              {{ $t('import-constants-add') }}
            </Button>
          </div>
        </div>
      </SectionCard>

      <!-- How to import. -->
      <SectionCard content-padding="p-4">
        <template #title>{{ $t('import-options-heading') }}</template>
        <div class="flex flex-col gap-5">
          <div class="flex flex-col gap-2">
            <span class="text-sm font-medium text-primary">{{ $t('import-options-match-label', { type: job.job_type }) }}</span>
            <template v-if="isAssets">
              <SegmentedControl
                v-model="matchBy"
                class="self-start"
                :options="matchOptions"
                size="sm"
                :aria-label="$t('import-options-match-label', { type: job.job_type })"
              />
              <p class="text-xs text-tertiary">{{ matchHelp }}</p>
            </template>
            <p v-else class="text-xs text-tertiary">{{ fixedMatchText }}</p>
          </div>

          <div v-if="isAssets" class="flex flex-col gap-2" data-test="import-mode">
            <span class="text-sm font-medium text-primary">{{ $t('import-options-mode-label') }}</span>
            <div v-if="isMobile" class="max-w-xs">
              <BaseDropdown
                v-model="mode"
                :options="modeOptions"
                size="sm"
                :aria-label="$t('import-options-mode-label')"
              />
            </div>
            <SegmentedControl
              v-else
              v-model="mode"
              class="self-start"
              :options="modeOptions"
              size="sm"
              :aria-label="$t('import-options-mode-label')"
            />
            <p class="text-xs text-tertiary">{{ modeHelp }}</p>
          </div>

          <div v-if="showsDateOrder && !datesUnresolved" class="flex flex-col gap-2" data-test="import-dates">
            <span class="text-sm font-medium text-primary">{{ $t('import-options-date-label') }}</span>
            <SegmentedControl
              v-if="changingDates"
              v-model="datesModel"
              class="self-start"
              :options="dateOptions"
              size="sm"
              :aria-label="$t('import-options-date-label')"
            />
            <p v-else class="text-sm text-secondary flex flex-wrap items-center gap-x-2">
              <span>{{ $t('import-options-date-fact', { order: dateOrder }) }}</span>
              <span aria-hidden="true" class="text-tertiary">·</span>
              <button type="button" :class="linkClass" @click="changingDates = true">
                {{ $t('import-map-change') }}
              </button>
            </p>
          </div>

          <div v-if="showsDecimal" class="flex flex-col gap-2" data-test="import-decimal">
            <span class="text-sm font-medium text-primary">{{ $t('import-options-decimal-label') }}</span>
            <SegmentedControl
              v-if="changingDecimal"
              v-model="decimalModel"
              class="self-start"
              :options="decimalOptions"
              size="sm"
              :aria-label="$t('import-options-decimal-label')"
            />
            <p v-else class="text-sm text-secondary flex flex-wrap items-center gap-x-2">
              <span>{{ $t('import-options-decimal-fact', { mark: decimalName }) }}</span>
              <span aria-hidden="true" class="text-tertiary">·</span>
              <button type="button" :class="linkClass" @click="changingDecimal = true">
                {{ $t('import-map-change') }}
              </button>
            </p>
          </div>
        </div>
      </SectionCard>
    </template>

    <!-- Action bar. -->
    <div
      class="sticky bottom-0 bg-app border-t border-default py-3 flex flex-wrap items-center gap-3"
    >
      <Button
        variant="ghost"
        :loading="discarding"
        :disabled="busyElsewhere"
        data-test="import-change-file"
        @click="showChangeFile = true"
      >
        {{ $t('import-map-change-file') }}
      </Button>
      <span v-if="blocked" class="ml-auto text-xs text-tertiary">
        {{ $t('import-map-blocked', { count: found.length }) }}
      </span>
      <Button
        ref="checkButton"
        :class="blocked ? '' : 'ml-auto'"
        :disabled="blocked || busyElsewhere"
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
