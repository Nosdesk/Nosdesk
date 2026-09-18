<script setup lang="ts">
/**
 * Time field on Reka's TimeField: a `role=group` of `spinbutton`
 * segments (hour, minute, and a day-period segment when the locale
 * clock is 12-hour), typed digit by digit, arrows step the focused
 * segment (minutes by `minuteStep`), Left/Right move between segments,
 * Backspace clears. Native `<input type="time">` chrome is
 * browser-controlled, so the control is ours to theme; the hour and
 * minute popover grid the old input carried is gone, the segments are
 * the picker.
 *
 * Value contract is unchanged: `HH:MM` (24-hour, zero-padded), empty
 * string means unset. A 12-hour locale shows and takes 12-hour input;
 * the model stays 24-hour. The field commits when focus leaves it and
 * on Enter, not on every keystroke: an hour typed digit by digit
 * passes through another complete time on the way.
 */
import { computed, onBeforeUnmount, ref, shallowRef, useId, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { parseTime, Time } from '@internationalized/date'
import { TimeFieldInput, TimeFieldRoot, type SegmentPart, type TimeValue } from 'reka-ui'

interface Props {
  modelValue: string
  size?: 'sm' | 'md'
  /** Arrow-key step on the minute segment. Typed minutes are taken
   *  as they are, never snapped. */
  minuteStep?: number
  /** Label rendered above the field in the same shell as FormInput. */
  label?: string
  /** Accessible name when there is no visible label. */
  ariaLabel?: string
  disabled?: boolean
  /** Helper text shown below the field. */
  description?: string
  /** Error text shown below the field; the segments are flagged
   *  invalid. `true` flags them without a message. */
  error?: string | boolean
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

const generatedId = useId()
const inputId = `time-picker-${generatedId}`
const labelId = computed(() => (props.label ? `${inputId}-label` : undefined))
const errorText = computed(() => (typeof props.error === 'string' ? props.error : ''))
const describedById = computed(() =>
  errorText.value || props.description ? `${inputId}-desc` : undefined,
)

const HHMM_RE = /^([01]\d|2[0-3]):[0-5]\d$/

function toTime(value: string): Time | undefined {
  return HHMM_RE.test(value) ? parseTime(value) : undefined
}

const toHhmm = (value: TimeValue | undefined | null): string =>
  value ? `${String(value.hour).padStart(2, '0')}:${String(value.minute).padStart(2, '0')}` : ''

// The field shows what was typed; the parent hears about it on commit.
// Parent writes replace the draft. Only an edit commits: a parent value
// the field could not parse is left alone, not cleared.
let dirty = false
const draft = shallowRef<Time | undefined>(toTime(props.modelValue))
watch(
  () => props.modelValue,
  (v) => {
    draft.value = toTime(v)
    dirty = false
  },
)

function onUpdate(value: TimeValue | undefined): void {
  draft.value = value ? new Time(value.hour, value.minute) : undefined
  dirty = true
}

function commit(): void {
  if (!dirty) return
  dirty = false
  const next = toHhmm(draft.value)
  if (next !== props.modelValue) emit('update:modelValue', next)
}

// Focus leaving the control commits; moving between segments does
// not. A host popover can unmount the field before any blur lands, so
// unmount commits too.
const rootEl = ref<HTMLElement | null>(null)
function onFocusOut(event: FocusEvent): void {
  const next = event.relatedTarget
  if (next instanceof Node && rootEl.value?.contains(next)) return
  commit()
}
onBeforeUnmount(commit)

const step = computed(() => ({ minute: props.minuteStep }))

// Reka labels segments in English ("hour, ", "AM/PM"); ours come from
// the catalogue.
const SEGMENT_KEYS: Partial<Record<SegmentPart, string>> = {
  hour: 'date-segment-hour',
  minute: 'date-segment-minute',
  second: 'date-segment-second',
  dayPeriod: 'date-segment-day-period',
}

function segmentAttrs(part: SegmentPart): Record<string, string> {
  const key = SEGMENT_KEYS[part]
  if (!key) return {}
  const attrs: Record<string, string> = { 'aria-label': t(key) }
  if (props.error) attrs['aria-invalid'] = 'true'
  return attrs
}
</script>

<template>
  <div ref="rootEl" class="time-picker" @focusout="onFocusOut">
    <label
      v-if="label"
      :id="labelId"
      :for="inputId"
      class="text-xs font-medium text-tertiary uppercase tracking-wide"
    >
      {{ label }}
    </label>
    <TimeFieldRoot
      v-slot="{ segments }"
      :id="inputId"
      :model-value="draft"
      granularity="minute"
      :step="step"
      :step-snapping="false"
      :disabled="disabled"
      class="time-picker__field"
      :class="[
        size === 'sm' ? 'time-picker__field--sm' : 'time-picker__field--md',
        error && 'time-picker__field--invalid',
      ]"
      :aria-label="label ? undefined : ariaLabel"
      :aria-labelledby="labelId"
      :aria-describedby="describedById"
      @update:model-value="onUpdate"
      @keydown.enter="commit"
    >
      <TimeFieldInput
        v-for="(item, i) in segments"
        :key="i"
        :part="item.part"
        class="time-picker__segment"
        v-bind="segmentAttrs(item.part)"
      >
        {{ item.value }}
      </TimeFieldInput>
    </TimeFieldRoot>
    <p v-if="errorText" :id="describedById" class="text-xs text-status-error">{{ errorText }}</p>
    <p v-else-if="description" :id="describedById" class="text-xs text-tertiary">{{ description }}</p>
  </div>
</template>

<style scoped>
.time-picker {
  display: inline-flex;
  flex-direction: column;
  gap: 0.375rem;
}
</style>

<style>
/* Unscoped: the field and its segments are Reka-rendered, so scoped
   hashes never reach them. The class prefix keeps the rules logically
   scoped. */
.time-picker__field {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background-color: var(--color-surface-alt);
  border: 1px solid var(--color-subtle);
  border-radius: 0.5rem;
  color: var(--color-primary);
  font-variant-numeric: tabular-nums;
  font-feature-settings: 'tnum';
  transition:
    border-color 150ms ease,
    box-shadow 150ms ease;
  min-width: 4.5rem;
}

.time-picker__field--sm {
  padding: 0.25rem 0.5rem;
  font-size: 12px;
  line-height: 1.4;
}

.time-picker__field--md {
  padding: 0.5rem 0.75rem;
  font-size: 14px;
}

.time-picker__field:focus-within {
  border-color: var(--color-accent);
  box-shadow: 0 0 0 2px var(--color-accent-muted);
}

.time-picker__field--invalid,
.time-picker__field[data-invalid] {
  border-color: var(--color-status-error);
}

.time-picker__field[data-disabled] {
  opacity: 0.5;
  cursor: not-allowed;
}

.time-picker__segment {
  padding: 0 1px;
  border-radius: 0.125rem;
  outline: none;
  caret-color: transparent;
}

.time-picker__segment[data-placeholder] {
  color: var(--color-tertiary);
}

.time-picker__segment[data-reka-time-field-segment='literal'] {
  color: var(--color-tertiary);
  padding: 0;
}

.time-picker__segment:focus {
  background-color: var(--color-accent);
  color: var(--color-on-accent);
}
</style>
