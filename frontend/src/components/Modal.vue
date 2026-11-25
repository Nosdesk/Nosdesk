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
 * sheet below `md` (the breakpoint every other sheet in the app uses), a
 * centred card above it.
 *
 * The sheet dismisses on a swipe down (`useSheetDismissGesture`): from
 * the header always, from the body when its scroller is at the top. The
 * grabber at the top is the affordance; it sits in the header's top
 * padding rather than on a row of its own.
 *
 * `alert` switches to Reka's AlertDialog: no backdrop dismiss, no swipe
 * dismiss, and focus lands on the Cancel action (wrap it in
 * `AlertDialogCancel`), for confirmations where a stray click must not
 * decide anything.
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
import { useSheetDismissGesture } from '@/composables/useSheetDismissGesture'

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
  /** AlertDialog semantics: neither backdrop click nor swipe dismisses,
   *  focus opens on the Cancel action. */
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

// The Overlay and Content parts render as-child onto these elements, so
// the gesture has the real nodes to move and dim (and Reka's `data-state`
// keyframes land on the same nodes).
const panelEl = ref<HTMLElement | null>(null)
const backdropEl = ref<HTMLElement | null>(null)

useSheetDismissGesture({
  panel: panelEl,
  backdrop: backdropEl,
  enabled: () => !props.alert,
  onDismiss: () => emit('close'),
})

// Honour an explicit `autofocus` inside the body over Reka's
// first-tabbable default, so forms open on the field they mean to.
function onOpenAutoFocus(event: Event) {
  const preferred = panelEl.value?.querySelector<HTMLElement>('[autofocus]')
  if (preferred) {
    event.preventDefault()
    preferred.focus()
  }
}
</script>

<template>
  <component :is="parts.Root" :open="show" @update:open="onOpenChange">
    <component :is="parts.Portal">
      <component :is="parts.Overlay" as-child>
        <div ref="backdropEl" class="modal-backdrop bg-black/40 dark:bg-black/60 backdrop-blur-sm" />
      </component>
      <!-- Without a Description part, Reka would still point
           aria-describedby at an id that renders nothing; unset it. -->
      <component
        :is="parts.Content"
        as-child
        v-bind="description ? {} : { 'aria-describedby': undefined }"
        @open-auto-focus="onOpenAutoFocus"
      >
        <div
          ref="panelEl"
          class="modal-panel"
          :class="[sizeClass, contentClass]"
          aria-modal="true"
        >
          <!-- Decorative: the close button is the accessible way out. -->
          <div class="modal-grabber" aria-hidden="true" />
          <!-- The header is the drag handle: a drag may start here however
               the body is scrolled. -->
          <header
            class="modal-header"
            :class="[description && 'modal-header--described', headerClass]"
            data-sheet-handle
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
        </div>
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

/* Fixed and self-positioned (no centring wrapper): below `md` it is a
   bottom sheet pinned to the viewport's bottom edge; from `md` up
   `inset: 0; margin: auto` centres the content-sized panel without a
   transform, which leaves transform free for the motion below.

   The sheet extends 2.5rem below the viewport (and pads the same amount,
   so nothing visible moves): the swipe gesture lets it stretch up to
   40px past rest, and without the extension the page would show under
   its bottom edge. */
.modal-panel {
  --modal-pad-x: 1rem;
  --modal-pad-body: 1rem;
  --modal-footer-pad-y: 0.75rem;
  --modal-header-close: 2.75rem;
  --modal-sheet-stretch: 2.5rem;

  position: fixed;
  inset: auto 0 calc(-1 * var(--modal-sheet-stretch)) 0;
  z-index: var(--z-overlay);
  display: grid;
  grid-template-rows: auto minmax(0, 1fr) auto;
  grid-template-areas:
    'header'
    'body'
    'footer';
  width: 100%;
  max-width: none;
  max-height: calc(min(92dvh, 920px) + var(--modal-sheet-stretch));
  overflow: hidden;
  background: var(--color-surface);
  border-radius: 1rem 1rem 0 0;
  box-shadow:
    0 -8px 32px -8px rgb(0 0 0 / 0.2),
    0 0 0 1px var(--color-border-default);
  border-bottom: 0;
  padding-bottom: calc(env(safe-area-inset-bottom) + var(--modal-sheet-stretch));
  outline: none;
}

@media (min-width: 768px) {
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
@media (min-width: 768px) {
  .modal-panel--sm { max-width: 28rem; }
  .modal-panel--md { max-width: 42rem; }
  .modal-panel--lg { max-width: 48rem; }
  .modal-panel--xl { max-width: 56rem; }
}

@media (min-width: 1024px) {
  .modal-panel--md { max-width: 56rem; }
  .modal-panel--lg { max-width: 64rem; }
  .modal-panel--xl { max-width: 72rem; }
}

/* --- Grabber ---------------------------------------------------- */

/* The sheet's swipe affordance, iOS-sized. Absolutely positioned over the
   header's top padding so it costs no row; the header grows by 4px to
   clear it (8px with a description, whose tighter padding would otherwise
   run into the pill). Hidden on the desktop card, which has no gesture. */
.modal-grabber {
  position: absolute;
  top: 0.375rem;
  left: 50%;
  width: 2.25rem;
  height: 0.25rem;
  transform: translateX(-50%);
  border-radius: 9999px;
  background: var(--color-border-strong);
  pointer-events: none;
  /* Above a header a consumer has made `relative`. */
  z-index: 1;
}

@media (min-width: 768px) {
  .modal-grabber {
    display: none;
  }
}

/* --- Header grid ----------------------------------------------- */

/* Below `md` the header is the sheet's drag handle: `touch-action: none`
   keeps WebKit from starting a pan of its own (there is nothing here to
   scroll), and the title is not selectable, so a long press cannot turn
   a drag into a text selection. */
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
  padding-top: 0.25rem;
  touch-action: none;
  -webkit-user-select: none;
  user-select: none;
}

.modal-header--described {
  padding-top: 0.5rem;
}

.modal-header__main {
  grid-area: main;
  min-width: 0;
  padding-block: 0.75rem;
}

.modal-header__title {
  margin: 0;
  font-size: var(--text-sm);
  font-weight: var(--font-weight-semibold);
  line-height: 1.25;
  letter-spacing: -0.01em;
  color: var(--color-text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.modal-header__description {
  margin: 0.25rem 0 0;
  font-size: var(--text-xs);
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

@media (min-width: 768px) {
  .modal-header {
    min-height: 2.25rem;
    padding-top: 0;
    touch-action: auto;
    -webkit-user-select: auto;
    user-select: auto;
  }

  .modal-header__main {
    align-self: center;
    padding-block: 0;
  }

  .modal-header__title {
    font-size: var(--text-xs-plus);
    line-height: 1;
  }

  .modal-header__description {
    font-size: var(--text-2xs);
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

/* Action rows written in place (a `justify-end` or `justify-between` row
   of buttons, in the body or the footer) wrap instead of running off the
   panel once larger text widens their buttons. */
.modal-panel :deep(.flex.justify-end),
.modal-panel :deep(.flex.justify-between:has(> button)) {
  flex-wrap: wrap;
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

@media (min-width: 768px) {
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
  font-size: var(--text-sm);
  font-weight: var(--font-weight-medium);
  border-radius: 0.5rem;
}

@media (min-width: 768px) {
  .modal-footer :deep(.modal-actions button) {
    min-height: 0;
    padding: 0.5rem 0.875rem;
  }
}

/* --- Motion ---------------------------------------------------- */

/* Driven by Reka's `data-state`; Presence keeps the element mounted
   until the closed animation ends. The sheet slides up, the card scales
   in; the backdrop fades. A swipe dismiss has already taken the sheet
   off-screen and sets an inline `animation: none` on both elements, so
   the closed keyframes do not replay it. */
.modal-backdrop[data-state='open'] { animation: modal-fade-in 0.15s ease; }
.modal-backdrop[data-state='closed'] { animation: modal-fade-out 0.15s ease; }
.modal-panel[data-state='open'] { animation: modal-sheet-in 0.2s cubic-bezier(0.16, 1, 0.3, 1); }
.modal-panel[data-state='closed'] { animation: modal-sheet-out 0.15s ease; }

@media (min-width: 768px) {
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
