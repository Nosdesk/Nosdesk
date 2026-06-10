<!-- Modal.vue -->
<script setup lang="ts">
/**
 * App-standard dialog / bottom-sheet surface.
 *
 * Layout is driven by CSS grid regions (see scoped styles) — not
 * breakpoint class stacks. Mobile renders as a bottom sheet with
 * [title · close] [body] [footer]. Desktop renders as a centred
 * card with compact h-9 header (SectionCard vocabulary).
 *
 * Footer actions: wrap buttons in `.modal-actions` for touch-sized
 * mobile targets and stacked full-width layout.
 */
import { computed, toRef } from 'vue'
import { useFluent } from 'fluent-vue'
import { useScrollLock } from '@/composables/useScrollLock'
import { useModalDialog } from '@/composables/useModalDialog'
import Icon from '@/components/common/Icon.vue'

const props = withDefaults(defineProps<{
  show: boolean
  title: string
  description?: string
  contentClass?: string
  headerClass?: string
  footerClass?: string
  removePadding?: boolean
  scrollContent?: boolean
  size?: 'sm' | 'md' | 'lg' | 'xl'
  closeAriaLabel?: string
}>(), {
  scrollContent: true,
})

const emit = defineEmits<{ close: [] }>()

const fluent = useFluent()
const closeLabel = computed(() => props.closeAriaLabel ?? fluent.$t('common-modal-close'))

const titleId = computed(() => `modal-title-${Math.random().toString(36).slice(2, 9)}`)
const descriptionId = computed(() =>
  props.description ? `modal-desc-${Math.random().toString(36).slice(2, 9)}` : undefined,
)

const sizeClass = computed(() => {
  switch (props.size) {
    case 'sm': return 'modal-panel--sm'
    case 'lg': return 'modal-panel--lg'
    case 'xl': return 'modal-panel--xl'
    default: return 'modal-panel--md'
  }
})

const showRef = toRef(props, 'show')
useScrollLock(showRef)
const { dialogRef, onTrapKeydown } = useModalDialog(showRef, () => emit('close'))
</script>

<template>
  <Teleport to="body">
    <Transition name="modal" appear>
      <div v-if="show" class="modal-root">
        <div
          class="absolute inset-0 bg-black/40 dark:bg-black/60 backdrop-blur-sm"
          aria-hidden="true"
          @click="emit('close')"
        />

        <div
          ref="dialogRef"
          role="dialog"
          aria-modal="true"
          :aria-labelledby="titleId"
          :aria-describedby="descriptionId"
          tabindex="-1"
          class="modal-panel"
          :class="[sizeClass, contentClass]"
          @keydown="onTrapKeydown"
        >
          <header
            class="modal-header"
            :class="[description && 'modal-header--described', headerClass]"
          >
            <div class="modal-header__main">
              <h2 :id="titleId" class="modal-header__title">{{ title }}</h2>
              <p
                v-if="description"
                :id="descriptionId"
                class="modal-header__description"
              >
                {{ description }}
              </p>
            </div>

            <button
              type="button"
              class="modal-header__close"
              :aria-label="closeLabel"
              @click="emit('close')"
            >
              <Icon name="close" size="sm" />
            </button>
          </header>

          <div
            class="modal-body"
            :class="[
              scrollContent === false
                ? 'modal-body--fixed'
                : 'modal-body--scroll',
              removePadding && 'modal-body--flush',
            ]"
          >
            <slot />
          </div>

          <footer
            v-if="$slots.footer"
            class="modal-footer"
            :class="footerClass"
          >
            <slot name="footer" />
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
/* --- Overlay --------------------------------------------------- */

.modal-root {
  position: fixed;
  inset: 0;
  z-index: var(--z-overlay);
  display: flex;
  align-items: flex-end;
  justify-content: stretch;
}

/* Backdrop styling lives on the element as Tailwind utilities
   (absolute inset-0 bg-black/40 dark:bg-black/60 backdrop-blur-sm),
   matching GlobalSearchModal. It used to be scoped CSS with
   `:global(.dark) .modal-backdrop`, but that selector mis-compiled to a
   bare global `.dark { background }` that leaked onto every .dark
   element (notably <html class="dark">). Tailwind's dark: variant
   compiles to a correctly-scoped `.dark .selector`, so the bug class
   can't recur. */

@media (min-width: 640px) {
  .modal-root {
    align-items: center;
    justify-content: center;
    padding: 1rem;
  }
}

/* --- Panel shell ----------------------------------------------- */

.modal-panel {
  --modal-pad-x: 1rem;
  --modal-pad-body: 1rem;
  --modal-footer-pad-y: 0.75rem;
  --modal-header-close: 2.75rem;

  position: relative;
  z-index: 1;
  display: grid;
  grid-template-rows: auto minmax(0, 1fr) auto;
  grid-template-areas:
    'header'
    'body'
    'footer';
  width: 100%;
  max-width: none;
  max-height: min(92dvh, 920px);
  overflow: hidden;
  background: var(--color-surface);
  border-radius: 1rem 1rem 0 0;
  box-shadow:
    0 -8px 32px -8px rgb(0 0 0 / 0.2),
    0 0 0 1px var(--color-border-default);
  border-bottom: 0;
  padding-bottom: env(safe-area-inset-bottom);
}

@media (min-width: 640px) {
  .modal-panel {
    --modal-pad-x: 0.75rem;
    --modal-pad-body: 1.25rem;
    --modal-footer-pad-y: 0.5rem;
    --modal-header-close: 1.75rem;

    max-height: min(85vh, 920px);
    border-radius: 1rem;
    box-shadow:
      0 25px 50px -12px rgb(0 0 0 / 0.25),
      0 0 0 1px var(--color-border-default);
    padding-bottom: 0;
  }
}

/* Size caps apply on desktop only — mobile sheets are full-bleed. */
@media (min-width: 640px) {
  .modal-panel--sm { max-width: 28rem; }
  .modal-panel--md { max-width: 32rem; }
  .modal-panel--lg { max-width: 36rem; }
  .modal-panel--xl { max-width: 42rem; }
}

@media (min-width: 768px) {
  .modal-panel--md { max-width: 42rem; }
  .modal-panel--lg { max-width: 48rem; }
  .modal-panel--xl { max-width: 56rem; }
}

@media (min-width: 1024px) {
  .modal-panel--md { max-width: 56rem; }
  .modal-panel--lg { max-width: 64rem; }
  .modal-panel--xl { max-width: 72rem; }
}

/* --- Header grid ----------------------------------------------- */

.modal-header {
  grid-area: header;
  display: grid;
  grid-template-columns: minmax(0, 1fr) var(--modal-header-close);
  grid-template-areas: 'main close';
  column-gap: 0.75rem;
  align-items: center;
  min-height: var(--modal-header-close);
  border-bottom: 1px solid var(--color-border-default);
  background: var(--color-surface-alt);
  padding-inline: var(--modal-pad-x);
}

.modal-header__main {
  grid-area: main;
  min-width: 0;
  padding-block: 0.75rem;
}

.modal-header__title {
  margin: 0;
  font-size: 0.875rem;
  font-weight: 600;
  line-height: 1.25;
  letter-spacing: -0.01em;
  color: var(--color-text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.modal-header__description {
  margin: 0.25rem 0 0;
  font-size: 0.75rem;
  line-height: 1.35;
  color: var(--color-text-secondary);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.modal-header__close {
  grid-area: close;
  align-self: center;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: var(--modal-header-close);
  height: var(--modal-header-close);
  margin-inline-end: -0.25rem;
  border: 0;
  border-radius: 0.5rem;
  background: transparent;
  color: var(--color-text-tertiary);
  cursor: pointer;
  transition: color 0.15s ease, background-color 0.15s ease;
}

.modal-header__close:hover {
  color: var(--color-text-primary);
  background: var(--color-surface-hover);
}

.modal-header--described .modal-header__main {
  padding-block: 0.375rem 0.625rem;
}

@media (min-width: 640px) {
  .modal-header {
    min-height: 2.25rem;
  }

  .modal-header__main {
    align-self: center;
    padding-block: 0;
  }

  .modal-header__title {
    font-size: 0.8125rem;
    line-height: 1;
  }

  .modal-header__description {
    font-size: 0.6875rem;
  }

  .modal-header--described {
    min-height: auto;
    padding-block: 0.375rem;
  }

  .modal-header--described .modal-header__main {
    padding-block: 0;
  }
}

/* --- Body ------------------------------------------------------ */

.modal-body {
  grid-area: body;
  min-height: 0;
}

.modal-body--scroll {
  overflow-y: auto;
  overscroll-behavior: contain;
  padding: var(--modal-pad-body);
  scrollbar-width: thin;
  scrollbar-color: var(--color-border-default) transparent;
}

.modal-body--scroll::-webkit-scrollbar {
  width: 6px;
}

.modal-body--scroll::-webkit-scrollbar-track {
  background: transparent;
}

.modal-body--scroll::-webkit-scrollbar-thumb {
  background-color: var(--color-border-default);
  border-radius: 3px;
}

.modal-body--fixed {
  display: flex;
  flex-direction: column;
  overflow: hidden;
  padding: var(--modal-pad-body);
}

.modal-body--flush {
  padding: 0;
}

/* --- Footer ---------------------------------------------------- */

.modal-footer {
  grid-area: footer;
  padding: var(--modal-footer-pad-y) var(--modal-pad-x);
  border-top: 1px solid var(--color-border-default);
  background: color-mix(in srgb, var(--color-surface-alt) 50%, transparent);
}

/* Shared action-row layout for footer slots. */
.modal-footer :deep(.modal-actions) {
  display: flex;
  flex-direction: column-reverse;
  gap: 0.5rem;
}

.modal-footer :deep(.modal-actions > *) {
  width: 100%;
}

@media (min-width: 640px) {
  .modal-footer :deep(.modal-actions) {
    flex-direction: row;
    justify-content: flex-end;
  }

  .modal-footer :deep(.modal-actions > *) {
    width: auto;
  }
}

.modal-footer :deep(.modal-actions button) {
  min-height: 2.75rem;
  padding-inline: 1rem;
  font-size: 0.875rem;
  font-weight: 500;
  border-radius: 0.5rem;
}

@media (min-width: 640px) {
  .modal-footer :deep(.modal-actions button) {
    min-height: 0;
    padding: 0.5rem 0.875rem;
  }
}

/* --- Motion ---------------------------------------------------- */

.modal-enter-active,
.modal-leave-active {
  transition: opacity 0.15s ease;
}

.modal-enter-active .modal-panel,
.modal-leave-active .modal-panel {
  transition:
    transform 0.2s cubic-bezier(0.16, 1, 0.3, 1),
    opacity 0.15s ease;
}

.modal-enter-from,
.modal-leave-to {
  opacity: 0;
}

.modal-enter-from .modal-panel,
.modal-leave-to .modal-panel {
  opacity: 0;
  transform: translateY(100%);
}

@media (min-width: 640px) {
  .modal-enter-from .modal-panel,
  .modal-leave-to .modal-panel {
    transform: scale(0.97) translateY(-6px);
  }
}

@media (prefers-reduced-motion: reduce) {
  .modal-enter-active,
  .modal-leave-active,
  .modal-enter-active .modal-panel,
  .modal-leave-active .modal-panel {
    transition: none;
  }
}
</style>
