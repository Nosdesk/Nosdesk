<script setup lang="ts">
import { ref, watch, nextTick } from 'vue';

interface Props {
  modelValue: string;
  placeholder?: string;
  textSize?: 'sm' | 'base' | 'lg' | 'xl' | '2xl';
  canEdit?: boolean;
  prefix?: string;
  showEditHint?: boolean;
  truncate?: boolean;
  /** Cap the display mode to N lines with ellipsis past that.
   * Overrides `truncate` when set. `2` is the recommended value
   * for header titles: a long title wraps once instead of being
   * lost to ellipsis, but never grows the row beyond two lines.
   * Unset = unlimited wrap (the previous `truncate: false` default). */
  maxLines?: 1 | 2;
}

const props = withDefaults(defineProps<Props>(), {
  placeholder: 'Enter text...',
  textSize: 'base',
  canEdit: true,
  prefix: '',
  showEditHint: true,
  truncate: false
});

const emit = defineEmits<{
  // Committed once per edit session (blur / Enter), only when the value
  // actually changed. This is the one that should write/audit.
  'update:modelValue': [value: string];
  // Transient draft on every keystroke. For live display and optional
  // SSE field-preview broadcast; never commits.
  'preview': [value: string];
}>();

const isEditing = ref(false);
const originalValue = ref(props.modelValue);
const inputRef = ref<HTMLInputElement | null>(null);

// Local value for editing - prevents cursor jumping from async parent updates
// This is the Vue best practice for controlled inputs
const localValue = ref(props.modelValue);

// Sync from the parent only when NOT editing. Mid-edit we must leave both
// refs alone: originalValue has to stay the pre-edit snapshot so commit-on-
// blur can tell a real change from a no-op, and localValue must keep the
// user's in-progress text (our own per-keystroke `preview` echoes back
// through modelValue, and so can a remote SSE update).
watch(() => props.modelValue, (newValue) => {
  if (!isEditing.value) {
    originalValue.value = newValue;
    localValue.value = newValue;
  }
});

// Auto-focus and snapshot the starting value when entering edit mode.
watch(isEditing, async (newValue) => {
  if (newValue) {
    originalValue.value = props.modelValue;
    localValue.value = props.modelValue;
    await nextTick();
    inputRef.value?.focus();
    inputRef.value?.select();
  }
});

const handleClick = () => {
  if (props.canEdit && !isEditing.value) {
    isEditing.value = true;
  }
};

const handleInput = (event: Event) => {
  const target = event.target as HTMLInputElement;
  localValue.value = target.value;
  // Per-keystroke draft only: drives live display / SSE preview, never commits.
  emit('preview', localValue.value);
};

const handleBlur = () => {
  if (isEditing.value) {
    isEditing.value = false;
    // Commit once at the end of the edit session, and only if it changed,
    // so an open-then-close with no edit (or an unchanged value) is a no-op.
    if (localValue.value !== originalValue.value) {
      emit('update:modelValue', localValue.value);
    }
  }
};

const handleKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Enter') {
    event.preventDefault();
    (event.target as HTMLInputElement).blur();
  } else if (event.key === 'Escape') {
    // Cancel: restore original value, exit, and reset any transient
    // preview state the parent built up from keystrokes.
    localValue.value = originalValue.value;
    isEditing.value = false;
    emit('preview', originalValue.value);
  }
};

// Text size classes
const textSizeClasses = {
  sm: 'text-sm',
  base: 'text-base',
  lg: 'text-lg',
  xl: 'text-xl',
  '2xl': 'text-2xl'
};
</script>

<template>
  <div class="flex items-center gap-3 group flex-1 min-w-0">
    <span
      v-if="prefix"
      class="text-tertiary font-medium flex items-center select-none flex-shrink-0"
      :class="[textSizeClasses[textSize], { 'opacity-50': isEditing }]"
    >
      {{ prefix }}
    </span>

    <div class="flex-1 relative min-w-0">
      <!-- Display mode - shows wrapped text or truncated text -->
      <div
        v-if="!isEditing"
        @click="handleClick"
        class="w-full font-semibold px-1 py-0.5 rounded-lg hover:bg-surface-hover transition-all duration-150 border-2 border-transparent"
        :class="[
          textSizeClasses[textSize],
          // `maxLines` takes precedence over `truncate`. line-clamp-2
          // wraps to two lines with ellipsis past that, paired with
          // leading-tight so two lines comfortably fit in a header
          // row sized for one. Falls back to single-line truncate or
          // unlimited break-words depending on the legacy
          // `truncate` flag.
          maxLines === 2
            ? 'line-clamp-2 leading-tight break-words'
            : maxLines === 1 || truncate
              ? 'truncate'
              : 'break-words',
          {
            'cursor-pointer': canEdit,
            'cursor-default': !canEdit,
            'text-primary': modelValue,
            'text-tertiary italic': !modelValue
          }
        ]"
        :title="(maxLines || truncate) && modelValue ? modelValue : undefined"
      >
        {{ modelValue || placeholder }}
      </div>

      <!-- Edit mode - input field using local value to preserve cursor position -->
      <input
        v-else
        :value="localValue"
        @input="handleInput"
        type="text"
        class="w-full bg-surface-hover text-primary font-semibold px-1 py-0.5 rounded-lg focus:bg-surface focus:outline-none transition-all duration-150 border-2 border-transparent focus:border-accent/50"
        :class="[
          textSizeClasses[textSize],
          'cursor-text'
        ]"
        :placeholder="placeholder"
        @blur="handleBlur"
        @keydown="handleKeydown"
        ref="inputRef"
      />

      <!-- Edit indicator -->
      <span
        v-if="!isEditing && canEdit && showEditHint"
        class="absolute right-3 top-1/2 -translate-y-1/2 text-tertiary text-sm opacity-0 group-hover:opacity-100 transition-opacity duration-200 pointer-events-none select-none"
      >
        Click to edit
      </span>
    </div>
  </div>
</template>

<style scoped>
.transition-all {
  transition-property: all;
  transition-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
}

@media (prefers-reduced-motion: reduce) {
  .transition-all {
    transition: opacity 0.1s ease-in-out;
    transform: none;
  }
}
</style> 