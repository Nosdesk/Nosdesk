<template>
  <div class="relative">
    <!-- Visual digit boxes -->
    <div class="flex gap-1.5 sm:gap-2">
      <div
        v-for="i in length"
        :key="i"
        class="w-10 sm:w-11 h-12 sm:h-14 bg-surface-alt border rounded-lg flex items-center justify-center text-primary text-lg sm:text-xl font-mono transition-colors"
        :class="[
          isFocused && modelValue.length === i - 1 ? 'border-blue-500 ring-2 ring-blue-500/50' :
          modelValue.length >= i ? 'border-strong' : 'border-subtle'
        ]"
      >
        {{ modelValue[i - 1] || '' }}
      </div>
    </div>

    <!-- Hidden input for interaction and autofill -->
    <input
      ref="inputRef"
      type="text"
      inputmode="numeric"
      :maxlength="length"
      autocomplete="one-time-code"
      :value="modelValue"
      @input="handleInput"
      @paste="handlePaste"
      @focus="isFocused = true"
      @blur="isFocused = false"
      @keydown="handleKeydown"
      class="absolute inset-0 w-full h-full bg-transparent border-none outline-none cursor-text z-10"
      style="color: transparent; -webkit-text-fill-color: transparent; caret-color: transparent;"
      :aria-label="ariaLabel"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue';

const props = withDefaults(defineProps<{
  modelValue: string;
  length?: number;
  ariaLabel?: string;
}>(), {
  length: 6,
  ariaLabel: 'One-time password'
});

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
  (e: 'complete', value: string): void;
}>();

const inputRef = ref<HTMLInputElement | null>(null);
const isFocused = ref(false);

// Handle input changes
const handleInput = (event: Event) => {
  const target = event.target as HTMLInputElement;
  const value = target.value.replace(/[^0-9]/g, '').slice(0, props.length);

  emit('update:modelValue', value);
  target.value = value;

  if (value.length === props.length) {
    emit('complete', value);
  }
};

// Handle paste
const handlePaste = (event: ClipboardEvent) => {
  event.preventDefault();
  const pastedText = event.clipboardData?.getData('text') || '';
  const cleanValue = pastedText.replace(/[^0-9]/g, '').slice(0, props.length);

  emit('update:modelValue', cleanValue);

  if (inputRef.value) {
    inputRef.value.value = cleanValue;
  }

  if (cleanValue.length === props.length) {
    emit('complete', cleanValue);
  }
};

// Handle keydown
const handleKeydown = (event: KeyboardEvent) => {
  const key = event.key;

  // Allow navigation and control keys
  if (['Tab', 'ArrowLeft', 'ArrowRight', 'Backspace', 'Delete'].includes(key)) {
    return;
  }

  // Allow clipboard shortcuts (Ctrl+V, Cmd+V, etc.)
  if (event.ctrlKey || event.metaKey) {
    return;
  }

  // Only allow numeric input
  if (!/^[0-9]$/.test(key)) {
    event.preventDefault();
  }
};

// Focus the input programmatically
const focus = () => {
  inputRef.value?.focus();
};

// Expose focus method
defineExpose({ focus });
</script>
