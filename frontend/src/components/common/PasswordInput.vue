<template>
  <div class="relative">
    <input
      :id="id"
      :value="modelValue"
      @input="$emit('update:modelValue', ($event.target as HTMLInputElement).value)"
      :type="showPassword ? 'text' : 'password'"
      :placeholder="placeholder"
      :required="required"
      :autocomplete="autocomplete"
      :disabled="disabled"
      :class="[
        'block w-full px-3 py-2 pr-10 bg-surface border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent disabled:opacity-50 disabled:cursor-not-allowed',
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
      <!-- Eye icon (show) -->
      <Icon v-if="!showPassword" name="eye" />
      <!-- Eye-off icon (hide) -->
      <Icon v-else name="eyeOff" />
    </button>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import Icon from '@/components/common/Icon.vue';

defineProps<{
  modelValue: string;
  id?: string;
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
</script>
