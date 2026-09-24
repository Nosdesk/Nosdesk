<!--
Password field with a show/hide toggle. With `label`, it renders the same
label + hint/error frame as FormInput; without one it is the bare input, for
callers that lay out their own label.
-->
<template>
  <div :class="label || error || description ? 'flex flex-col gap-1.5' : undefined">
    <label
      v-if="label"
      :for="inputId"
      class="text-xs font-medium text-tertiary uppercase tracking-wide"
    >
      {{ label }}<span v-if="required" class="text-status-error ml-0.5" aria-hidden="true">*</span>
    </label>
    <div class="relative">
      <input
        :id="inputId"
        :value="modelValue"
        @input="$emit('update:modelValue', ($event.target as HTMLInputElement).value)"
        :type="showPassword ? 'text' : 'password'"
        :placeholder="placeholder"
        :required="required"
        :autocomplete="autocomplete"
        :disabled="disabled"
        :aria-invalid="error ? 'true' : undefined"
        :aria-describedby="describedById"
        :class="[
          'block w-full px-3 py-2 pr-10 border rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent disabled:opacity-50 disabled:cursor-not-allowed',
          label ? 'bg-surface-alt' : 'bg-surface',
          error ? 'border-status-error' : label ? 'border-subtle' : 'border-default',
          inputClass
        ]"
      />
      <button
        type="button"
        tabindex="-1"
        @click="showPassword = !showPassword"
        class="absolute inset-y-0 right-0 px-3 flex items-center text-tertiary hover:text-secondary transition-colors"
        :aria-label="showPassword ? 'Hide password' : 'Show password'"
        :disabled="disabled"
      >
        <Icon v-if="!showPassword" name="eye" />
        <Icon v-else name="eyeOff" />
      </button>
    </div>
    <p v-if="error" :id="describedById" class="text-xs text-status-error">{{ error }}</p>
    <p v-else-if="description" :id="describedById" class="text-xs text-tertiary">
      {{ description }}
    </p>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, useId } from 'vue';
import Icon from '@/components/common/Icon.vue';

const props = defineProps<{
  modelValue: string;
  id?: string;
  label?: string;
  /** Helper text shown below the field. */
  description?: string;
  /** Error text shown below the field; also flags aria-invalid. */
  error?: string;
  placeholder?: string;
  required?: boolean;
  autocomplete?: string;
  disabled?: boolean;
  inputClass?: string;
}>();

defineEmits<{
  (e: 'update:modelValue', value: string): void;
}>();

const showPassword = ref(false);
const generatedId = useId();
const inputId = computed(() => props.id ?? generatedId);
const describedById = computed(() =>
  props.error || props.description ? `${inputId.value}-desc` : undefined,
);
</script>
