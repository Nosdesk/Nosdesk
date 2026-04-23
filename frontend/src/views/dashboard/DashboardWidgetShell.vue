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

Edit-mode affordances (drag grip / hide × / size selector) live in
the shell's header — they're part of the card, not stickers floating
over it. The shell doesn't own the drag *logic* (that's
`usePointerSortable` in the dashboard parent); it only renders the
controls and emits the events when the parent has flipped edit-mode
on via `provide()`.
-->
<script setup lang="ts">
import { computed, inject } from 'vue'
import {
  DASHBOARD_WIDGET_CONTEXT,
  type DashboardWidgetContext,
} from './widgetContext'
import type { WidgetSpan } from './widgets'

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
    actionLabel: 'View all',
    emptyTitle: 'Nothing here yet.',
    emptyDescription: '',
    flushBody: true,
  },
)

// Edit-mode context is optional — when a widget is rendered outside
// the dashboard (e.g. a ticket list on a profile page), the context
// is undefined and the shell renders with no edit affordances at all.
const ctx = inject<DashboardWidgetContext | undefined>(DASHBOARD_WIDGET_CONTEXT, undefined)

const editMode = computed(() => ctx?.editMode.value ?? false)
const dragging = computed(() => ctx?.dragging.value ?? false)
const currentSpan = computed<WidgetSpan>(() => ctx?.currentSpan.value ?? 1)

const SIZES: WidgetSpan[] = [1, 2, 3]

function onResize(span: WidgetSpan) {
  ctx?.onResize(span)
}
function onHide() {
  ctx?.onHide()
}
function onHandlePointerDown(e: PointerEvent) {
  ctx?.onHandlePointerDown(e)
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
    class="bg-surface rounded-xl border border-default overflow-hidden flex flex-col h-full relative"
  >
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
      Header: fixed-height pill that houses the drag grip (edit),
      title, header-actions slot (filter controls, etc.), the
      "View all" link, the size selector (edit), and hide × (edit).
    -->
    <header class="flex items-center gap-2 px-3 h-9 border-b border-default bg-surface-alt flex-shrink-0">
      <!-- Drag grip — only in edit mode. Sits to the left of the title
           so it reads as "this row is draggable" without visual noise. -->
      <button
        v-if="editMode"
        type="button"
        class="flex items-center justify-center w-5 h-5 rounded text-tertiary hover:text-primary cursor-grab active:cursor-grabbing touch-none"
        :aria-label="`Drag ${title}`"
        @pointerdown="onHandlePointerDown"
      >
        <svg class="w-3.5 h-3.5" fill="currentColor" viewBox="0 0 16 16">
          <circle cx="5" cy="3" r="1.2" />
          <circle cx="5" cy="8" r="1.2" />
          <circle cx="5" cy="13" r="1.2" />
          <circle cx="11" cy="3" r="1.2" />
          <circle cx="11" cy="8" r="1.2" />
          <circle cx="11" cy="13" r="1.2" />
        </svg>
      </button>

      <h2 class="text-[13px] font-semibold text-primary truncate flex-1 tracking-tight">{{ title }}</h2>

      <!-- Widget-specific header controls (e.g. filter dropdowns). -->
      <slot name="headerActions" />

      <!-- "View all" link. Hidden in edit mode to keep the header
           compact for the edit controls. -->
      <router-link
        v-if="actionTo && !editMode"
        :to="actionTo"
        class="text-[11px] font-medium text-accent hover:underline whitespace-nowrap"
      >
        {{ actionLabel }} →
      </router-link>

      <!-- Edit-mode: size selector + hide. Replace the view-all link. -->
      <template v-if="editMode">
        <div
          role="radiogroup"
          :aria-label="`${title} size`"
          class="inline-flex items-center gap-0.5 p-0.5 rounded-full bg-surface border border-default"
        >
          <button
            v-for="size in SIZES"
            :key="size"
            type="button"
            role="radio"
            :aria-checked="currentSpan === size"
            :class="[
              'h-5 px-1.5 inline-flex items-center justify-center rounded-full text-[10px] font-semibold transition-colors',
              currentSpan === size
                ? 'bg-accent text-white'
                : 'text-tertiary hover:text-primary',
            ]"
            :title="`Size ${size} of 3`"
            @click="onResize(size)"
          >
            {{ size }}
          </button>
        </div>
        <button
          type="button"
          class="flex items-center justify-center w-5 h-5 rounded text-tertiary hover:text-status-error transition-colors"
          :aria-label="`Hide ${title}`"
          :title="`Hide ${title}`"
          @click="onHide"
        >
          <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" stroke-width="2.5" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </template>
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
          :aria-label="`Loading ${title}`"
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
            <p class="text-sm text-secondary">{{ emptyTitle }}</p>
            <p v-if="emptyDescription" class="text-xs text-tertiary mt-1">{{ emptyDescription }}</p>
          </slot>
        </div>
        <div v-else key="content" class="flex-1 flex flex-col">
          <slot />
        </div>
      </Transition>
    </div>
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
