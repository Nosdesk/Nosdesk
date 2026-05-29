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
 * Locale-aware first day of week and weekday-header labels are left
 * for a future iteration — Monday-first is the most common global
 * default and keeps the v1 simple.
 */
import { computed, nextTick, ref, watch, type Ref } from 'vue'
import { useFluent } from 'fluent-vue'
import Popover from '@/components/common/Popover.vue'
import Icon from '@/components/common/Icon.vue'

interface Props {
  modelValue: string
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
}

const props = withDefaults(defineProps<Props>(), {
  size: 'sm',
  disabled: false,
  error: false,
})

const emit = defineEmits<{ (e: 'update:modelValue', v: string): void }>()

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

// Currently-displayed month in the popover. Defaults to the model
// value's month (or today's month if unset).
const today = new Date()
const todayIso = formatIso(today.getFullYear(), today.getMonth() + 1, today.getDate())

const cursorYear = ref(today.getFullYear())
const cursorMonth = ref(today.getMonth() + 1)

watch(
  () => [props.modelValue, open.value] as const,
  ([v, isOpen]) => {
    if (!isOpen) return
    const parsed = parseIso(v)
    if (parsed) {
      cursorYear.value = parsed.year
      cursorMonth.value = parsed.month
    } else {
      cursorYear.value = today.getFullYear()
      cursorMonth.value = today.getMonth() + 1
    }
  },
)

function shiftMonth(delta: number): void {
  const date = new Date(Date.UTC(cursorYear.value, cursorMonth.value - 1 + delta, 1))
  cursorYear.value = date.getUTCFullYear()
  cursorMonth.value = date.getUTCMonth() + 1
}

interface Cell {
  year: number
  month: number
  day: number
  iso: string
  inCurrentMonth: boolean
  isToday: boolean
  isSelected: boolean
  disabled: boolean
}

// Monday-first grid. We compute the cell sequence on every render
// (cheap; 42 entries). Leading days come from the previous month,
// trailing from the next, so the grid is always 6 weeks.
const cells = computed<Cell[]>(() => {
  const year = cursorYear.value
  const month = cursorMonth.value
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
    out.push({
      year: y,
      month: m,
      day,
      iso,
      inCurrentMonth: m === month,
      isToday: iso === todayIso,
      isSelected: iso === props.modelValue,
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

    <Popover
      :open="open"
      :anchor="{ type: 'element', element: () => triggerRef }"
      placement="bottom-start"
      role="dialog"
      :offset="4"
      :auto-focus="false"
      popover-class="date-picker__popover"
      @close="open = false"
    >
      <header class="date-picker__header">
        <button
          type="button"
          class="date-picker__nav"
          :aria-label="t('date-picker-prev-month-aria')"
          @click="shiftMonth(-1)"
        >
          <Icon name="chevronLeft" class="w-3.5 h-3.5" />
        </button>
        <span class="date-picker__month-label">{{ monthLabel }}</span>
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

      <div class="date-picker__grid">
        <button
          v-for="(cell, i) in cells"
          :key="i"
          type="button"
          class="date-picker__cell"
          :class="{
            'is-other-month': !cell.inCurrentMonth,
            'is-today': cell.isToday,
            'is-selected': cell.isSelected,
            'is-disabled': cell.disabled,
          }"
          :disabled="cell.disabled"
          :aria-current="cell.isToday ? 'date' : undefined"
          :aria-pressed="cell.isSelected"
          @click="applyDate(cell.year, cell.month, cell.day)"
        >
          {{ cell.day }}
        </button>
      </div>
    </Popover>
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

.date-picker__month-label {
  flex: 1;
  text-align: center;
  font-size: 12px;
  font-weight: 600;
  color: var(--color-primary);
  text-transform: capitalize;
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
