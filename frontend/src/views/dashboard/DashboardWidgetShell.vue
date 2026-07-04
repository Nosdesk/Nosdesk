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
    /**
     * Three-taxonomy empty-state contract from the parent plan's
     * decision 12. Drives the default copy + icon when the caller
     * does not override via `emptyTitle` / `emptyDescription` /
     * `#empty` slot:
     *
     *   - `never-had-data`  fresh workspace, the row simply hasn't
     *                       been written yet. Onboarding tone.
     *   - `filtered`        rows exist but the active filter excludes
     *                       everything visible.
     *   - `unconfigured`    the feature needs admin setup before it
     *                       can show anything (e.g. SLA without a
     *                       policy). Calls the admin to configure.
     *
     * Defaults to `never-had-data` so existing callers stay
     * compatible.
     */
    emptyTaxonomy?: 'never-had-data' | 'filtered' | 'unconfigured'
    /** Default-slot replacement copy when `empty === true` and the
     *  caller doesn't provide the `#empty` slot. Falls back to the
     *  taxonomy default when omitted. */
    emptyTitle?: string
    emptyDescription?: string
    /** Optional CTA link beneath the empty-state description. Used
     *  most often by the `unconfigured` taxonomy ("Set up an SLA
     *  policy") to lead the admin into the right admin screen. */
    emptyCtaTo?: string
    emptyCtaLabel?: string
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
    /**
     * Body padding tier. Maps to the design-language density scale:
     *
     *   `compact`  → p-3   (44-52px hairline rows, list/queue widgets)
     *   `regular`  → p-4   (default widget body — balanced density)
     *   `spacious` → p-6   (hero KPI band, one-card-per-row contexts)
     *
     * Has no effect when `flushBody` is true (the widget owns its own
     * internal padding). Defaults to `regular`.
     */
    density?: 'compact' | 'regular' | 'spacious'
  }>(),
  {
    actionLabel: '',
    emptyTaxonomy: 'never-had-data',
    emptyTitle: '',
    emptyDescription: '',
    emptyCtaTo: '',
    emptyCtaLabel: '',
    flushBody: true,
    density: 'regular',
  },
)

const densityPadding = computed(() => {
  if (props.flushBody) return ''
  switch (props.density) {
    case 'compact':
      return 'p-3'
    case 'spacious':
      return 'p-6'
    default:
      return 'p-4'
  }
})

const actionLabelText = computed(() => props.actionLabel || t('dashboard-widget-shell-action-view-all'))
const emptyTitleText = computed(() => {
  if (props.emptyTitle) return props.emptyTitle
  return t(`dashboard-widget-shell-empty-${props.emptyTaxonomy}-title`)
})
const emptyDescriptionText = computed(() => {
  if (props.emptyDescription) return props.emptyDescription
  return t(`dashboard-widget-shell-empty-${props.emptyTaxonomy}-description`)
})
const emptyCtaLabelText = computed(() => {
  if (props.emptyCtaLabel) return props.emptyCtaLabel
  return props.emptyCtaTo ? t('dashboard-widget-shell-empty-cta-default') : ''
})

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

/** Header acts as the sole drag source in edit mode. Skip when the
 *  event originated inside an interactive descendant (View-all link,
 *  slot buttons) so clicks on those still work. */
function onHeaderPointerDown(e: PointerEvent) {
  if (!ctx?.editMode.value) return
  const target = e.target as HTMLElement | null
  if (target?.closest('a, button, [role="button"], input, select, textarea')) return
  ctx.onHandlePointerDown(e)
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
  <!-- Projected-reorder model: while a widget is being dragged, the
       grid moves it to its projected post-commit slot and renders it
       there with a dashed accent outline + dimmed body. The widget
       itself IS the magnet zone, sized exactly to where it will land.
       Siblings reflow around it via the grid's FLIP transition so the
       destination layout previews correctly. -->
  <article
    :class="[
      'bg-surface rounded-xl border border-default overflow-hidden flex h-full relative transition-shadow',
      editMode
        ? 'ring-1 ring-accent/30 hover:ring-accent/40 focus-visible:ring-2 focus-visible:ring-accent focus-visible:outline-none'
        : '',
      dragging ? 'outline outline-2 outline-dashed outline-accent outline-offset-2 cursor-grabbing' : '',
    ]"
    :tabindex="editMode ? 0 : -1"
    @contextmenu="onContextMenu"
    @keydown="onCardKeydown"
  >
    <div class="flex flex-col flex-1 min-w-0 min-h-0">
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
    <!-- Title-bar grab: in edit mode the entire header is a drag
         source. Pointerdown bubbles up here; interactive children
         (the View-all link, the headerActions slot, the right-click
         menu) all fire their handlers on pointerup or click, which
         resolve before usePointerSortable's clickThreshold elapses,
         so they still work without snagging the drag. -->
    <header
      :class="[
        'flex items-center gap-2 px-3 h-9 border-b border-default bg-surface-alt flex-shrink-0',
        editMode ? 'cursor-grab active:cursor-grabbing touch-none' : '',
      ]"
      @pointerdown="onHeaderPointerDown"
    >
      <h2 class="text-[13px] font-semibold text-primary truncate tracking-tight">{{ title }}</h2>

      <!-- Optional inline count badge next to the title (e.g. "12"
           assigned tickets). Renders as a pill: h-5, rounded, mono
           tabular-nums against a surface-hover tint. -->
      <slot name="subtitle" />

      <div class="flex-1" />

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
      :class="[
        'flex-1 min-h-0 flex flex-col overflow-y-auto',
        // minBodyHeight is a desktop load-shift guard (keeps skeleton + sparse
        // states the same height on the fixed grid). It's xl-only so the 1-col
        // mobile layout collapses empty/sparse widgets to their content instead.
        minBodyHeight ? 'xl:min-h-[var(--dash-min-body)]' : '',
        densityPadding,
        dragging ? 'opacity-40 pointer-events-none' : '',
      ]"
      :style="minBodyHeight ? { '--dash-min-body': minBodyHeight } : undefined"
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
          class="flex-1 flex flex-col min-h-0 h-full"
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
          class="flex-1 flex flex-col items-center justify-center py-6 text-center px-4 gap-2"
        >
          <slot name="empty">
            <p class="text-sm text-secondary">{{ emptyTitleText }}</p>
            <p v-if="emptyDescriptionText" class="text-xs text-tertiary">{{ emptyDescriptionText }}</p>
            <router-link
              v-if="emptyCtaTo"
              :to="emptyCtaTo"
              class="mt-1 text-xs font-medium text-accent hover:underline"
            >
              {{ emptyCtaLabelText }} →
            </router-link>
          </slot>
        </div>
        <div v-else key="content" class="flex-1 flex flex-col min-h-0 h-full">
          <slot />
        </div>
      </Transition>
    </div>

    <!-- Optional footer pinned below the body region. Widgets with a
         plot + caption row (heatmap legend, chart axis notes) use this
         instead of baking chrome into the default slot so the plot can
         consume `flex-1 min-h-0` without manual ResizeObserver budgets. -->
    <footer
      v-if="$slots.footer"
      :class="[
        'shrink-0 flex items-center border-t border-default px-3 py-1.5 min-h-[1.75rem]',
        dragging ? 'opacity-40 pointer-events-none' : '',
      ]"
    >
      <slot name="footer" />
    </footer>
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
