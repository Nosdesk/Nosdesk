<!--
Shared number-input primitive: label + numeric input with explicit
± stepper buttons + hint/error, in the same shell as FormInput so
forms that mix the two stay visually coherent.

On Reka's NumberField: the input is `type="text"` with the right
`inputmode` for mobile keyboards, parsing and formatting go through
`Intl.NumberFormat` in the active locale (so "1,5" is a decimal for a
French user), keystrokes that cannot become a number are refused,
ArrowUp/Down and PageUp/Down step, Home/End jump to the bounds, the
steppers hold-to-repeat, and `aria-valuenow/min/max` are on the input.
Wheel-to-change stays off: scrolling a form must not edit it.

Behaviour:
- modelValue: number | null. Null means "empty"; the input renders
  the empty string. Required fields catch null at submit time.
- The value commits on blur, Enter and the steppers, clamped to
  min / max and snapped to `step` (unless `stepSnapping` is off);
  mid-typing text is not pushed.
- integer: no fraction digits, so a decimal separator is refused.
- Grouping separators are off: every field here is a port, a count
  or an order, never a quantity to read in thousands.

Arbitrary native attributes (name, autocomplete, @blur, ...) fall
through to the inner <input>; a `class` on the component lands on
the wrapper for layout.
-->
<script setup lang="ts">
import { computed, useId } from 'vue';
import { NumberFieldDecrement, NumberFieldIncrement, NumberFieldInput, NumberFieldRoot } from 'reka-ui';
import { useFluent } from 'fluent-vue';

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
  /** Lower bound; clamped on commit and on every stepper click. */
  min?: number;
  /** Upper bound; clamped on commit and on every stepper click. */
  max?: number;
  /** Stepper increment + ArrowUp/Down step. Default 1. */
  step?: number;
  /** Snap the committed value to a multiple of `step`. Off, `step`
   * only drives the steppers and any typed decimal is kept. */
  stepSnapping?: boolean;
  /** Whole numbers only. */
  integer?: boolean;
}

defineOptions({ inheritAttrs: false });

const props = withDefaults(defineProps<Props>(), {
  size: 'md',
  step: 1,
  stepSnapping: true,
  integer: false,
});

const model = defineModel<number | null>({ required: true });

const fluent = useFluent();
const generatedId = useId();
const inputId = computed(() => props.id ?? generatedId);
const describedById = computed(() =>
  props.error || props.description ? `${inputId.value}-desc` : undefined,
);

// Intl's default of three fraction digits would round a typed
// decimal; the field keeps what was typed and leaves rounding to
// `step`.
const formatOptions = computed<Intl.NumberFormatOptions>(() => ({
  useGrouping: false,
  maximumFractionDigits: props.integer ? 0 : 20,
}));

// Reka reports an empty field as undefined (and NaN mid-clear).
function onUpdate(value: number | undefined) {
  model.value = value == null || Number.isNaN(value) ? null : value;
}

const stepperClasses = computed(() => [
  'px-3 text-secondary hover:text-primary hover:bg-surface-hover disabled:opacity-30 disabled:cursor-not-allowed transition-colors leading-none flex items-center justify-center select-none',
  props.size === 'sm' ? 'text-base' : 'text-lg',
]);
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
    <NumberFieldRoot
      :id="inputId"
      :model-value="model ?? undefined"
      :min="min"
      :max="max"
      :step="step"
      :step-snapping="stepSnapping"
      :format-options="formatOptions"
      :disabled="disabled"
      :required="required"
      disable-wheel-change
      :class="[
        'flex items-stretch w-full bg-surface-alt border rounded-lg overflow-hidden transition-colors',
        'focus-within:ring-2 focus-within:ring-accent focus-within:border-accent',
        disabled ? 'opacity-50' : '',
        error ? 'border-status-error' : 'border-subtle',
      ]"
      @update:model-value="onUpdate"
    >
      <NumberFieldDecrement as-child>
        <button
          type="button"
          :aria-label="fluent.$t('form-number-decrement')"
          :class="[stepperClasses, 'border-r border-subtle']"
        >
          &minus;
        </button>
      </NumberFieldDecrement>
      <!-- The root's id lands on the input. Reka's English role
           description is dropped; `spinbutton` already says it. -->
      <NumberFieldInput
        :placeholder="placeholder"
        :aria-roledescription="undefined"
        :required="required"
        :aria-invalid="error ? 'true' : undefined"
        :aria-describedby="describedById"
        :class="[
          'flex-1 min-w-0 bg-transparent text-primary placeholder-tertiary text-center tabular-nums',
          'focus:outline-none',
          'disabled:cursor-not-allowed',
          size === 'sm' ? 'px-2 py-1.5 text-sm' : 'px-3 py-2',
        ]"
        v-bind="{ ...$attrs, class: undefined }"
      />
      <NumberFieldIncrement as-child>
        <button
          type="button"
          :aria-label="fluent.$t('form-number-increment')"
          :class="[stepperClasses, 'border-l border-subtle']"
        >
          +
        </button>
      </NumberFieldIncrement>
    </NumberFieldRoot>
    <p v-if="error" :id="describedById" class="text-xs text-status-error">{{ error }}</p>
    <p v-else-if="description" :id="describedById" class="text-xs text-tertiary">
      {{ description }}
    </p>
  </div>
</template>
