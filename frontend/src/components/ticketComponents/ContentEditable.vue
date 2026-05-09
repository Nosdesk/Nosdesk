<script setup lang="ts">
import { computed, ref, watch, onMounted } from 'vue';

interface Props {
  modelValue: string;
  tag?: string;
  /** Hard character cap on the contenteditable. When set, paste
   *  + typing both clamp at this length and the over-cap suffix
   *  is silently dropped. Used by the ticket title editor to
   *  enforce the backend's `tickets.title VARCHAR(255)` limit
   *  client-side rather than waiting for a 500 on save.
   *  Counter renders below the field as the user approaches the
   *  cap so they don't run into the wall blind. */
  maxLength?: number;
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
const isFocused = ref(false);

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

/** Truncate a value to the configured maxLength. No-op when
 *  maxLength is unset or the value already fits. Returns the
 *  clamped string so callers can compare against the original
 *  to detect a truncation. */
function clamp(value: string): string {
  if (props.maxLength == null) return value;
  return value.length > props.maxLength
    ? value.slice(0, props.maxLength)
    : value;
}

// Handle input events - emit immediately for instant UI updates
const handleInput = () => {
  if (contentRef.value) {
    let newValue = contentRef.value.textContent || '';
    // Clamp paste / IME input that overshoots the cap. Reset the
    // contenteditable to the clamped value AND restore the cursor
    // to the end so the user sees their typing continue at the
    // tail rather than getting silently bounced.
    const clamped = clamp(newValue);
    if (clamped !== newValue) {
      contentRef.value.textContent = clamped;
      placeCaretAtEnd(contentRef.value);
      newValue = clamped;
    }
    if (newValue !== lastValue.value) {
      lastValue.value = newValue;
      emit('update:modelValue', newValue); // Immediate emit - no delay!
    }
  }
};

/** After programmatically resetting the contenteditable's text
 *  on overflow, the caret jumps to the start of the field — which
 *  reads as the field "rejecting" the input. Move it back to the
 *  end so the user's next keystroke continues at the tail. */
function placeCaretAtEnd(el: HTMLElement) {
  const range = document.createRange();
  range.selectNodeContents(el);
  range.collapse(false);
  const sel = window.getSelection();
  sel?.removeAllRanges();
  sel?.addRange(range);
}

const charCount = computed<number>(() => lastValue.value.length);
const showCounter = computed<boolean>(() => {
  if (props.maxLength == null) return false;
  // Render the counter when the field is focused and the user is
  // within 20 chars of the cap, OR always when actually at the
  // cap. Idle / unfocused fields stay clean.
  if (charCount.value >= props.maxLength) return true;
  return isFocused.value && charCount.value >= props.maxLength - 20;
});

// Handle paste to strip formatting. Pre-clamping the pasted text
// against any remaining capacity means a 1000-char paste into an
// almost-full field doesn't blow past the cap and get silently
// truncated by the input handler below — better feedback.
const handlePaste = (e: ClipboardEvent) => {
  e.preventDefault();
  const text = e.clipboardData?.getData('text/plain');
  if (!text) return;
  const remaining = props.maxLength != null
    ? Math.max(0, props.maxLength - charCount.value)
    : text.length;
  if (remaining <= 0) return;
  const toInsert = props.maxLength != null && text.length > remaining
    ? text.slice(0, remaining)
    : text;
  document.execCommand('insertText', false, toInsert);
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
  isFocused.value = true;
  emit('focus');
};

const handleBlur = () => {
  isFocused.value = false;
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
  <div class="w-full">
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
    <!-- Character counter. Only renders near the cap so the
         field stays clean for short titles. Goes red at the
         hard limit so the user understands further keystrokes
         won't land. -->
    <div
      v-if="showCounter"
      class="px-2 pt-0.5 text-[10px] tabular-nums"
      :class="charCount >= (maxLength ?? 0)
        ? 'text-status-error'
        : 'text-tertiary'"
      aria-live="polite"
    >
      {{ charCount }} / {{ maxLength }}
    </div>
  </div>
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