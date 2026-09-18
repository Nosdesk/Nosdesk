<script setup lang="ts">
/**
 * Date picker on Reka's DatePicker (single) and DateRangePicker
 * (`range`): a segmented field plus a calendar popover. Native
 * `<input type="date">` chrome is browser-controlled, so the control
 * is ours to theme; Reka owns the parts a hand-rolled one gets wrong:
 *
 * - the field is `role=group` of `spinbutton` segments in the locale's
 *   order (day/month/year), typed digit by digit, arrows step, Backspace
 *   clears, Left/Right move between segments;
 * - the calendar is a `grid` with roving focus, arrows move a day,
 *   PageUp/PageDown a month, Home/End a week, Enter selects, Escape
 *   returns focus to the trigger; the week starts on the locale's day
 *   and the day cells carry full localized labels;
 * - the popover is a Reka layer, so it nests inside dialogs and
 *   other popovers without dismissing them.
 *
 * Value contract is unchanged: ISO `YYYY-MM-DD`, empty string means
 * unset. `range` swaps the single `v-model` for `v-model:start` /
 * `v-model:end`; the first day picked emits the start (and clears the
 * end), the second emits the end, ordered.
 *
 * The field commits when focus leaves it, on Enter, and when the
 * calendar closes (a pick closes it), not on every keystroke: the
 * consumers autosave, and a date typed digit by digit passes through
 * other complete dates on the way. `min` / `max` disable days in the
 * grid and hold a typed value outside them in the field (flagged
 * invalid) without committing it. The old month/year drill-down panes
 * are gone: jumping to a distant date is typing the year segment.
 */
import { computed, onBeforeUnmount, ref, shallowRef, useId, watch, type Component } from 'vue'
import { useFluent } from 'fluent-vue'
import { parseDate, today, type CalendarDate, type DateValue } from '@internationalized/date'
import {
  DatePickerCalendar,
  DatePickerCell,
  DatePickerCellTrigger,
  DatePickerContent,
  DatePickerField,
  DatePickerGrid,
  DatePickerGridBody,
  DatePickerGridHead,
  DatePickerGridRow,
  DatePickerHeadCell,
  DatePickerHeader,
  DatePickerHeading,
  DatePickerInput,
  DatePickerNext,
  DatePickerPrev,
  DatePickerRoot,
  DatePickerTrigger,
  DateRangePickerCalendar,
  DateRangePickerCell,
  DateRangePickerCellTrigger,
  DateRangePickerContent,
  DateRangePickerField,
  DateRangePickerGrid,
  DateRangePickerGridBody,
  DateRangePickerGridHead,
  DateRangePickerGridRow,
  DateRangePickerHeadCell,
  DateRangePickerHeader,
  DateRangePickerHeading,
  DateRangePickerInput,
  DateRangePickerNext,
  DateRangePickerPrev,
  DateRangePickerRoot,
  DateRangePickerTrigger,
  type DateRange,
  type SegmentPart,
} from 'reka-ui'
import { getLocalTimeZone } from '@nosdesk/core/utils/dateMath'
import Icon from '@/components/common/Icon.vue'

interface Props {
  modelValue?: string
  size?: 'sm' | 'md'
  /** Label rendered above the field in the same shell as FormInput. */
  label?: string
  /** Accessible name when there is no visible label. */
  ariaLabel?: string
  disabled?: boolean
  error?: boolean
  /** Lower bound (ISO YYYY-MM-DD inclusive). Days before it are
   *  disabled in the grid; a typed value below it is held, not committed. */
  min?: string
  /** Upper bound (ISO YYYY-MM-DD inclusive). */
  max?: string
  /** Stretch the field to fill its container instead of the default
   *  fixed inline width. */
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
const t = (key: string, args?: Record<string, string>) => fluent.$t(key, args)

const generatedId = useId()
const inputId = `date-picker-${generatedId}`
const labelId = computed(() => (props.label ? `${inputId}-label` : undefined))
// Reka names the grid "Event Date, <month>" unless told otherwise.
const calendarLabel = computed(() => props.label ?? props.ariaLabel ?? t('date-picker-calendar-aria'))

// Single and range pickers are parallel part sets on Reka's side; one
// template renders either through this map. Typed loosely on purpose:
// the two sets differ in model type, and the template is written to
// the props they share.
const single: Record<string, Component> = {
  Root: DatePickerRoot,
  Field: DatePickerField,
  Input: DatePickerInput,
  Trigger: DatePickerTrigger,
  Content: DatePickerContent,
  Calendar: DatePickerCalendar,
  Header: DatePickerHeader,
  Prev: DatePickerPrev,
  Next: DatePickerNext,
  Heading: DatePickerHeading,
  Grid: DatePickerGrid,
  GridHead: DatePickerGridHead,
  GridRow: DatePickerGridRow,
  HeadCell: DatePickerHeadCell,
  GridBody: DatePickerGridBody,
  Cell: DatePickerCell,
  CellTrigger: DatePickerCellTrigger,
}
const ranged: Record<string, Component> = {
  Root: DateRangePickerRoot,
  Field: DateRangePickerField,
  Input: DateRangePickerInput,
  Trigger: DateRangePickerTrigger,
  Content: DateRangePickerContent,
  Calendar: DateRangePickerCalendar,
  Header: DateRangePickerHeader,
  Prev: DateRangePickerPrev,
  Next: DateRangePickerNext,
  Heading: DateRangePickerHeading,
  Grid: DateRangePickerGrid,
  GridHead: DateRangePickerGridHead,
  GridRow: DateRangePickerGridRow,
  HeadCell: DateRangePickerHeadCell,
  GridBody: DateRangePickerGridBody,
  Cell: DateRangePickerCell,
  CellTrigger: DateRangePickerCellTrigger,
}
const parts = computed(() => (props.range ? ranged : single))

// ---- ISO <-> CalendarDate ------------------------------------------

function toDate(iso: string | undefined): CalendarDate | undefined {
  if (!iso) return undefined
  try {
    return parseDate(iso.trim())
  } catch {
    return undefined
  }
}

const toIso = (d: DateValue | undefined | null): string => (d ? d.toString().slice(0, 10) : '')

function inBounds(iso: string): boolean {
  if (props.min && iso < props.min) return false
  if (props.max && iso > props.max) return false
  return true
}

const minValue = computed(() => toDate(props.min))
const maxValue = computed(() => toDate(props.max))

// The field shows what was typed, including a value outside the
// bounds (Reka flags it invalid); the parent hears about it on commit,
// and only when it is a value it can accept. Parent writes replace
// the draft. Only an edit commits: a parent value the field could not
// parse is left alone, not cleared.
let dirty = false
const draft = shallowRef<CalendarDate | undefined>(toDate(props.modelValue))
watch(
  () => props.modelValue,
  (v) => {
    draft.value = toDate(v)
    dirty = false
  },
)

function onUpdate(value: DateValue | undefined): void {
  draft.value = value ? parseDate(toIso(value)) : undefined
  dirty = true
}

const rangeDraft = shallowRef<DateRange>({ start: toDate(props.start), end: toDate(props.end) })
watch(
  () => [props.start, props.end],
  ([start, end]) => {
    rangeDraft.value = { start: toDate(start), end: toDate(end) }
    dirty = false
  },
)

function onRangeUpdate(value: DateRange): void {
  rangeDraft.value = value
  dirty = true
}

// The calendar opens on the value, else today (Reka reads this once and
// follows the value from then on); a date-only placeholder also pins
// the field to three segments.
const placeholder = (props.range ? rangeDraft.value.start : draft.value) ?? today(getLocalTimeZone())

function commit(): void {
  if (!dirty) return
  dirty = false
  if (props.range) {
    const start = toIso(rangeDraft.value.start)
    const end = toIso(rangeDraft.value.end)
    if (start !== props.start && (!start || inBounds(start))) emit('update:start', start)
    if (end !== props.end && (!end || inBounds(end))) emit('update:end', end)
    return
  }
  const iso = toIso(draft.value)
  if (iso && !inBounds(iso)) return
  if (iso !== props.modelValue) emit('update:modelValue', iso)
}

// Focus leaving the control commits; moving between segments, or
// into the calendar, does not. The host popover of a consumer can
// unmount the field before any blur lands, so unmount commits too.
const rootEl = ref<HTMLElement | null>(null)
function onFocusOut(event: FocusEvent): void {
  const next = event.relatedTarget
  if (next instanceof Node && rootEl.value?.contains(next)) return
  commit()
}
function onOpenChange(open: boolean): void {
  if (!open) commit()
}
onBeforeUnmount(commit)

// ---- Segment labels -----------------------------------------------

// Reka labels segments in English ("day,", "AM/PM"); ours come from the
// catalogue, with the range end prefixed so "Start day" and "End day"
// read apart.
const SEGMENT_KEYS: Partial<Record<SegmentPart, string>> = {
  day: 'date-segment-day',
  month: 'date-segment-month',
  year: 'date-segment-year',
  hour: 'date-segment-hour',
  minute: 'date-segment-minute',
  second: 'date-segment-second',
  dayPeriod: 'date-segment-day-period',
}

function segmentLabel(part: SegmentPart, end?: 'start' | 'end'): string | undefined {
  const key = SEGMENT_KEYS[part]
  if (!key) return undefined
  const segment = t(key)
  if (!end) return segment
  return t(end === 'start' ? 'date-range-start-segment' : 'date-range-end-segment', { segment })
}

// Invalid is ARIA on the spinbuttons, not the group. Only set when
// the parent says so: a fallthrough `undefined` would strip the flag
// Reka puts on an out-of-bounds value.
function segmentAttrs(part: SegmentPart, end?: 'start' | 'end'): Record<string, string> {
  if (part === 'literal') return {}
  const attrs: Record<string, string> = { 'aria-label': segmentLabel(part, end) ?? '' }
  if (props.error) attrs['aria-invalid'] = 'true'
  return attrs
}
</script>

<template>
  <div ref="rootEl" class="date-picker" :class="{ 'date-picker--block': block }" @focusout="onFocusOut">
    <label
      v-if="label"
      :id="labelId"
      :for="inputId"
      class="text-xs font-medium text-tertiary uppercase tracking-wide"
    >
      {{ label }}
    </label>

    <component
      :is="parts.Root"
      :id="inputId"
      :model-value="range ? rangeDraft : draft"
      :default-placeholder="placeholder"
      granularity="day"
      :min-value="minValue"
      :max-value="maxValue"
      :disabled="disabled"
      close-on-select
      prevent-deselect
      fixed-weeks
      weekday-format="short"
      @update:model-value="range ? onRangeUpdate($event) : onUpdate($event)"
      @update:open="onOpenChange"
    >
      <component
        :is="parts.Field"
        v-slot="{ segments }"
        class="date-picker__field"
        :class="[
          size === 'sm' ? 'date-picker__field--sm' : 'date-picker__field--md',
          range && 'date-picker__field--range',
          error && 'date-picker__field--invalid',
        ]"
        :aria-label="label ? undefined : ariaLabel"
        :aria-labelledby="labelId"
        @keydown.enter="commit"
      >
        <template v-if="range">
          <component
            :is="parts.Input"
            v-for="(item, i) in segments.start"
            :key="`start-${i}`"
            :part="item.part"
            type="start"
            class="date-picker__segment"
            v-bind="segmentAttrs(item.part, 'start')"
          >
            {{ item.value }}
          </component>
          <span class="date-picker__range-sep" aria-hidden="true">–</span>
          <component
            :is="parts.Input"
            v-for="(item, i) in segments.end"
            :key="`end-${i}`"
            :part="item.part"
            type="end"
            class="date-picker__segment"
            v-bind="segmentAttrs(item.part, 'end')"
          >
            {{ item.value }}
          </component>
        </template>
        <template v-else>
          <component
            :is="parts.Input"
            v-for="(item, i) in segments"
            :key="i"
            :part="item.part"
            class="date-picker__segment"
            v-bind="segmentAttrs(item.part)"
          >
            {{ item.value }}
          </component>
        </template>

        <!-- Inside the group so Left/Right reach it after the last
             segment, the way Reka lays the field out. -->
        <component :is="parts.Trigger" as-child>
          <button
            type="button"
            class="date-picker__trigger"
            :disabled="disabled"
            :aria-label="t('date-picker-open-calendar-aria')"
          >
            <Icon name="calendar" class="w-3.5 h-3.5" />
          </button>
        </component>
      </component>

      <component
        :is="parts.Content"
        side="bottom"
        align="start"
        :side-offset="4"
        :collision-padding="8"
        class="date-picker__popover popover-inner z-overlay"
        :class="range && 'date-picker__popover--range'"
      >
        <component :is="parts.Calendar" v-slot="{ weekDays, grid }" :calendar-label="calendarLabel">
          <component :is="parts.Header" class="date-picker__header">
            <component :is="parts.Prev" as-child>
              <button type="button" class="date-picker__nav" :aria-label="t('date-picker-prev-month-aria')">
                <Icon name="chevronLeft" class="w-3.5 h-3.5" />
              </button>
            </component>
            <component :is="parts.Heading" class="date-picker__title" />
            <component :is="parts.Next" as-child>
              <button type="button" class="date-picker__nav" :aria-label="t('date-picker-next-month-aria')">
                <Icon name="chevronRight" class="w-3.5 h-3.5" />
              </button>
            </component>
          </component>

          <component
            :is="parts.Grid"
            v-for="month in grid"
            :key="month.value.toString()"
            class="date-picker__grid"
          >
            <component :is="parts.GridHead">
              <component :is="parts.GridRow" class="date-picker__row">
                <component
                  :is="parts.HeadCell"
                  v-for="day in weekDays"
                  :key="day"
                  class="date-picker__weekday"
                >
                  {{ day }}
                </component>
              </component>
            </component>
            <component :is="parts.GridBody">
              <component
                :is="parts.GridRow"
                v-for="(week, i) in month.rows"
                :key="`week-${i}`"
                class="date-picker__row"
              >
                <component
                  :is="parts.Cell"
                  v-for="date in week"
                  :key="date.toString()"
                  :date="date"
                  class="date-picker__cell"
                >
                  <component
                    :is="parts.CellTrigger"
                    :day="date"
                    :month="month.value"
                    class="date-picker__day"
                  />
                </component>
              </component>
            </component>
          </component>
        </component>
      </component>
    </component>
  </div>
</template>

<style scoped>
.date-picker {
  display: inline-flex;
  flex-direction: column;
  gap: 0.375rem;
}

.date-picker--block {
  display: flex;
  width: 100%;
}

.date-picker--block .date-picker__field {
  width: 100%;
  min-width: 0;
}
</style>

<style>
/* Unscoped: the field's segments and the popover are Reka-rendered
   (the popover teleported), so scoped hashes never reach them. The
   class prefix keeps the rules logically scoped. */
.date-picker__field {
  display: inline-flex;
  align-items: center;
  gap: 0;
  background-color: var(--color-surface-alt);
  border: 1px solid var(--color-subtle);
  border-radius: 0.5rem;
  color: var(--color-primary);
  font-variant-numeric: tabular-nums;
  font-feature-settings: 'tnum';
  transition:
    border-color 150ms ease,
    box-shadow 150ms ease;
  min-width: 8.5rem;
}

.date-picker__field--sm {
  padding: 0.25rem 0.25rem 0.25rem 0.5rem;
  font-size: 12px;
  line-height: 1.4;
}

.date-picker__field--md {
  padding: 0.5rem 0.5rem 0.5rem 0.75rem;
  font-size: 14px;
}

.date-picker__field--range {
  min-width: 15rem;
}

.date-picker__field:focus-within {
  border-color: var(--color-accent);
  box-shadow: 0 0 0 2px var(--color-accent-muted);
}

.date-picker__field--invalid,
.date-picker__field[data-invalid] {
  border-color: var(--color-status-error);
}

.date-picker__field[data-disabled] {
  opacity: 0.5;
  cursor: not-allowed;
}

.date-picker__segment {
  padding: 0 1px;
  border-radius: 0.125rem;
  outline: none;
  caret-color: transparent;
}

.date-picker__segment[data-placeholder] {
  color: var(--color-tertiary);
}

.date-picker__segment[data-reka-date-field-segment='literal'] {
  color: var(--color-tertiary);
  padding: 0;
}

.date-picker__segment:focus {
  background-color: var(--color-accent);
  color: var(--color-on-accent);
}

.date-picker__range-sep {
  color: var(--color-tertiary);
  padding: 0 0.25rem;
}

.date-picker__trigger {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  margin-left: auto;
  padding-left: 0.375rem;
  width: 1.5rem;
  height: 1.25rem;
  border-radius: 0.25rem;
  color: var(--color-tertiary);
  background: transparent;
  border: none;
  cursor: pointer;
  transition:
    background-color 100ms ease,
    color 100ms ease;
}

.date-picker__trigger:hover:not(:disabled),
.date-picker__trigger[data-state='open'] {
  color: var(--color-primary);
}

.date-picker__trigger:focus-visible {
  outline: none;
  box-shadow: 0 0 0 1px var(--color-accent);
}

.date-picker__trigger:disabled {
  cursor: not-allowed;
}

.date-picker__popover {
  background-color: var(--color-surface);
  border: 1px solid var(--color-default);
  border-radius: 0.5rem;
  box-shadow: 0 10px 25px -10px rgba(0, 0, 0, 0.2);
  padding: 0.5rem;
  width: 16rem;
  outline: none;
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
  transition:
    background-color 100ms ease,
    color 100ms ease;
}

.date-picker__nav:hover:not(:disabled) {
  background-color: var(--color-surface-hover);
  color: var(--color-primary);
}

.date-picker__nav:focus-visible {
  outline: none;
  box-shadow: 0 0 0 1px var(--color-accent);
}

.date-picker__nav:disabled {
  opacity: 0.3;
  cursor: not-allowed;
}

.date-picker__title {
  flex: 1;
  text-align: center;
  font-size: 12px;
  font-weight: 600;
  color: var(--color-primary);
  text-transform: capitalize;
}

.date-picker__grid {
  width: 100%;
  border-collapse: collapse;
  table-layout: fixed;
}

.date-picker__weekday {
  padding: 0 0 0.25rem;
  text-align: center;
  font-size: 10px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--color-tertiary);
}

.date-picker__cell {
  padding: 0.5px;
  text-align: center;
}

.date-picker__day {
  aspect-ratio: 1 / 1;
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid transparent;
  border-radius: 0.25rem;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  color: var(--color-secondary);
  cursor: pointer;
  outline: none;
  transition:
    background-color 100ms ease,
    color 100ms ease,
    border-color 100ms ease;
}

.date-picker__day:hover:not([data-disabled]) {
  background-color: var(--color-surface-hover);
  color: var(--color-primary);
}

.date-picker__day:focus-visible {
  box-shadow: 0 0 0 1px var(--color-accent);
}

.date-picker__day[data-outside-view] {
  color: var(--color-tertiary);
  opacity: 0.55;
}

.date-picker__day[data-today] {
  border-color: var(--color-accent);
  color: var(--color-primary);
  font-weight: 600;
}

/* Range: the days inside the span (committed, or the live preview
   while the second end is picked) get a band; the two ends the fill. */
.date-picker__popover--range .date-picker__day[data-selected],
.date-picker__day[data-highlighted] {
  background-color: var(--color-accent-muted);
  color: var(--color-primary);
  border-radius: 0;
}

.date-picker__popover:not(.date-picker__popover--range) .date-picker__day[data-selected],
.date-picker__popover:not(.date-picker__popover--range) .date-picker__day[data-selected]:hover,
.date-picker__popover--range .date-picker__day[data-selection-start],
.date-picker__popover--range .date-picker__day[data-selection-end],
.date-picker__popover--range .date-picker__day[data-highlighted-start],
.date-picker__popover--range .date-picker__day[data-highlighted-end],
.date-picker__popover--range .date-picker__day[data-selection-start]:hover,
.date-picker__popover--range .date-picker__day[data-selection-end]:hover,
.date-picker__popover--range .date-picker__day[data-highlighted-start]:hover,
.date-picker__popover--range .date-picker__day[data-highlighted-end]:hover {
  background-color: var(--color-accent);
  color: var(--color-on-accent);
  border-color: var(--color-accent);
  border-radius: 0.25rem;
  font-weight: 600;
}

.date-picker__day[data-disabled],
.date-picker__day[data-unavailable] {
  cursor: not-allowed;
  opacity: 0.3;
}
</style>
