<!-- DocumentIconSelector.vue -->
<script setup lang="ts">
import { ref, watch, computed } from 'vue';
import DocumentIconModal from './DocumentIconModal.vue';

interface Props {
  initialIcon?: string;
  size?: 'sm' | 'md' | 'lg';
}

const props = withDefaults(defineProps<Props>(), {
  initialIcon: '📄',
  size: 'md',
});

const emit = defineEmits(['update:icon']);

const currentIcon = ref(props.initialIcon);
const showModal = ref(false);

// Common document-related emojis
const commonIcons = [
  '📄', '📝', '📑', '📃', '📜', '📋', '📁', '📂', 
  '📓', '📔', '📕', '📗', '📘', '📙', '📚', '📖',
  '🗒️', '🗓️', '📊', '📈', '📉', '🔍', '🔎', '🔖',
  '💡', '⚙️', '🔧', '🛠️', '🧰', '📌', '📎', '🔗'
];

// Watch for changes to the selected icon
watch(currentIcon, (newIcon) => {
  emit('update:icon', newIcon);
});

const toggleModal = () => {
  showModal.value = !showModal.value;
};

const updateIcon = (newIcon: string) => {
  currentIcon.value = newIcon;
};

const sizeClasses = {
  sm: 'text-lg',
  md: 'text-xl',
  lg: 'text-2xl'
};

// Check if the current icon is an SVG
const isCurrentIconSvg = computed(() => {
  return currentIcon.value.startsWith('<svg');
});
</script>

<template>
  <div class="relative">
    <!-- Current icon display -->
    <button
      @click="toggleModal"
      class="flex items-center justify-center hover:bg-surface-hover rounded-lg p-1.5 transition-colors focus:outline-none"
      :class="[sizeClasses[size]]"
      aria-label="Select document icon"
    >
      <span v-if="!isCurrentIconSvg" class="select-none">{{ currentIcon }}</span>
      <span v-else v-safe-html.svg="currentIcon" :class="[sizeClasses[size]]" class="text-primary"></span>
    </button>

    <!-- Icon modal -->
    <DocumentIconModal 
      :show="showModal" 
      :initial-icon="currentIcon"
      @update:icon="updateIcon"
      @close="showModal = false"
    />
  </div>
</template>

<style scoped>
/* Add any additional styles here */
</style> 