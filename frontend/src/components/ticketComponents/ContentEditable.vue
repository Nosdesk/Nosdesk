<script setup lang="ts">
import { ref, watch, onMounted } from 'vue';

interface Props {
  modelValue: string;
  tag?: string;
}

const props = withDefaults(defineProps<Props>(), {
  tag: 'div'
});

const emit = defineEmits<{
  'update:modelValue': [value: string];
  'focus': [];
  'blur': [];
}>();

const contentRef = ref<HTMLElement | null>(null);
const lastValue = ref(props.modelValue);

// Update content when modelValue changes externally (from SSE)
// Only update if the element is not focused (user is not actively editing)
watch(() => props.modelValue, (newValue) => {
  if (contentRef.value && newValue !== lastValue.value) {
    // Don't update while user is actively editing - this would reset cursor
    const isEditing = document.activeElement === contentRef.value;
    if (!isEditing) {
      contentRef.value.textContent = newValue;
    }
    lastValue.value = newValue;
  }
});

// Handle input events - emit immediately for instant UI updates
const handleInput = () => {
  if (contentRef.value) {
    const newValue = contentRef.value.textContent || '';
    if (newValue !== lastValue.value) {
      lastValue.value = newValue;
      emit('update:modelValue', newValue); // Immediate emit - no delay!
    }
  }
};

// Handle paste to strip formatting
const handlePaste = (e: ClipboardEvent) => {
  e.preventDefault();
  const text = e.clipboardData?.getData('text/plain');
  if (text) {
    document.execCommand('insertText', false, text);
  }
};

// Prevent Enter key from creating new lines in title
const handleKeydown = (e: KeyboardEvent) => {
  if (e.key === 'Enter') {
    e.preventDefault();
    // Just blur the field to save, don't insert newline
    (e.target as HTMLElement).blur();
  }
};

// Emit focus/blur events for SSE coordination
const handleFocus = () => {
  emit('focus');
};

const handleBlur = () => {
  emit('blur');
};

// Initialize content on mount
onMounted(() => {
  if (contentRef.value && props.modelValue) {
    contentRef.value.textContent = props.modelValue;
  }
});
</script>

<template>
  <component
    :is="tag"
    ref="contentRef"
    contenteditable="true"
    @input="handleInput"
    @paste="handlePaste"
    @keydown="handleKeydown"
    @focus="handleFocus"
    @blur="handleBlur"
    class="w-full min-h-[1.75rem] px-2 py-1 text-sm text-primary rounded outline-none transition-all whitespace-pre-wrap break-words"
    spellcheck="true"
  />
</template>

<style scoped>
/* Focus styling */
[contenteditable]:focus {
  background-color: var(--bg-surface-hover);
}

/* Hover styling */
[contenteditable]:hover {
  background-color: var(--bg-surface-hover);
  opacity: 0.7;
}

/* Remove the ugly focus ring and use a subtle glow instead */
[contenteditable]:focus-visible {
  outline: none;
  box-shadow: 0 0 0 1px rgb(59 130 246 / 0.3); /* subtle blue glow */
}
</style>