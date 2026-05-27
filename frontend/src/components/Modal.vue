<!-- Modal.vue -->
<script setup lang="ts">
import { computed, toRef, ref, watch, nextTick, onMounted, onUnmounted } from 'vue'
import { useFluent } from 'fluent-vue'
import { useScrollLock } from '@/composables/useScrollLock'

const props = defineProps<{
  show: boolean
  title: string
  contentClass?: string
  headerClass?: string
  removePadding?: boolean
  size?: 'sm' | 'md' | 'lg' | 'xl'
  // Optional override for the close-button screen-reader label.
  // Defaults to the shared `common-modal-close` key so every modal
  // narrates the same close affordance in the active locale.
  closeAriaLabel?: string
}>()

const fluent = useFluent()
const closeLabel = computed(() => props.closeAriaLabel ?? fluent.$t('common-modal-close'))

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
useScrollLock(toRef(props, 'show'))

// Handle escape key globally. Lives on document (not on the modal
// root) so Esc closes the modal no matter which descendant has
// focus, including inputs that would normally swallow the event.
const onEscape = (e: KeyboardEvent) => {
  if (e.key === 'Escape' && props.show) emit('close')
}

onMounted(() => {
  document.addEventListener('keydown', onEscape)
  // Some callers v-if-mount the modal already-open rather than toggling
  // `show`. The watcher below only fires on change, so move focus in
  // here too when we mount in the open state.
  if (props.show) moveFocusIntoDialog()
})
onUnmounted(() => {
  document.removeEventListener('keydown', onEscape)
  // Restore focus if we're unmounted while still open (v-if teardown).
  restoreFocus()
})

// --- Focus trap + restore ---
//
// On open: snapshot whatever had focus, then move focus into the
// modal so screen readers and keyboard users land inside the
// dialog. On close: restore focus to the snapshot so the user
// continues from where they left off.
//
// Tab/Shift+Tab cycles within the modal's focusable children
// rather than escaping to the page behind it.
const dialogRef = ref<HTMLElement | null>(null)
let previouslyFocused: HTMLElement | null = null

const FOCUSABLE_SELECTOR = [
  'a[href]',
  'button:not([disabled])',
  'textarea:not([disabled])',
  'input:not([disabled]):not([type="hidden"])',
  'select:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(',')

function focusableInDialog(): HTMLElement[] {
  const root = dialogRef.value
  if (!root) return []
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
    (el) => !el.hasAttribute('disabled') && el.offsetParent !== null,
  )
}

function onTrapKeydown(e: KeyboardEvent): void {
  if (e.key !== 'Tab') return
  const elements = focusableInDialog()
  if (elements.length === 0) {
    // No focusable children: keep focus on the dialog container.
    e.preventDefault()
    dialogRef.value?.focus()
    return
  }
  const first = elements[0]
  const last = elements[elements.length - 1]
  const active = document.activeElement as HTMLElement | null
  if (e.shiftKey) {
    if (active === first || !dialogRef.value?.contains(active)) {
      e.preventDefault()
      last.focus()
    }
  } else {
    if (active === last) {
      e.preventDefault()
      first.focus()
    }
  }
}

async function moveFocusIntoDialog(): Promise<void> {
  previouslyFocused = (document.activeElement as HTMLElement | null) ?? null
  await nextTick()
  const elements = focusableInDialog()
  ;(elements[0] ?? dialogRef.value)?.focus()
}

function restoreFocus(): void {
  if (!previouslyFocused) return
  // Defer to let any teleported-element cleanup finish first.
  const target = previouslyFocused
  previouslyFocused = null
  nextTick(() => target.focus())
}

watch(
  () => props.show,
  (open) => {
    if (open) moveFocusIntoDialog()
    else restoreFocus()
  },
)
</script>

<template>
  <Teleport to="body">
    <Transition name="modal" appear>
      <div
        v-if="show"
        class="fixed inset-0 z-overlay flex items-end sm:items-center justify-center"
      >
        <!-- Backdrop -->
        <div
          class="absolute inset-0 bg-black/50"
          @click="emit('close')"
        />

        <!-- Modal -->
        <div
          ref="dialogRef"
          role="dialog"
          aria-modal="true"
          :aria-labelledby="titleId"
          tabindex="-1"
          :class="[
            'modal-content relative w-full bg-surface shadow-xl flex flex-col pointer-events-auto',
            'max-h-[90vh] sm:max-h-[85vh]',
            'rounded-t-2xl sm:rounded-xl',
            'mx-0 sm:mx-4',
            sizeClasses,
            contentClass
          ]"
          @keydown="onTrapKeydown"
        >
          <!-- Header -->
          <div :class="['flex items-center justify-between p-4 bg-surface-alt border-b border-default flex-shrink-0 rounded-t-2xl sm:rounded-t-xl', headerClass]">
            <h3 :id="titleId" class="text-lg font-semibold text-primary truncate pr-4">{{ title }}</h3>
            <button
              type="button"
              @click="emit('close')"
              class="p-1 -mr-1 text-tertiary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors flex-shrink-0 touch-target inline-flex items-center justify-center"
              :aria-label="closeLabel"
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
