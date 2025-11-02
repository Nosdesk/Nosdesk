<!-- Modal.vue -->
<script setup lang="ts">
import { onMounted, onUnmounted, ref, computed } from 'vue'

const props = defineProps<{
  show: boolean;
  title: string;
  contentClass?: string;
  headerClass?: string;
  removePadding?: boolean;
  size?: 'sm' | 'md' | 'lg' | 'xl';
}>()

// Compute size classes
const sizeClasses = computed(() => {
  switch (props.size) {
    case 'sm': return 'max-w-md';
    case 'lg': return 'max-w-6xl';
    case 'xl': return 'max-w-7xl';
    default: return 'max-w-4xl';
  }
})

const emit = defineEmits<{
  (e: 'close'): void;
}>()

// Refs for DOM elements
const backdropRef = ref<HTMLElement | null>(null)
const modalContentRef = ref<HTMLElement | null>(null)

const handleEscape = (e: KeyboardEvent) => {
  if (e.key === 'Escape') {
    emit('close')
  }
}

// Standard Vue approach for closing on backdrop click
const handleClick = (e: MouseEvent) => {
  // Only close if clicked on backdrop directly (not inside the modal content)
  // We use the element references to check this directly
  if (backdropRef.value && e.target instanceof Node && backdropRef.value.contains(e.target) && 
      modalContentRef.value && !modalContentRef.value.contains(e.target)) {
    emit('close')
  }
}

onMounted(() => {
  document.addEventListener('keydown', handleEscape)
  // Add click listener to document to ensure proper event capture
  document.addEventListener('click', handleClick)
})

onUnmounted(() => {
  document.removeEventListener('keydown', handleEscape)
  document.removeEventListener('click', handleClick)
})
</script>

<template>
  <Teleport to="body">
    <div v-if="show" class="fixed inset-0 z-[9999] overflow-y-auto">
      <!-- Backdrop - no click handler here -->
      <div ref="backdropRef" class="fixed inset-0 bg-black/40 backdrop-blur-sm z-[9999]"></div>

      <!-- Modal -->
      <div class="relative flex min-h-full items-center justify-center p-4 z-[10000]">
        <div
          ref="modalContentRef"
          :class="['relative bg-surface rounded-xl shadow-xl w-full', sizeClasses, contentClass]"
        >
          <!-- Header -->
          <div :class="['flex items-center justify-between p-4 border-b border-default', headerClass]">
            <h3 class="text-lg font-semibold text-primary">{{ title }}</h3>
            <button
              @click="emit('close')"
              class="text-tertiary hover:text-primary transition-colors"
            >
              <svg class="w-5 h-5" viewBox="0 0 20 20" fill="currentColor">
                <path fill-rule="evenodd" d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z" clip-rule="evenodd" />
              </svg>
            </button>
          </div>

          <!-- Content -->
          <div :class="[removePadding ? '' : 'p-6']">
            <slot></slot>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template> 