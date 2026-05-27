<!--
Shared text-input primitive: label + input + hint/error, with the field
styling and focus ring unified in one place instead of repeated inline
class strings. The label is programmatically associated (generated id),
and the hint/error is wired via aria-describedby + aria-invalid.

Arbitrary native attributes (name, maxlength, pattern, readonly,
autocomplete, inputmode, @blur, ...) fall through to the inner <input>;
a `class` on the component lands on the wrapper for layout. For passwords
with a show/hide toggle use PasswordInput; for selects use BaseDropdown /
SearchableDropdown; for multi-line use FormTextarea.
-->
<script setup lang="ts">
import { computed, useId } from 'vue';

type Size = 'sm' | 'md';

interface Props {
  label?: string;
  type?: string;
  placeholder?: string;
  /** Helper text shown below the field. */
  description?: string;
  /** Error text shown below the field; also flags aria-invalid. */
  error?: string;
  required?: boolean;
  disabled?: boolean;
  size?: Size;
  /** Override the generated id (e.g. to point an external <label> at it). */
  id?: string;
}

defineOptions({ inheritAttrs: false });

const props = withDefaults(defineProps<Props>(), {
  type: 'text',
  size: 'md',
});

const model = defineModel<string>({ required: true });

const generatedId = useId();
const inputId = computed(() => props.id ?? generatedId);
const describedById = computed(() =>
  props.error || props.description ? `${inputId.value}-desc` : undefined,
);
</script>

<template>
  <div class="flex flex-col gap-1.5" :class="$attrs.class">
    <label
      v-if="label"
      :for="inputId"
      class="text-xs font-medium text-tertiary uppercase tracking-wide"
    >
      {{ label }}<span v-if="required" class="text-status-error ml-0.5" aria-hidden="true">*</span>
    </label>
    <input
      :id="inputId"
      v-model="model"
      :type="type"
      :placeholder="placeholder"
      :required="required"
      :disabled="disabled"
      :aria-invalid="error ? 'true' : undefined"
      :aria-describedby="describedById"
      :class="[
        'w-full bg-surface-alt border rounded-lg text-primary placeholder-tertiary transition-colors',
        'focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:border-accent',
        'disabled:opacity-50 disabled:cursor-not-allowed',
        size === 'sm' ? 'px-3 py-1.5 text-sm' : 'px-3 py-2',
        error ? 'border-status-error' : 'border-subtle',
      ]"
      v-bind="{ ...$attrs, class: undefined }"
    />
    <p v-if="error" :id="describedById" class="text-xs text-status-error">{{ error }}</p>
    <p v-else-if="description" :id="describedById" class="text-xs text-tertiary">
      {{ description }}
    </p>
  </div>
</template>
