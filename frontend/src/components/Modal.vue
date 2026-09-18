<!-- Modal.vue -->
<script setup lang="ts">
/**
 * App-standard dialog / bottom-sheet surface, on Reka UI's Dialog.
 *
 * Reka owns the behaviour: focus trapped against keyboard, pointer and
 * programmatic focus, the rest of the page hidden from assistive tech
 * (`aria-hidden`), body scroll lock, Escape and backdrop dismiss, focus
 * restored to the opener on close, nested dialogs stacked. This file owns
 * the chrome: CSS grid regions [title · close] [body] [footer], a bottom
 * sheet below `sm`, a centred card above it.
 *
 * `alert` switches to Reka's AlertDialog: no backdrop dismiss, and focus
 * lands on the Cancel action (wrap it in `AlertDialogCancel`), for
 * confirmations where a stray click must not decide anything.
 *
 * Footer actions: wrap buttons in `.modal-actions` for touch-sized
 * mobile targets and stacked full-width layout.
 *
 * `autofocus` on an element inside the body still wins the initial focus;
 * otherwise Reka focuses the first tabbable element.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import {
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogOverlay,
  AlertDialogPortal,
  AlertDialogRoot,
  AlertDialogTitle,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
} from 'reka-ui'
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
  /** AlertDialog semantics: backdrop click does not dismiss, focus opens
   *  on the Cancel action. */
  alert?: boolean
}>(), {
  scrollContent: true,
  alert: false,
})

const emit = defineEmits<{ close: [] }>()

const fluent = useFluent()
const closeLabel = computed(() => props.closeAriaLabel ?? fluent.$t('common-modal-close'))

const sizeClass = computed(() => {
  switch (props.size) {
    case 'sm': return 'modal-panel--sm'
    case 'lg': return 'modal-panel--lg'
    case 'xl': return 'modal-panel--xl'
    default: return 'modal-panel--md'
  }
})

// The Dialog and AlertDialog parts share one prop and emit surface, so the
// chrome is written once and the primitive is picked per prop.
const parts = computed(() =>
  props.alert
    ? {
        Root: AlertDialogRoot,
        Portal: AlertDialogPortal,
        Overlay: AlertDialogOverlay,
        Content: AlertDialogContent,
        Title: AlertDialogTitle,
        Description: AlertDialogDescription,
      }
    : {
        Root: DialogRoot,
        Portal: DialogPortal,
        Overlay: DialogOverlay,
        Content: DialogContent,
        Title: DialogTitle,
        Description: DialogDescription,
      },
)

function onOpenChange(open: boolean) {
  if (!open) emit('close')
}

const contentRef = ref<{ $el?: HTMLElement } | null>(null)

// Honour an explicit `autofocus` inside the body over Reka's
// first-tabbable default, so forms open on the field they mean to.
function onOpenAutoFocus(event: Event) {
  const preferred = contentRef.value?.$el?.querySelector<HTMLElement>('[autofocus]')
  if (preferred) {
    event.preventDefault()
    preferred.focus()
  }
}
</script>

<template>
  <component :is="parts.Root" :open="show" @update:open="onOpenChange">
    <component :is="parts.Portal">
      <component
        :is="parts.Overlay"
        class="modal-backdrop bg-black/40 dark:bg-black/60 backdrop-blur-sm"
      />
      <!-- Without a Description part, Reka would still point
           aria-describedby at an id that renders nothing; unset it. -->
      <component
        :is="parts.Content"
        ref="contentRef"
        class="modal-panel"
        :class="[sizeClass, contentClass]"
        aria-modal="true"
        v-bind="description ? {} : { 'aria-describedby': undefined }"
        @open-auto-focus="onOpenAutoFocus"
      >
        <header
          class="modal-header"
          :class="[description && 'modal-header--described', headerClass]"
        >
          <div class="modal-header__main">
            <component :is="parts.Title" as="h2" class="modal-header__title">{{ title }}</component>
            <component
              :is="parts.Description"
              v-if="description"
              as="p"
              class="modal-header__description"
            >
              {{ description }}
            </component>
          </div>

          <DialogClose class="modal-header__close" :aria-label="closeLabel">
            <Icon name="close" size="sm" />
          </DialogClose>
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
      </component>
    </component>
  </component>
</template>

<style scoped>
/* --- Backdrop -------------------------------------------------- */

/* Colour and blur are Tailwind utilities on the element (`dark:` compiles
   to a correctly scoped selector; a scoped `:global(.dark)` rule once
   leaked onto every .dark element). */
.modal-backdrop {
  position: fixed;
  inset: 0;
  z-index: var(--z-overlay);
}

/* --- Panel shell ----------------------------------------------- */

/* Fixed and self-positioned (no centring wrapper): below `sm` it is a
   bottom sheet pinned to the viewport's bottom edge; from `sm` up
   `inset: 0; margin: auto` centres the content-sized panel without a
   transform, which leaves transform free for the motion below. */
.modal-panel {
  --modal-pad-x: 1rem;
  --modal-pad-body: 1rem;
  --modal-footer-pad-y: 0.75rem;
  --modal-header-close: 2.75rem;

  position: fixed;
  inset: auto 0 0 0;
  z-index: var(--z-overlay);
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
  outline: none;
}

@media (min-width: 640px) {
  .modal-panel {
    --modal-pad-x: 0.75rem;
    --modal-pad-body: 1.25rem;
    --modal-footer-pad-y: 0.5rem;
    --modal-header-close: 1.75rem;

    inset: 0;
    margin: auto;
    height: fit-content;
    width: calc(100% - 2rem);
    max-height: min(85vh, 920px);
    border-radius: 1rem;
    box-shadow:
      0 25px 50px -12px rgb(0 0 0 / 0.25),
      0 0 0 1px var(--color-border-default);
    padding-bottom: 0;
  }
}

/* Size caps apply on desktop only; mobile sheets are full-bleed. */
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

/* Driven by Reka's `data-state`; Presence keeps the element mounted
   until the closed animation ends. The sheet slides up, the card scales
   in; the backdrop fades. */
.modal-backdrop[data-state='open'] { animation: modal-fade-in 0.15s ease; }
.modal-backdrop[data-state='closed'] { animation: modal-fade-out 0.15s ease; }
.modal-panel[data-state='open'] { animation: modal-sheet-in 0.2s cubic-bezier(0.16, 1, 0.3, 1); }
.modal-panel[data-state='closed'] { animation: modal-sheet-out 0.15s ease; }

@media (min-width: 640px) {
  .modal-panel[data-state='open'] { animation: modal-card-in 0.2s cubic-bezier(0.16, 1, 0.3, 1); }
  .modal-panel[data-state='closed'] { animation: modal-card-out 0.15s ease; }
}

@keyframes modal-fade-in { from { opacity: 0; } to { opacity: 1; } }
@keyframes modal-fade-out { from { opacity: 1; } to { opacity: 0; } }
@keyframes modal-sheet-in { from { opacity: 0; transform: translateY(100%); } to { opacity: 1; transform: none; } }
@keyframes modal-sheet-out { from { opacity: 1; transform: none; } to { opacity: 0; transform: translateY(100%); } }
@keyframes modal-card-in { from { opacity: 0; transform: scale(0.97) translateY(-6px); } to { opacity: 1; transform: none; } }
@keyframes modal-card-out { from { opacity: 1; transform: none; } to { opacity: 0; transform: scale(0.97) translateY(-6px); } }

@media (prefers-reduced-motion: reduce) {
  .modal-backdrop[data-state],
  .modal-panel[data-state] {
    animation-duration: 1ms;
  }
}
</style>
