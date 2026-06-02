<!--
Universal card chrome for every dashboard widget. This is the ONLY
place the visual vocabulary for a widget is defined — card border,
radius, header treatment, body padding, loading/empty/error states,
and edit-mode affordances. Individual widgets render their content
through the default slot and never draw their own chrome.

Why a shell:
  * Before this existed, each widget rolled its own `<div class=
    "bg-surface rounded-xl border…">`. Spacing, radius, header
    typography and empty-state styling drifted between widgets, and
    the stat widgets didn't use a card at all (they were bare grids
    of micro-tiles under a section heading). That's the "amateur
    dashboard" look — multiple competing visual languages at once.
  * With the shell, adding a new widget is just content. The chrome
    is inherited automatically and is, by definition, consistent with
    every other widget.

Edit-mode affordances (docs/dashboard-and-analytics-plan.md decision
20–22):
  * Drag handle is a 4px shaded gutter running the full left edge of
    the card. It reads as a card-affordance rather than a header-
    affordance, doesn't compete with header content for space, and
    has the same Fitts-friendly long-axis target Linear / Notion use.
  * Right-click (or context-menu key) opens a per-widget context menu
    with resize 1/2/3 and hide. The header no longer carries those
    controls — fewer floating sticker buttons over the card chrome.
  * Number keys 1, 2, 3 resize the focused widget. The card itself is
    tabbable in edit mode so keyboard users can drive sizing without
    a mouse.
The shell doesn't own the drag *logic* (that's `usePointerSortable`
in the dashboard parent); it only renders the affordances and emits
the events when the parent has flipped edit-mode on via `provide()`.
-->
<script setup lang="ts">
import { computed, inject, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import {
  DASHBOARD_WIDGET_CONTEXT,
  type DashboardWidgetContext,
} from './widgetContext'
import type { WidgetSpan } from './widgets'
import ContextMenu, { type MenuItem } from '@/components/common/ContextMenu.vue'
import { ICON_REGISTRY } from '@/components/common/icons'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

const props = withDefaults(
  defineProps<{
    /** Header title, always shown. */
    title: string
    /** Router destination for a right-aligned "View all →" link.
     *  Omit when the widget has no drill-down. */
    actionTo?: string
    /** Label for the action link; defaults to "View all". */
    actionLabel?: string
    /** True while the widget is fetching its *initial* data — the
     *  shell renders a skeleton in place of content. Callers should
     *  flip this back to `false` once the first response has landed
     *  (even an empty one) so subsequent refetches use `refreshing`
     *  instead, keeping rendered content visible. */
    loading?: boolean
    /** True during a background refetch after the initial load has
     *  completed. Content stays rendered; an indeterminate shimmer
     *  bar at the top of the card signals the stale-then-fresh swap
     *  without blanking the UI. */
    refreshing?: boolean
    /** Non-empty = render the error state instead of the body. */
    error?: string | null
    /** True = render the `empty` slot (or default empty text) instead
     *  of the body. Callers control this from their own data. */
    empty?: boolean
    /** Default-slot replacement copy when `empty === true` and the
     *  caller doesn't provide the `#empty` slot. */
    emptyTitle?: string
    emptyDescription?: string
    /**
     * When `true`, the body slot is rendered flush with the card edges
     * (no padding) — use for widgets that draw their own list rows or
     * grid cells and own their own internal spacing.
     */
    flushBody?: boolean
    /**
     * Baseline minimum height for the body region (the area that
     * transitions between skeleton / error / empty / content). Widgets
     * should pass a value matching their skeleton's rendered height so
     * that skeleton → data swaps don't shrink the card, which in a
     * row-stretch grid would cascade-shrink every sibling in the row.
     *
     * Data richer than the baseline still grows the card naturally;
     * data sparser than the baseline pads with honest whitespace
     * rather than pulling the whole row smaller. Leave undefined for
     * widgets that can't be taller than their single content block
     * (e.g. the horizontal stat rails).
     */
    minBodyHeight?: string
  }>(),
  {
    actionLabel: '',
    emptyTitle: '',
    emptyDescription: '',
    flushBody: true,
  },
)

const actionLabelText = computed(() => props.actionLabel || t('dashboard-widget-shell-action-view-all'))
const emptyTitleText = computed(() => props.emptyTitle || t('dashboard-widget-shell-empty-title-default'))

// Edit-mode context is optional — when a widget is rendered outside
// the dashboard (e.g. a ticket list on a profile page), the context
// is undefined and the shell renders with no edit affordances at all.
const ctx = inject<DashboardWidgetContext | undefined>(DASHBOARD_WIDGET_CONTEXT, undefined)

const editMode = computed(() => ctx?.editMode.value ?? false)
const dragging = computed(() => ctx?.dragging.value ?? false)
const currentSpan = computed<WidgetSpan>(() => ctx?.currentSpan.value ?? 1)

function onResize(span: WidgetSpan) {
  ctx?.onResize(span)
}
function onHide() {
  ctx?.onHide()
}
function onHandlePointerDown(e: PointerEvent) {
  ctx?.onHandlePointerDown(e)
}

// Context menu: anchored at the click point, opened by right-click
// or the keyboard context-menu key. Sizing radio + hide live here;
// removing them from the header keeps the chrome quiet while leaving
// the affordance one gesture away.
const menuOpen = ref(false)
const menuX = ref(0)
const menuY = ref(0)

const menuItems = computed<MenuItem[]>(() => [
  {
    id: 'resize-1',
    label: t('dashboard-widget-context-menu-resize-1'),
    icon: ICON_REGISTRY.check.d,
    checked: currentSpan.value === 1,
    trailing: '1',
  },
  {
    id: 'resize-2',
    label: t('dashboard-widget-context-menu-resize-2'),
    icon: ICON_REGISTRY.check.d,
    checked: currentSpan.value === 2,
    trailing: '2',
  },
  {
    id: 'resize-3',
    label: t('dashboard-widget-context-menu-resize-3'),
    icon: ICON_REGISTRY.check.d,
    checked: currentSpan.value === 3,
    trailing: '3',
  },
  {
    id: 'hide',
    label: t('dashboard-widget-context-menu-hide'),
    icon: ICON_REGISTRY.close.d,
    danger: true,
    divider: true,
  },
])

function onContextMenu(e: MouseEvent) {
  if (!editMode.value) return
  e.preventDefault()
  menuX.value = e.clientX
  menuY.value = e.clientY
  menuOpen.value = true
}

function onMenuSelect(id: string) {
  switch (id) {
    case 'resize-1':
      onResize(1)
      break
    case 'resize-2':
      onResize(2)
      break
    case 'resize-3':
      onResize(3)
      break
    case 'hide':
      onHide()
      break
  }
}

/** Keyboard sizing: 1, 2, 3 set the focused widget's span. Active
 *  only in edit mode and only when the event target is the widget
 *  root itself (not bubbled up from a focused control inside the
 *  body), so typing "1" into a filter input inside a widget doesn't
 *  silently resize the card.
 *
 *  `stopPropagation` AND `preventDefault` are both required: the
 *  dashboard's document-level keybindings (useDashboardKeybindings)
 *  bind 1..=7 to section-anchor jumps, so without stopping the
 *  bubble the card would resize AND the page would scroll away to
 *  a section anchor. */
function onCardKeydown(e: KeyboardEvent) {
  if (!editMode.value) return
  if (e.target !== e.currentTarget) return
  if (e.metaKey || e.ctrlKey || e.altKey) return
  const span = sizeKeyToSpan(e.key)
  if (span === null) return
  e.preventDefault()
  e.stopPropagation()
  onResize(span)
}

function sizeKeyToSpan(key: string): WidgetSpan | null {
  switch (key) {
    case '1':
      return 1
    case '2':
      return 2
    case '3':
      return 3
    default:
      return null
  }
}
</script>

<template>
  <!--
    Dragged widget renders as a clean accent-tinted landing slot
    rather than its normal chrome. Combined with the preview-reorder
    in the parent, it lands where the drop will commit — neighbours
    shift around it so the user sees the destination layout
    optimistically. No text, no dashed border; the tint alone is
    enough signal once it's the only accent-coloured surface on the
    dashboard.
  -->
  <div
    v-if="dragging"
    class="min-h-[9rem] h-full rounded-xl bg-accent/10 border border-accent/40"
    aria-hidden="true"
  />
  <article
    v-else
    :class="[
      'bg-surface rounded-xl border overflow-hidden flex h-full relative transition-colors',
      editMode
        ? 'border-default hover:border-accent/40 focus-visible:border-accent focus-visible:outline-none'
        : 'border-default',
    ]"
    :tabindex="editMode ? 0 : -1"
    @contextmenu="onContextMenu"
    @keydown="onCardKeydown"
  >
    <!-- Drag-handle gutter: 4px shaded column running the full left
         edge of the card in edit mode. Touch targets need depth so
         the actual hit area is wider than the visual stripe — the
         button is 12px wide, the visible bar is the inner 4px. The
         gutter doubles as the visual cue that the card is movable;
         nothing in the header competes for that affordance. -->
    <button
      v-if="editMode"
      type="button"
      class="group flex-shrink-0 w-3 h-full flex items-stretch justify-center cursor-grab active:cursor-grabbing touch-none focus-visible:outline-none"
      :aria-label="t('dashboard-widget-shell-drag-label', { title })"
      @pointerdown="onHandlePointerDown"
    >
      <span
        class="block w-1 h-full bg-default group-hover:bg-accent group-focus-visible:bg-accent transition-colors"
        aria-hidden="true"
      />
    </button>

    <div class="flex flex-col flex-1 min-w-0">
    <!--
      Indeterminate progress bar for background refetches. Positioned
      at the very top of the card, clipped by the shell's rounded
      corners, so it reads as "this card is refreshing" without
      blanking the content underneath.
    -->
    <div
      v-if="refreshing && !loading"
      aria-hidden="true"
      class="absolute top-0 inset-x-0 h-[2px] overflow-hidden bg-surface-alt z-10 pointer-events-none"
    >
      <div class="widget-shimmer-bar h-full w-1/3 bg-accent/80" />
    </div>
    <!--
      Header: fixed-height pill that houses the title, widget-
      specific header actions, and the "View all" link. Resize and
      hide controls have moved to the right-click context menu so
      the header reads the same in view mode and edit mode (minus
      the View-all link, which goes quiet in edit mode).
    -->
    <header class="flex items-center gap-2 px-3 h-9 border-b border-default bg-surface-alt flex-shrink-0">
      <h2 class="text-[13px] font-semibold text-primary truncate flex-1 tracking-tight">{{ title }}</h2>

      <!-- Widget-specific header controls (e.g. filter dropdowns). -->
      <slot name="headerActions" />

      <!-- "View all" link. Hidden in edit mode so the card chrome
           reads as "this is in flux" rather than "this is live." -->
      <router-link
        v-if="actionTo && !editMode"
        :to="actionTo"
        class="text-[11px] font-medium text-accent hover:underline whitespace-nowrap"
      >
        {{ actionLabelText }} →
      </router-link>
    </header>

    <!--
      Optional subheader rendered between the chrome header and the
      body. Unlike the default slot, it is always rendered regardless
      of loading / empty / error state so widget-level controls like
      filter tabs remain usable when the data slot has nothing to show.
    -->
    <slot name="subheader" />

    <!-- Body states: the shell owns loading / error / empty, so every
         widget gets the same visual treatment for free.

         `minBodyHeight` applies an inline min-height so skeleton and
         sparse-data states fill the same vertical space — preventing
         the row-cascade shift on initial load. -->
    <div
      :class="['flex-1 min-h-0 flex flex-col', flushBody ? '' : 'p-4']"
      :style="minBodyHeight ? { minHeight: minBodyHeight } : undefined"
    >
      <!--
        State machine for the body: skeleton → (error | empty | data).
        Wrapped in a keyed fade transition so swaps cross-dissolve
        instead of cutting. Height still changes when states differ
        in size (unavoidable with auto-rows-min), but the opacity
        fade softens the transition so it reads as "refining" rather
        than "jumping."

        Initial-load skeleton: widgets pass a `#skeleton` slot that
        mirrors their real content layout. The fallback is a 5-row
        divided-list skeleton matching the shape every simple list
        widget on the dashboard caps at (they all `.slice(0, 5)`), so
        it lands at the same height as their typical populated state.
      -->
      <Transition name="widget-state" mode="out-in">
        <div
          v-if="loading"
          key="skeleton"
          class="flex-1 flex flex-col"
          aria-busy="true"
          :aria-label="t('dashboard-widget-shell-loading-label', { title })"
        >
          <slot name="skeleton">
            <ul class="divide-y divide-default">
              <li
                v-for="i in 5"
                :key="i"
                class="flex items-center gap-3 px-4 h-10"
              >
                <div class="h-2.5 w-8 rounded bg-surface-alt animate-pulse" />
                <div
                  class="h-2.5 rounded bg-surface-alt animate-pulse flex-1"
                  :style="{ maxWidth: `${70 - (i % 5) * 8}%` }"
                />
                <div class="h-2.5 w-8 rounded bg-surface-alt animate-pulse" />
              </li>
            </ul>
          </slot>
        </div>
        <div
          v-else-if="error"
          key="error"
          class="flex-1 flex items-center justify-center py-6 text-xs text-status-error text-center px-4"
        >
          {{ error }}
        </div>
        <div
          v-else-if="empty"
          key="empty"
          class="flex-1 flex flex-col items-center justify-center py-6 text-center px-4"
        >
          <slot name="empty">
            <p class="text-sm text-secondary">{{ emptyTitleText }}</p>
            <p v-if="emptyDescription" class="text-xs text-tertiary mt-1">{{ emptyDescription }}</p>
          </slot>
        </div>
        <div v-else key="content" class="flex-1 flex flex-col">
          <slot />
        </div>
      </Transition>
    </div>
    </div>

    <ContextMenu
      :items="menuItems"
      :x="menuX"
      :y="menuY"
      :open="menuOpen"
      @select="onMenuSelect"
      @close="menuOpen = false"
    />
  </article>
</template>

<style scoped>
/* Indeterminate shimmer for background refetches. The bar moves a
 * full cycle beyond both sides of the clip region so it reads as a
 * continuous loop, not a reset. */
.widget-shimmer-bar {
  animation: widget-shimmer 1.25s linear infinite;
  will-change: transform;
}
@keyframes widget-shimmer {
  0%   { transform: translateX(-100%); }
  100% { transform: translateX(400%); }
}

/* Cross-fade between body states (skeleton, error, empty, content).
 * Short enough (120ms) to not feel slow, long enough to register as a
 * transition rather than a cut. */
.widget-state-enter-active,
.widget-state-leave-active {
  transition: opacity 120ms ease;
}
.widget-state-enter-from,
.widget-state-leave-to {
  opacity: 0;
}
</style>
