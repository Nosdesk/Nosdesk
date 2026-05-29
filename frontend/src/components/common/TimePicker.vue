<script setup lang="ts">
/**
 * 24-hour time picker. Compact text input + popover grid, modelled
 * on Linear / Notion / Airtable's pattern: type literal HH:MM when
 * you know the value, click hour + minute cells when you don't.
 * Arrow keys on the focused input step by `minuteStep`.
 *
 * Why a custom primitive rather than `<input type="time">`:
 *   - Browser chrome is browser-controlled; we can't actually own
 *     the visual to match the form's theme tokens.
 *   - The picker indicator renders differently in Chrome / Safari /
 *     Firefox, so any styling becomes a workaround per engine.
 *   - The form this lives in already uses SearchableDropdown for
 *     timezone; the time field should read as the same family of
 *     control, not a foreign browser widget.
 *
 * The value contract is HH:MM (24h, zero-padded). Empty string is
 * allowed for unset; invalid strings are accepted into the local
 * draft but not committed until they parse. Bad text shows in red
 * until the user fixes it or blurs to a valid value.
 */
import { computed, nextTick, ref, watch, type Ref } from 'vue'
import { useFluent } from 'fluent-vue'
import Popover from '@/components/common/Popover.vue'

interface Props {
  modelValue: string
  size?: 'sm' | 'md'
  /** Cell granularity in minutes. 5 by default; common alternatives
   *  are 15 (quarter-hour) or 30 (half-hour). Hours are always full. */
  minuteStep?: number
  ariaLabel?: string
  disabled?: boolean
  /** Mark the field invalid (e.g. parent-side validation). Tints
   *  the input + the popover header so the user sees the issue. */
  error?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  size: 'sm',
  minuteStep: 5,
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

// External writes win over local draft; the draft is purely the
// in-flight typing buffer. Watching modelValue keeps the input in
// sync when the parent corrects the value programmatically.
watch(
  () => props.modelValue,
  (v) => {
    draftText.value = v
    isInvalid.value = false
  },
)

const HHMM_RE = /^([01]\d|2[0-3]):[0-5]\d$/

function clampMinuteToStep(minute: number, step: number): number {
  // For a HIGHLIGHT indicator we want the nearest step-aligned
  // value that still falls within [0, 60). Rounding (e.g. minute=59,
  // step=5 -> 12 * 5 = 60) can overshoot, so cap at the largest
  // valid step.
  const rounded = Math.round(minute / step) * step
  return Math.min(rounded, 60 - step)
}

function commitText(): void {
  const v = draftText.value.trim()
  if (v === '') {
    if (props.modelValue !== '') emit('update:modelValue', '')
    isInvalid.value = false
    return
  }
  if (HHMM_RE.test(v)) {
    isInvalid.value = false
    if (v !== props.modelValue) emit('update:modelValue', v)
  } else {
    isInvalid.value = true
  }
}

function applyParts(hour: number, minute: number): void {
  const v = `${String(hour).padStart(2, '0')}:${String(minute).padStart(2, '0')}`
  draftText.value = v
  isInvalid.value = false
  if (v !== props.modelValue) emit('update:modelValue', v)
}

const currentParts = computed(() => {
  const v = props.modelValue
  if (!HHMM_RE.test(v)) return { hour: 9, minute: 0 }
  const [h, m] = v.split(':').map(Number)
  return { hour: h, minute: m }
})

const hours = Array.from({ length: 24 }, (_, i) => i)
const minutes = computed(() => {
  const step = props.minuteStep
  return Array.from({ length: Math.floor(60 / step) }, (_, i) => i * step)
})

/**
 * The current minute may not land on a step boundary (when the
 * model value was entered freehand or imported). Highlight the
 * nearest step cell so the picker reads consistently without
 * silently snapping the actual value.
 */
const highlightedMinute = computed(() =>
  clampMinuteToStep(currentParts.value.minute, props.minuteStep),
)

function handleKeyDown(e: KeyboardEvent): void {
  if (props.disabled) return
  if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
    if (!HHMM_RE.test(props.modelValue)) return
    e.preventDefault()
    const dir = e.key === 'ArrowUp' ? 1 : -1
    const [h, m] = props.modelValue.split(':').map(Number)
    const totalMinutes = h * 60 + m + dir * props.minuteStep
    const wrapped = ((totalMinutes % 1440) + 1440) % 1440
    applyParts(Math.floor(wrapped / 60), wrapped % 60)
  }
}

// Scroll the currently-selected cell into view when the popover
// opens, so a value like 17:00 isn't hidden below the fold of a
// 24-row column.
const hourColRef = ref<HTMLElement | null>(null)
const minuteColRef = ref<HTMLElement | null>(null)

function scrollSelectedIntoView(col: HTMLElement | null): void {
  if (!col) return
  const selected = col.querySelector<HTMLElement>('[data-selected="true"]')
  if (selected) {
    selected.scrollIntoView({ block: 'center' })
  }
}

watch(open, async (isOpen) => {
  if (!isOpen) return
  await nextTick()
  scrollSelectedIntoView(hourColRef.value)
  scrollSelectedIntoView(minuteColRef.value)
})

function focusInput(): void {
  triggerRef.value?.focus()
}

defineExpose({ focus: focusInput })
</script>

<template>
  <div class="time-picker">
    <input
      ref="triggerRef"
      v-model="draftText"
      type="text"
      inputmode="numeric"
      maxlength="5"
      :aria-label="ariaLabel"
      :disabled="disabled"
      :aria-invalid="isInvalid || error ? 'true' : undefined"
      class="time-picker__input"
      :class="[
        size === 'sm' ? 'time-picker__input--sm' : 'time-picker__input--md',
        (isInvalid || error) && 'time-picker__input--invalid',
      ]"
      @focus="open = true"
      @click="open = true"
      @blur="commitText"
      @keydown="handleKeyDown"
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
      popover-class="time-picker__popover"
      @close="open = false"
    >
      <div class="time-picker__grid">
        <ul ref="hourColRef" class="time-picker__col" :aria-label="t('time-picker-hours-aria')">
          <li v-for="h in hours" :key="h">
            <button
              type="button"
              class="time-picker__cell"
              :class="h === currentParts.hour && 'is-selected'"
              :data-selected="h === currentParts.hour ? 'true' : 'false'"
              @click="applyParts(h, highlightedMinute)"
            >
              {{ String(h).padStart(2, '0') }}
            </button>
          </li>
        </ul>
        <ul ref="minuteColRef" class="time-picker__col" :aria-label="t('time-picker-minutes-aria')">
          <li v-for="m in minutes" :key="m">
            <button
              type="button"
              class="time-picker__cell"
              :class="m === highlightedMinute && 'is-selected'"
              :data-selected="m === highlightedMinute ? 'true' : 'false'"
              @click="applyParts(currentParts.hour, m)"
            >
              {{ String(m).padStart(2, '0') }}
            </button>
          </li>
        </ul>
      </div>
    </Popover>
  </div>
</template>

<style scoped>
.time-picker {
  position: relative;
  display: inline-block;
}

.time-picker__input {
  background-color: var(--color-surface-alt);
  border: 1px solid var(--color-subtle);
  border-radius: 0.5rem;
  color: var(--color-primary);
  font-variant-numeric: tabular-nums;
  font-feature-settings: 'tnum';
  text-align: center;
  transition: border-color 150ms ease, box-shadow 150ms ease;
  width: 4.5rem;
}

.time-picker__input--sm {
  padding: 0.25rem 0.5rem;
  font-size: 12px;
  line-height: 1.4;
}

.time-picker__input--md {
  padding: 0.5rem 0.75rem;
  font-size: 14px;
}

.time-picker__input:focus {
  outline: none;
  border-color: var(--color-accent);
  box-shadow: 0 0 0 2px var(--color-accent-muted);
}

.time-picker__input--invalid {
  border-color: var(--color-status-error);
}

.time-picker__input:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>

<style>
/* Unscoped because the popover is teleported to <body> and the
   scoped attribute hash never lands on its DOM tree. Class is
   prefixed so the rule still reads as belonging to this component. */
.time-picker__popover {
  background-color: var(--color-surface);
  border: 1px solid var(--color-default);
  border-radius: 0.5rem;
  box-shadow: 0 10px 25px -10px rgba(0, 0, 0, 0.2);
  overflow: hidden;
}

.time-picker__grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 1px;
  background-color: var(--color-subtle);
}

.time-picker__col {
  background-color: var(--color-surface);
  list-style: none;
  margin: 0;
  padding: 0.25rem 0;
  max-height: 14rem;
  overflow-y: auto;
  scrollbar-width: thin;
}

.time-picker__cell {
  display: block;
  width: 100%;
  padding: 0.25rem 1rem;
  background: transparent;
  border: none;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  font-feature-settings: 'tnum';
  color: var(--color-secondary);
  text-align: center;
  cursor: pointer;
  transition: background-color 100ms ease, color 100ms ease;
}

.time-picker__cell:hover {
  background-color: var(--color-surface-hover);
  color: var(--color-primary);
}

.time-picker__cell:focus-visible {
  outline: none;
  background-color: var(--color-surface-hover);
  color: var(--color-primary);
  box-shadow: inset 0 0 0 1px var(--color-accent);
}

.time-picker__cell.is-selected {
  background-color: var(--color-accent-muted);
  color: var(--color-accent);
  font-weight: 600;
}
</style>
