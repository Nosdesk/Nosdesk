<!--
Shared number-input primitive: label + numeric input with explicit
± stepper buttons + hint/error, in the same shell as FormInput so
forms that mix the two stay visually coherent.

Why not just `<input type="number">`: the native control has
inconsistent spinners across browsers (Chrome's are tiny, Firefox's
are different, Safari's barely render), the scroll-wheel-changes-
value misfeature is hard to disable cleanly, and HTML5 number
inputs accept Unicode decimal separators inconsistently across
locales. The input here is `type="text"` underneath with
`inputmode="numeric"` (or `decimal`) for mobile keyboards, and we
do our own numeric coercion + clamping in the component so the
behaviour is identical everywhere.

Behaviour:
- modelValue: number | null. Null means "empty"; the input renders
  the empty string. Required fields catch null at submit time.
- min / max: clamped on blur and on every stepper click.
- step: increment for the stepper buttons and ArrowUp/Down.
  Defaults to 1.
- integer: forces values to integers (truncates fractions on blur).
- ArrowUp / ArrowDown step by `step`.
- Disabled state: input read-only, both stepper buttons disabled.

Arbitrary native attributes (name, autocomplete, @blur, ...) fall
through to the inner <input>; a `class` on the component lands on
the wrapper for layout.
-->
<script setup lang="ts">
import { computed, ref, useId, watch } from 'vue';

type Size = 'sm' | 'md';

interface Props {
  label?: string;
  placeholder?: string;
  /** Helper text shown below the field. */
  description?: string;
  /** Error text shown below the field; also flags aria-invalid. */
  error?: string;
  required?: boolean;
  disabled?: boolean;
  size?: Size;
  /** Override the generated id (e.g. to point an external label at it). */
  id?: string;
  /** Lower bound; clamped on blur and on every stepper click. */
  min?: number;
  /** Upper bound; clamped on blur and on every stepper click. */
  max?: number;
  /** Stepper increment + ArrowUp/Down step. Default 1. */
  step?: number;
  /** Coerce to integer on blur (truncates fractions). */
  integer?: boolean;
}

defineOptions({ inheritAttrs: false });

const props = withDefaults(defineProps<Props>(), {
  size: 'md',
  step: 1,
  integer: false,
});

const model = defineModel<number | null>({ required: true });

const generatedId = useId();
const inputId = computed(() => props.id ?? generatedId);
const describedById = computed(() =>
  props.error || props.description ? `${inputId.value}-desc` : undefined,
);

// Local string for the input element. Lets the user type "-",
// "1.", "" etc. without us immediately coercing those mid-keystroke
// into something that doesn't round-trip. Coercion lands on blur.
const localValue = ref<string>(model.value == null ? '' : String(model.value));

// External model changes (parent reset, prop binding) should
// reflect in the input. Avoids the loop case by skipping when the
// local string already represents the same number.
watch(
  () => model.value,
  (next) => {
    const current = parseLocal(localValue.value);
    if (next === current) return;
    localValue.value = next == null ? '' : String(next);
  },
);

function parseLocal(raw: string): number | null {
  const trimmed = raw.trim();
  if (trimmed === '' || trimmed === '-' || trimmed === '.') return null;
  const n = Number(trimmed);
  if (Number.isNaN(n)) return null;
  return n;
}

function clamp(n: number): number {
  let result = n;
  if (props.integer) result = Math.trunc(result);
  if (props.min != null && result < props.min) result = props.min;
  if (props.max != null && result > props.max) result = props.max;
  return result;
}

function commit(): void {
  const parsed = parseLocal(localValue.value);
  if (parsed == null) {
    model.value = null;
    localValue.value = '';
    return;
  }
  const clamped = clamp(parsed);
  model.value = clamped;
  localValue.value = String(clamped);
}

function onInput(event: Event): void {
  // Live-update the local string; defer numeric coercion / clamping
  // to blur so the admin can type "12-3" -> backspace -> "123"
  // without the value flipping under them mid-keystroke.
  localValue.value = (event.target as HTMLInputElement).value;
  const parsed = parseLocal(localValue.value);
  if (parsed != null) {
    // Push intermediate parses to the model so consumers see live
    // updates; clamping waits for blur so the user isn't trapped
    // mid-edit.
    if (props.integer && !Number.isInteger(parsed)) {
      // Decimal mid-edit for integer-only field: hold the model at
      // its last known integer so v-model consumers don't see a
      // bogus fractional value. The blur clamp truncates.
      return;
    }
    model.value = parsed;
  } else {
    model.value = null;
  }
}

function bumpBy(direction: 1 | -1): void {
  if (props.disabled) return;
  const current = model.value ?? props.min ?? 0;
  const next = clamp(current + props.step * direction);
  model.value = next;
  localValue.value = String(next);
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key === 'ArrowUp') {
    event.preventDefault();
    bumpBy(1);
  } else if (event.key === 'ArrowDown') {
    event.preventDefault();
    bumpBy(-1);
  }
}

const stepUpDisabled = computed(
  () => props.disabled || (props.max != null && (model.value ?? props.min ?? 0) >= props.max),
);
const stepDownDisabled = computed(
  () => props.disabled || (props.min != null && (model.value ?? props.min) <= props.min),
);

const inputMode = computed(() => (props.integer ? 'numeric' : 'decimal'));
</script>

<template>
  <div class="flex flex-col gap-1.5" :class="$attrs.class">
    <label
      v-if="label"
      :for="inputId"
      class="text-xs font-medium text-tertiary uppercase tracking-wide"
    >
      {{ label
      }}<span v-if="required" class="text-status-error ml-0.5" aria-hidden="true">*</span>
    </label>
    <div
      :class="[
        'flex items-stretch w-full bg-surface-alt border rounded-lg overflow-hidden transition-colors',
        'focus-within:ring-2 focus-within:ring-accent focus-within:border-accent',
        disabled ? 'opacity-50' : '',
        error ? 'border-status-error' : 'border-subtle',
      ]"
    >
      <button
        type="button"
        :disabled="stepDownDisabled"
        :aria-label="($attrs['aria-label-decrement'] as string | undefined) ?? 'Decrement'"
        :class="[
          'px-3 text-secondary hover:text-primary hover:bg-surface-hover disabled:opacity-30 disabled:cursor-not-allowed transition-colors border-r border-subtle leading-none flex items-center justify-center',
          size === 'sm' ? 'text-base' : 'text-lg',
        ]"
        @click="bumpBy(-1)"
      >
        &minus;
      </button>
      <input
        :id="inputId"
        type="text"
        :value="localValue"
        :placeholder="placeholder"
        :required="required"
        :disabled="disabled"
        :inputmode="inputMode"
        :aria-invalid="error ? 'true' : undefined"
        :aria-describedby="describedById"
        :class="[
          'flex-1 min-w-0 bg-transparent text-primary placeholder-tertiary text-center tabular-nums',
          'focus:outline-none',
          'disabled:cursor-not-allowed',
          size === 'sm' ? 'px-2 py-1.5 text-sm' : 'px-3 py-2',
        ]"
        v-bind="{ ...$attrs, class: undefined, 'aria-label-decrement': undefined, 'aria-label-increment': undefined }"
        @input="onInput"
        @blur="commit"
        @keydown="onKeydown"
      />
      <button
        type="button"
        :disabled="stepUpDisabled"
        :aria-label="($attrs['aria-label-increment'] as string | undefined) ?? 'Increment'"
        :class="[
          'px-3 text-secondary hover:text-primary hover:bg-surface-hover disabled:opacity-30 disabled:cursor-not-allowed transition-colors border-l border-subtle leading-none flex items-center justify-center',
          size === 'sm' ? 'text-base' : 'text-lg',
        ]"
        @click="bumpBy(1)"
      >
        +
      </button>
    </div>
    <p v-if="error" :id="describedById" class="text-xs text-status-error">{{ error }}</p>
    <p v-else-if="description" :id="describedById" class="text-xs text-tertiary">
      {{ description }}
    </p>
  </div>
</template>
