<!-- Modal.vue -->
<script setup lang="ts">
import { computed, watch, onMounted, onUnmounted } from 'vue'

const props = defineProps<{
  show: boolean
  title: string
  contentClass?: string
  headerClass?: string
  removePadding?: boolean
  size?: 'sm' | 'md' | 'lg' | 'xl'
}>()

// Generate unique ID for aria-labelledby
const titleId = computed(() => `modal-title-${Math.random().toString(36).slice(2, 9)}`)

const emit = defineEmits<{
  close: []
}>()

const sizeClasses = computed(() => {
  switch (props.size) {
    case 'sm': return 'max-w-none sm:max-w-md'
    case 'lg': return 'max-w-none sm:max-w-xl md:max-w-3xl lg:max-w-5xl'
    case 'xl': return 'max-w-none sm:max-w-2xl md:max-w-4xl lg:max-w-6xl'
    default: return 'max-w-none sm:max-w-lg md:max-w-2xl lg:max-w-4xl'
  }
})

// Lock body scroll when modal is open
watch(() => props.show, (isOpen) => {
  document.body.style.overflow = isOpen ? 'hidden' : ''
}, { immediate: true })

// Handle escape key globally
const onEscape = (e: KeyboardEvent) => {
  if (e.key === 'Escape' && props.show) emit('close')
}

onMounted(() => document.addEventListener('keydown', onEscape))
onUnmounted(() => {
  document.removeEventListener('keydown', onEscape)
  document.body.style.overflow = ''
})
</script>

<template>
  <Teleport to="body">
    <Transition name="modal" appear>
      <div
        v-if="show"
        class="fixed inset-0 z-[9999] flex items-end sm:items-center justify-center"
      >
        <!-- Backdrop -->
        <div
          class="absolute inset-0 bg-black/50"
          @click="emit('close')"
        />

        <!-- Modal -->
        <div
          role="dialog"
          aria-modal="true"
          :aria-labelledby="titleId"
          :class="[
            'modal-content relative w-full bg-surface shadow-xl flex flex-col pointer-events-auto',
            'max-h-[90vh] sm:max-h-[85vh]',
            'rounded-t-2xl sm:rounded-xl',
            'mx-0 sm:mx-4',
            sizeClasses,
            contentClass
          ]"
        >
          <!-- Header -->
          <div :class="['flex items-center justify-between p-4 bg-surface-alt border-b border-default flex-shrink-0 rounded-t-2xl sm:rounded-t-xl', headerClass]">
            <h3 :id="titleId" class="text-lg font-semibold text-primary truncate pr-4">{{ title }}</h3>
            <button
              type="button"
              @click="emit('close')"
              class="p-1 -mr-1 text-tertiary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors flex-shrink-0"
              aria-label="Close modal"
            >
              <svg class="w-5 h-5" viewBox="0 0 20 20" fill="currentColor">
                <path fill-rule="evenodd" d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z" clip-rule="evenodd" />
              </svg>
            </button>
          </div>

          <!-- Content -->
          <div :class="['flex-1 overflow-y-auto min-h-0', removePadding ? '' : 'p-4 sm:p-6']">
            <slot />
          </div>

          <!-- Footer -->
          <div v-if="$slots.footer" class="flex-shrink-0 p-4 border-t border-default bg-surface">
            <slot name="footer" />
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.modal-enter-active,
.modal-leave-active {
  transition: opacity 0.2s ease;
}

.modal-enter-active .modal-content,
.modal-leave-active .modal-content {
  transition: transform 0.2s ease, opacity 0.2s ease;
}

.modal-enter-from,
.modal-leave-to {
  opacity: 0;
}

.modal-enter-from .modal-content {
  opacity: 0;
  transform: translateY(1rem);
}

.modal-leave-to .modal-content {
  opacity: 0;
  transform: translateY(1rem);
}

@media (min-width: 640px) {
  .modal-enter-from .modal-content,
  .modal-leave-to .modal-content {
    transform: scale(0.95);
  }
}
</style>
