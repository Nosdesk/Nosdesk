<script setup lang="ts">
/**
 * Calendar-grid date picker. Compact text input + popover with a
 * month grid; click a day, navigate months with the arrow buttons,
 * or type literal YYYY-MM-DD into the input. Same family of
 * primitive as TimePicker — exists for the same reason: native
 * `<input type="date">` chrome is browser-controlled and renders
 * differently in every engine, so it can't match the rest of the
 * form's theme tokens.
 *
 * Value contract is ISO YYYY-MM-DD. Empty string is "unset". Invalid
 * strings are accepted into the local draft but not committed until
 * they parse.
 *
 * The grid is a Monday-first 6-week (42 cell) window with leading +
 * trailing days from neighbouring months dimmed. Today gets an
 * accent ring; the selected date gets the accent fill. Click outside
 * or hit Escape to close.
 *
 * The header drills down: click the month label for a 12-month grid,
 * click the year for a 12-year grid, so jumping to a distant year is
 * two clicks instead of paging month by month.
 *
 * Range mode (`range`) swaps the single `v-model` for `v-model:start`
 * / `v-model:end`: first day click sets the start, second sets the
 * end (the grid previews the span on hover). Both ends stay ISO
 * YYYY-MM-DD; the trigger is read-only in this mode.
 *
 * Locale-aware first day of week and weekday-header labels are left
 * for a future iteration — Monday-first is the most common global
 * default and keeps the v1 simple.
 */
import { computed, nextTick, ref, watch, type Ref } from 'vue'
import { useFluent } from 'fluent-vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import Icon from '@/components/common/Icon.vue'

interface Props {
  modelValue?: string
  size?: 'sm' | 'md'
  ariaLabel?: string
  disabled?: boolean
  error?: boolean
  /** Lower bound (ISO YYYY-MM-DD inclusive). Cells before this are
   *  disabled in the grid; literal text below it is rejected. */
  min?: string
  /** Upper bound (ISO YYYY-MM-DD inclusive). */
  max?: string
  /** Stretch the trigger to fill its container instead of the
   *  default fixed inline width. Use when the picker sits in a
   *  form grid cell that expects `w-full` inputs. */
  block?: boolean
  /** Range mode: bind `v-model:start` and `v-model:end` instead of
   *  the single `v-model`. */
  range?: boolean
  /** Range start (ISO YYYY-MM-DD). Only used when `range`. */
  start?: string
  /** Range end (ISO YYYY-MM-DD). Only used when `range`. */
  end?: string
}

const props = withDefaults(defineProps<Props>(), {
  modelValue: '',
  size: 'sm',
  disabled: false,
  error: false,
  range: false,
  start: '',
  end: '',
})

const emit = defineEmits<{
  (e: 'update:modelValue', v: string): void
  (e: 'update:start', v: string): void
  (e: 'update:end', v: string): void
}>()

const fluent = useFluent()
const t = (key: string) => fluent.$t(key)

const triggerRef = ref<HTMLInputElement | null>(null) as Ref<HTMLInputElement | null>
const open = ref(false)
const draftText = ref(props.modelValue)
const isInvalid = ref(false)

watch(
  () => props.modelValue,
  (v) => {
    draftText.value = v
    isInvalid.value = false
  },
)

const ISO_RE = /^(\d{4})-(\d{2})-(\d{2})$/

interface DateParts {
  year: number
  month: number
  day: number
}

function parseIso(v: string): DateParts | null {
  const m = ISO_RE.exec(v.trim())
  if (!m) return null
  const year = Number(m[1])
  const month = Number(m[2])
  const day = Number(m[3])
  // JS Date validates day-in-month for us. month is 1-based in our
  // string but 0-based in Date.
  const d = new Date(Date.UTC(year, month - 1, day))
  if (
    d.getUTCFullYear() !== year ||
    d.getUTCMonth() !== month - 1 ||
    d.getUTCDate() !== day
  ) {
    return null
  }
  return { year, month, day }
}

function formatIso(year: number, month: number, day: number): string {
  return `${String(year).padStart(4, '0')}-${String(month).padStart(2, '0')}-${String(day).padStart(2, '0')}`
}

function isWithinBounds(iso: string): boolean {
  if (props.min && iso < props.min) return false
  if (props.max && iso > props.max) return false
  return true
}

function commitText(): void {
  const v = draftText.value.trim()
  if (v === '') {
    if (props.modelValue !== '') emit('update:modelValue', '')
    isInvalid.value = false
    return
  }
  const parsed = parseIso(v)
  if (parsed && isWithinBounds(v)) {
    isInvalid.value = false
    if (v !== props.modelValue) emit('update:modelValue', v)
  } else {
    isInvalid.value = true
  }
}

function applyDate(year: number, month: number, day: number): void {
  const iso = formatIso(year, month, day)
  if (!isWithinBounds(iso)) return
  draftText.value = iso
  isInvalid.value = false
  if (iso !== props.modelValue) emit('update:modelValue', iso)
}

// Range selection. `pendingStart` holds the first-clicked end while we
// wait for the second; `hoverIso` drives the live span preview.
const pendingStart = ref<string | null>(null)
const hoverIso = ref<string | null>(null)

/** The active span to paint: the live preview while picking, else the
 *  committed start/end. */
const activeRange = computed<{ from: string; to: string } | null>(() => {
  if (!props.range) return null
  const anchor = pendingStart.value
  if (anchor && hoverIso.value) {
    return hoverIso.value < anchor
      ? { from: hoverIso.value, to: anchor }
      : { from: anchor, to: hoverIso.value }
  }
  if (props.start && props.end) return { from: props.start, to: props.end }
  if (anchor) return { from: anchor, to: anchor }
  return null
})

function selectRange(iso: string): void {
  if (pendingStart.value === null) {
    // First click: anchor the start and clear any prior end.
    pendingStart.value = iso
    if (props.start !== iso) emit('update:start', iso)
    if (props.end !== '') emit('update:end', '')
    return
  }
  // Second click: order the two ends and commit.
  const anchor = pendingStart.value
  const from = iso < anchor ? iso : anchor
  const to = iso < anchor ? anchor : iso
  if (props.start !== from) emit('update:start', from)
  if (props.end !== to) emit('update:end', to)
  pendingStart.value = null
  hoverIso.value = null
  open.value = false
}

function onCellClick(cell: Cell): void {
  if (cell.disabled) return
  if (props.range) {
    selectRange(cell.iso)
  } else {
    applyDate(cell.year, cell.month, cell.day)
  }
}

// Currently-displayed month in the popover, and which drill-down pane
// is showing. Defaults to the value's month (or today's if unset).
const today = new Date()
const todayIso = formatIso(today.getFullYear(), today.getMonth() + 1, today.getDate())

type View = 'days' | 'months' | 'years'
const view = ref<View>('days')

const cursorYear = ref(today.getFullYear())
const cursorMonth = ref(today.getMonth() + 1)

watch(open, (isOpen) => {
  if (!isOpen) {
    pendingStart.value = null
    hoverIso.value = null
    return
  }
  // Reset to the day grid and centre it on the most relevant value.
  view.value = 'days'
  const focusIso = props.range ? props.start || props.end : props.modelValue
  const parsed = parseIso(focusIso)
  if (parsed) {
    cursorYear.value = parsed.year
    cursorMonth.value = parsed.month
  } else {
    cursorYear.value = today.getFullYear()
    cursorMonth.value = today.getMonth() + 1
  }
  // In range mode, an open with a start but no end means we're still
  // mid-selection, so prime the anchor to complete on the next click.
  pendingStart.value = props.range && props.start && !props.end ? props.start : null
})

function shiftMonth(delta: number): void {
  const date = new Date(Date.UTC(cursorYear.value, cursorMonth.value - 1 + delta, 1))
  cursorYear.value = date.getUTCFullYear()
  cursorMonth.value = date.getUTCMonth() + 1
}

function shiftYear(delta: number): void {
  cursorYear.value += delta
}

// 12-year pages, aligned so a page always starts on a multiple of 12.
const yearPageStart = computed(() => Math.floor(cursorYear.value / 12) * 12)

function shiftYearPage(delta: number): void {
  cursorYear.value += delta * 12
}

function pickMonth(month: number): void {
  cursorMonth.value = month
  view.value = 'days'
}

function pickYear(year: number): void {
  cursorYear.value = year
  view.value = 'months'
}

/** A whole month is unreachable when it sits entirely outside bounds. */
function monthDisabled(year: number, month: number): boolean {
  const firstIso = formatIso(year, month, 1)
  const lastDay = new Date(Date.UTC(year, month, 0)).getUTCDate()
  const lastIso = formatIso(year, month, lastDay)
  if (props.max && firstIso > props.max) return true
  if (props.min && lastIso < props.min) return true
  return false
}

function yearDisabled(year: number): boolean {
  if (props.max && formatIso(year, 1, 1) > props.max) return true
  if (props.min && formatIso(year, 12, 31) < props.min) return true
  return false
}

// Localised short month names (Jan, Feb, …), Mon-grid for the picker.
const monthShortNames = computed<string[]>(() => {
  try {
    const fmt = new Intl.DateTimeFormat(undefined, { month: 'short' })
    return Array.from({ length: 12 }, (_, m) => fmt.format(new Date(Date.UTC(2024, m, 1))))
  } catch {
    return ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']
  }
})

const yearPageLabel = computed(() => `${yearPageStart.value} – ${yearPageStart.value + 11}`)

const yearCells = computed(() =>
  Array.from({ length: 12 }, (_, i) => {
    const year = yearPageStart.value + i
    return { year, isCurrent: year === cursorYear.value, disabled: yearDisabled(year) }
  }),
)

interface Cell {
  year: number
  month: number
  day: number
  iso: string
  inCurrentMonth: boolean
  isToday: boolean
  isSelected: boolean
  isRangeEnd: boolean
  inRange: boolean
  disabled: boolean
}

// Monday-first grid. We compute the cell sequence on every render
// (cheap; 42 entries). Leading days come from the previous month,
// trailing from the next, so the grid is always 6 weeks.
const cells = computed<Cell[]>(() => {
  const year = cursorYear.value
  const month = cursorMonth.value
  const span = activeRange.value
  // JS getUTCDay returns 0=Sun..6=Sat; convert to Mon=0..Sun=6.
  const firstOfMonth = new Date(Date.UTC(year, month - 1, 1))
  const startOffset = (firstOfMonth.getUTCDay() + 6) % 7
  const out: Cell[] = []
  for (let i = 0; i < 42; i++) {
    const d = new Date(Date.UTC(year, month - 1, 1 - startOffset + i))
    const y = d.getUTCFullYear()
    const m = d.getUTCMonth() + 1
    const day = d.getUTCDate()
    const iso = formatIso(y, m, day)
    // In range mode the two ends get the accent fill and the days
    // between get a muted band; otherwise the single value is selected.
    const isRangeEnd = !!span && (iso === span.from || iso === span.to)
    const isSelected = props.range ? isRangeEnd : iso === props.modelValue
    out.push({
      year: y,
      month: m,
      day,
      iso,
      inCurrentMonth: m === month,
      isToday: iso === todayIso,
      isSelected,
      isRangeEnd,
      inRange: !!span && iso > span.from && iso < span.to,
      disabled: !isWithinBounds(iso),
    })
  }
  return out
})

const monthLabel = computed(() => {
  // Locale-aware month + year display. Falls back gracefully if
  // Intl is unavailable in the runtime.
  try {
    return new Intl.DateTimeFormat(undefined, {
      year: 'numeric',
      month: 'long',
    }).format(new Date(Date.UTC(cursorYear.value, cursorMonth.value - 1, 1)))
  } catch {
    return `${cursorYear.value}-${String(cursorMonth.value).padStart(2, '0')}`
  }
})

// Read-only text shown in the range-mode trigger. "start → end", with
// an ellipsis standing in for the end while it's still being picked.
const rangeDisplay = computed(() => {
  const s = props.start?.trim() ?? ''
  const e = props.end?.trim() ?? ''
  if (!s && !e) return ''
  if (s && !e) return `${s} → …`
  return `${s} → ${e}`
})

const weekdayLabels = computed<string[]>(() => {
  // Two-letter weekday labels, Mon-first. We build them from Intl
  // so the user's locale picks the names; if Intl is missing we fall
  // back to English.
  const out: string[] = []
  try {
    const fmt = new Intl.DateTimeFormat(undefined, { weekday: 'short' })
    // 2024-01-01 is a Monday — anchor the loop on it.
    for (let i = 0; i < 7; i++) {
      const d = new Date(Date.UTC(2024, 0, 1 + i))
      out.push(fmt.format(d).slice(0, 2))
    }
  } catch {
    out.push('Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa', 'Su')
  }
  return out
})

function focusInput(): void {
  triggerRef.value?.focus()
}

defineExpose({ focus: focusInput })

// Scroll-to-selected isn't useful for the date grid (always one
// month visible), so we just open and let the user navigate.
async function onOpen(): Promise<void> {
  open.value = true
  await nextTick()
}
</script>

<template>
  <div class="date-picker" :class="{ 'date-picker--block': block }">
    <input
      v-if="range"
      ref="triggerRef"
      type="text"
      readonly
      :value="rangeDisplay"
      :aria-label="ariaLabel"
      :disabled="disabled"
      placeholder="YYYY-MM-DD → YYYY-MM-DD"
      class="date-picker__input date-picker__input--range"
      :class="[
        size === 'sm' ? 'date-picker__input--sm' : 'date-picker__input--md',
        error && 'date-picker__input--invalid',
      ]"
      @focus="onOpen"
      @click="onOpen"
      @keydown.escape="open = false"
    />
    <input
      v-else
      ref="triggerRef"
      v-model="draftText"
      type="text"
      maxlength="10"
      :aria-label="ariaLabel"
      :disabled="disabled"
      :aria-invalid="isInvalid || error ? 'true' : undefined"
      placeholder="YYYY-MM-DD"
      class="date-picker__input"
      :class="[
        size === 'sm' ? 'date-picker__input--sm' : 'date-picker__input--md',
        (isInvalid || error) && 'date-picker__input--invalid',
      ]"
      @focus="onOpen"
      @click="onOpen"
      @blur="commitText"
      @keydown.enter.prevent="commitText"
      @keydown.escape="open = false"
    />

    <ResponsiveMenu
      :open="open"
      :anchor="{ type: 'element', element: () => triggerRef }"
      placement="bottom-start"
      role="dialog"
      :offset="4"
      :auto-focus="false"
      popover-class="date-picker__popover"
      @close="open = false"
    >
      <!-- Day grid: page months, drill into the month/year panes. -->
      <template v-if="view === 'days'">
        <header class="date-picker__header">
          <button
            type="button"
            class="date-picker__nav"
            :aria-label="t('date-picker-prev-month-aria')"
            @click="shiftMonth(-1)"
          >
            <Icon name="chevronLeft" class="w-3.5 h-3.5" />
          </button>
          <button
            type="button"
            class="date-picker__title"
            :aria-label="t('date-picker-select-month-aria')"
            @click="view = 'months'"
          >
            {{ monthLabel }}
          </button>
          <button
            type="button"
            class="date-picker__nav"
            :aria-label="t('date-picker-next-month-aria')"
            @click="shiftMonth(1)"
          >
            <Icon name="chevronRight" class="w-3.5 h-3.5" />
          </button>
        </header>

        <div class="date-picker__weekdays">
          <span v-for="(w, i) in weekdayLabels" :key="i" class="date-picker__weekday">
            {{ w }}
          </span>
        </div>

        <div class="date-picker__grid" @mouseleave="hoverIso = null">
          <button
            v-for="(cell, i) in cells"
            :key="i"
            type="button"
            class="date-picker__cell"
            :class="{
              'is-other-month': !cell.inCurrentMonth,
              'is-today': cell.isToday,
              'is-selected': cell.isSelected,
              'is-in-range': cell.inRange,
              'is-disabled': cell.disabled,
            }"
            :disabled="cell.disabled"
            :aria-current="cell.isToday ? 'date' : undefined"
            :aria-pressed="cell.isSelected"
            @click="onCellClick(cell)"
            @mouseenter="range ? (hoverIso = cell.iso) : undefined"
          >
            {{ cell.day }}
          </button>
        </div>
      </template>

      <!-- Month pane: pick a month within the cursor year. -->
      <template v-else-if="view === 'months'">
        <header class="date-picker__header">
          <button
            type="button"
            class="date-picker__nav"
            :aria-label="t('date-picker-prev-year-aria')"
            @click="shiftYear(-1)"
          >
            <Icon name="chevronLeft" class="w-3.5 h-3.5" />
          </button>
          <button
            type="button"
            class="date-picker__title"
            :aria-label="t('date-picker-select-year-aria')"
            @click="view = 'years'"
          >
            {{ cursorYear }}
          </button>
          <button
            type="button"
            class="date-picker__nav"
            :aria-label="t('date-picker-next-year-aria')"
            @click="shiftYear(1)"
          >
            <Icon name="chevronRight" class="w-3.5 h-3.5" />
          </button>
        </header>

        <div class="date-picker__pane-grid">
          <button
            v-for="(name, m) in monthShortNames"
            :key="m"
            type="button"
            class="date-picker__pane-cell"
            :class="{
              'is-selected': m + 1 === cursorMonth,
              'is-disabled': monthDisabled(cursorYear, m + 1),
            }"
            :disabled="monthDisabled(cursorYear, m + 1)"
            @click="pickMonth(m + 1)"
          >
            {{ name }}
          </button>
        </div>
      </template>

      <!-- Year pane: pick a year within the current 12-year page. -->
      <template v-else>
        <header class="date-picker__header">
          <button
            type="button"
            class="date-picker__nav"
            :aria-label="t('date-picker-prev-years-aria')"
            @click="shiftYearPage(-1)"
          >
            <Icon name="chevronLeft" class="w-3.5 h-3.5" />
          </button>
          <span class="date-picker__title is-static">{{ yearPageLabel }}</span>
          <button
            type="button"
            class="date-picker__nav"
            :aria-label="t('date-picker-next-years-aria')"
            @click="shiftYearPage(1)"
          >
            <Icon name="chevronRight" class="w-3.5 h-3.5" />
          </button>
        </header>

        <div class="date-picker__pane-grid">
          <button
            v-for="cell in yearCells"
            :key="cell.year"
            type="button"
            class="date-picker__pane-cell"
            :class="{ 'is-selected': cell.isCurrent, 'is-disabled': cell.disabled }"
            :disabled="cell.disabled"
            @click="pickYear(cell.year)"
          >
            {{ cell.year }}
          </button>
        </div>
      </template>
    </ResponsiveMenu>
  </div>
</template>

<style scoped>
.date-picker {
  position: relative;
  display: inline-block;
}

.date-picker--block {
  display: block;
  width: 100%;
}

.date-picker--block .date-picker__input {
  width: 100%;
}

.date-picker__input {
  background-color: var(--color-surface-alt);
  border: 1px solid var(--color-subtle);
  border-radius: 0.5rem;
  color: var(--color-primary);
  font-variant-numeric: tabular-nums;
  font-feature-settings: 'tnum';
  transition: border-color 150ms ease, box-shadow 150ms ease;
  width: 8.5rem;
}

.date-picker__input--sm {
  padding: 0.25rem 0.5rem;
  font-size: 12px;
  line-height: 1.4;
}

.date-picker__input--md {
  padding: 0.5rem 0.75rem;
  font-size: 14px;
}

.date-picker__input::placeholder {
  color: var(--color-tertiary);
}

.date-picker__input:focus {
  outline: none;
  border-color: var(--color-accent);
  box-shadow: 0 0 0 2px var(--color-accent-muted);
}

.date-picker__input--invalid {
  border-color: var(--color-status-error);
}

.date-picker__input--range {
  cursor: pointer;
}

.date-picker__input:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>

<style>
/* Unscoped: the popover is teleported to <body>, so scoped hashes
   never reach it. Class prefix keeps the rules scoped logically. */
.date-picker__popover {
  background-color: var(--color-surface);
  border: 1px solid var(--color-default);
  border-radius: 0.5rem;
  box-shadow: 0 10px 25px -10px rgba(0, 0, 0, 0.2);
  padding: 0.5rem;
  width: 16rem;
}

.date-picker__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.25rem;
  padding: 0.25rem 0.5rem 0.5rem;
}

.date-picker__nav {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 1.5rem;
  height: 1.5rem;
  border-radius: 0.25rem;
  color: var(--color-secondary);
  background: transparent;
  border: none;
  cursor: pointer;
  transition: background-color 100ms ease, color 100ms ease;
}

.date-picker__nav:hover {
  background-color: var(--color-surface-hover);
  color: var(--color-primary);
}

.date-picker__title {
  flex: 1;
  text-align: center;
  font-size: 12px;
  font-weight: 600;
  color: var(--color-primary);
  text-transform: capitalize;
  background: transparent;
  border: none;
  border-radius: 0.25rem;
  padding: 0.25rem 0.5rem;
  cursor: pointer;
  transition: background-color 100ms ease;
}

.date-picker__title:hover {
  background-color: var(--color-surface-hover);
}

.date-picker__title.is-static {
  cursor: default;
}

.date-picker__title.is-static:hover {
  background: transparent;
}

/* Month / year drill-down panes: a 3-column grid of larger targets. */
.date-picker__pane-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 0.25rem;
  padding: 0.25rem;
}

.date-picker__pane-cell {
  padding: 0.5rem 0;
  background: transparent;
  border: 1px solid transparent;
  border-radius: 0.25rem;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  color: var(--color-secondary);
  text-transform: capitalize;
  cursor: pointer;
  transition: background-color 100ms ease, color 100ms ease, border-color 100ms ease;
}

.date-picker__pane-cell:hover:not(.is-disabled) {
  background-color: var(--color-surface-hover);
  color: var(--color-primary);
}

.date-picker__pane-cell:focus-visible {
  outline: none;
  box-shadow: 0 0 0 1px var(--color-accent);
}

.date-picker__pane-cell.is-selected {
  background-color: var(--color-accent);
  color: var(--color-on-accent);
  border-color: var(--color-accent);
  font-weight: 600;
}

.date-picker__pane-cell.is-disabled {
  cursor: not-allowed;
  opacity: 0.3;
}

.date-picker__weekdays {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  padding: 0 0.25rem 0.25rem;
}

.date-picker__weekday {
  text-align: center;
  font-size: 10px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--color-tertiary);
}

.date-picker__grid {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: 1px;
  padding: 0 0.25rem;
}

.date-picker__cell {
  aspect-ratio: 1 / 1;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: 1px solid transparent;
  border-radius: 0.25rem;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  color: var(--color-secondary);
  cursor: pointer;
  transition: background-color 100ms ease, color 100ms ease, border-color 100ms ease;
}

.date-picker__cell:hover:not(.is-disabled) {
  background-color: var(--color-surface-hover);
  color: var(--color-primary);
}

.date-picker__cell:focus-visible {
  outline: none;
  box-shadow: 0 0 0 1px var(--color-accent);
}

.date-picker__cell.is-other-month {
  color: var(--color-tertiary);
  opacity: 0.55;
}

.date-picker__cell.is-today {
  border-color: var(--color-accent);
  color: var(--color-primary);
  font-weight: 600;
}

.date-picker__cell.is-in-range {
  background-color: var(--color-accent-muted);
  color: var(--color-primary);
  border-radius: 0;
}

.date-picker__cell.is-in-range:hover {
  background-color: var(--color-accent-muted);
}

.date-picker__cell.is-selected {
  background-color: var(--color-accent);
  color: var(--color-on-accent);
  border-color: var(--color-accent);
  font-weight: 600;
}

.date-picker__cell.is-selected:hover {
  background-color: var(--color-accent);
  color: var(--color-on-accent);
}

.date-picker__cell.is-disabled {
  cursor: not-allowed;
  opacity: 0.3;
}
</style>
