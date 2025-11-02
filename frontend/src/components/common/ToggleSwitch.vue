<script setup lang="ts">
interface Props {
  modelValue: boolean;
  disabled?: boolean;
  size?: 'sm' | 'md' | 'lg';
  label?: string;
  description?: string;
}

interface Emits {
  (e: 'update:modelValue', value: boolean): void;
}

const props = withDefaults(defineProps<Props>(), {
  disabled: false,
  size: 'md'
});

const emit = defineEmits<Emits>();

const toggle = () => {
  if (!props.disabled) {
    emit('update:modelValue', !props.modelValue);
  }
};

// Size variants
const sizeClasses = {
  sm: {
    container: 'h-4 w-7',
    thumb: 'h-3 w-3',
    translate: 'translate-x-3'
  },
  md: {
    container: 'h-5 w-9',
    thumb: 'h-4 w-4',
    translate: 'translate-x-4'
  },
  lg: {
    container: 'h-6 w-11',
    thumb: 'h-5 w-5',
    translate: 'translate-x-5'
  }
};

const currentSize = sizeClasses[props.size];
</script>

<template>
  <div class="flex items-center justify-between" :class="label ? 'py-2' : ''">
    <!-- Label and description (if provided) -->
    <div v-if="label || description" class="flex-1 mr-4">
      <div v-if="label" class="text-sm font-medium text-primary">{{ label }}</div>
      <div v-if="description" class="text-xs text-tertiary">{{ description }}</div>
    </div>

    <!-- Toggle Switch -->
    <button
      @click="toggle"
      :disabled="disabled"
      :class="[
        'relative inline-flex items-center rounded-full transition-colors focus:outline-none focus:ring-2 focus:ring-blue-500/50 focus:ring-offset-2 focus:ring-offset-surface',
        currentSize.container,
        modelValue ? 'bg-blue-600' : 'bg-border-default',
        disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'
      ]"
    >
      <span
        :class="[
          'inline-block transform rounded-full bg-white transition-transform',
          currentSize.thumb,
          modelValue ? currentSize.translate : 'translate-x-0.5'
        ]"
      />
    </button>
  </div>
</template> 