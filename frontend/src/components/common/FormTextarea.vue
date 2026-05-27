<!--
Shared multi-line text input: the FormInput field styling applied to a
<textarea>, with label + hint/error and programmatic label association.
`mono` switches to a monospace face (signatures, code-ish content).
Arbitrary native attributes (name, maxlength, readonly, @blur, ...) fall
through to the <textarea>; a `class` on the component lands on the
wrapper.
-->
<script setup lang="ts">
import { computed, useId } from 'vue';

interface Props {
  label?: string;
  placeholder?: string;
  description?: string;
  error?: string;
  required?: boolean;
  disabled?: boolean;
  rows?: number;
  /** Render the textarea in a monospace face. */
  mono?: boolean;
  id?: string;
}

defineOptions({ inheritAttrs: false });

const props = withDefaults(defineProps<Props>(), {
  rows: 4,
  mono: false,
});

const model = defineModel<string>({ required: true });

const generatedId = useId();
const textareaId = computed(() => props.id ?? generatedId);
const describedById = computed(() =>
  props.error || props.description ? `${textareaId.value}-desc` : undefined,
);
</script>

<template>
  <div class="flex flex-col gap-1.5" :class="$attrs.class">
    <label
      v-if="label"
      :for="textareaId"
      class="text-xs font-medium text-tertiary uppercase tracking-wide"
    >
      {{ label }}<span v-if="required" class="text-status-error ml-0.5" aria-hidden="true">*</span>
    </label>
    <textarea
      :id="textareaId"
      v-model="model"
      :placeholder="placeholder"
      :required="required"
      :disabled="disabled"
      :rows="rows"
      :aria-invalid="error ? 'true' : undefined"
      :aria-describedby="describedById"
      :class="[
        'w-full bg-surface-alt border rounded-lg text-primary placeholder-tertiary transition-colors resize-y',
        'focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:border-accent',
        'disabled:opacity-50 disabled:cursor-not-allowed',
        'px-3 py-2',
        mono ? 'font-mono text-sm' : '',
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
