<script setup lang="ts">
import { ref, watch, nextTick } from 'vue';

interface Props {
  modelValue: string;
  placeholder?: string;
  textSize?: 'sm' | 'base' | 'lg' | 'xl' | '2xl';
  canEdit?: boolean;
  prefix?: string;
  showEditHint?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  placeholder: 'Enter text...',
  textSize: 'base',
  canEdit: true,
  prefix: '',
  showEditHint: true
});

const emit = defineEmits<{
  'update:modelValue': [value: string];
}>();

const isEditing = ref(false);
const originalValue = ref(props.modelValue);
const inputRef = ref<HTMLInputElement | null>(null);

// Update original value when modelValue changes from parent
watch(() => props.modelValue, (newValue) => {
  originalValue.value = newValue;
});

// Auto-focus input when entering edit mode
watch(isEditing, async (newValue) => {
  if (newValue) {
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

const handleBlur = () => {
  if (isEditing.value) {
    isEditing.value = false;
    emit('update:modelValue', props.modelValue);
  }
};

const handleKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Enter') {
    event.preventDefault();
    (event.target as HTMLInputElement).blur();
  } else if (event.key === 'Escape') {
    emit('update:modelValue', originalValue.value);
    isEditing.value = false;
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
  <div class="flex items-center gap-3 group flex-1">
    <span
      v-if="prefix"
      class="text-tertiary font-medium flex items-center select-none"
      :class="[textSizeClasses[textSize], { 'opacity-50': isEditing }]"
    >
      {{ prefix }}
    </span>

    <div class="flex-1 relative">
      <!-- Display mode - shows wrapped text -->
      <div
        v-if="!isEditing"
        @click="handleClick"
        class="w-full font-semibold px-1 py-0.5 rounded-lg hover:bg-surface-hover transition-all duration-150 border-2 border-transparent break-words"
        :class="[
          textSizeClasses[textSize],
          {
            'cursor-pointer': canEdit,
            'cursor-default': !canEdit,
            'text-primary': modelValue,
            'text-tertiary italic': !modelValue
          }
        ]"
      >
        {{ modelValue || placeholder }}
      </div>

      <!-- Edit mode - input field -->
      <input
        v-else
        :value="modelValue"
        @input="$emit('update:modelValue', ($event.target as HTMLInputElement).value)"
        type="text"
        class="w-full bg-surface-hover text-primary font-semibold px-1 py-0.5 rounded-lg focus:bg-surface focus:outline-none transition-all duration-150 border-2 border-transparent focus:border-blue-500/50"
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