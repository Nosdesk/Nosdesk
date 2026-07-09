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

Edit-mode affordances:
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
import { computed, inject, nextTick, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import {
  DASHBOARD_WIDGET_CONTEXT,
  type DashboardWidgetContext,
  type MoveDirection,
  type ResizePreviewIntent,
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
     * CSS `aspect-ratio` (e.g. `'2 / 1'`) for a PLOTTED chart body (LineChart,
     * heatmap) that has no intrinsic height. On the stacked mobile layout the
     * grid row is `auto` (indefinite), so a `height:100%` plot chain collapses
     * to 0 — worst on iOS WebKit. aspect-ratio manufactures a height from the
     * always-known width instead, so the plot never collapses and needs no pixel
     * height. On the xl lattice the row is definite: the body fills it and
     * aspect-ratio self-disables (it only applies when a dimension is `auto`).
     * Leave undefined for list/text/KPI widgets — they size to their content.
     */
    bodyAspect?: string
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

// Custom properties consumed by the body's aspect-ratio / min-height utilities.
const bodyStyle = computed(() => {
  const s: Record<string, string> = {}
  if (props.minBodyHeight) s['--dash-min-body'] = props.minBodyHeight
  if (props.bodyAspect) s['--dash-aspect'] = props.bodyAspect
  return Object.keys(s).length ? s : undefined
})

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
const currentRowSpan = computed<WidgetSpan>(() => ctx?.currentRowSpan.value ?? 1)
const minSpan = computed<WidgetSpan>(() => ctx?.minSpan ?? 1)
const minRowSpan = computed<WidgetSpan>(() => ctx?.minRowSpan ?? 1)

function onResize(span: WidgetSpan) {
  ctx?.onResize(span)
}
function onResizeRow(rowSpan: WidgetSpan) {
  ctx?.onResizeRow(rowSpan)
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
// or the keyboard context-menu key. Width / Height radio groups +
// hide live here; removing them from the header keeps the chrome
// quiet while leaving the affordance one gesture away. Hovering (or
// focus-traversing) a size option live-previews it: the grid
// re-packs around the previewed footprint and reverts if the menu
// closes without a selection. Nothing is written to the store until
// select, so a committed change stays one undo step.
const menuOpen = ref(false)
const menuX = ref(0)
const menuY = ref(0)

const SPAN_OPTIONS: WidgetSpan[] = [1, 2, 3]

const menuItems = computed<MenuItem[]>(() => [
  {
    id: 'width-heading',
    label: t('dashboard-widget-context-menu-width-heading'),
    heading: true,
  },
  ...SPAN_OPTIONS.map<MenuItem>((n) => ({
    id: `width-${n}`,
    label: t(`dashboard-widget-context-menu-width-${n}`),
    checked: currentSpan.value === n,
    disabled: n < minSpan.value,
    trailing: `${n}`,
  })),
  {
    id: 'height-heading',
    label: t('dashboard-widget-context-menu-height-heading'),
    heading: true,
    divider: true,
  },
  ...SPAN_OPTIONS.map<MenuItem>((n) => ({
    id: `height-${n}`,
    label: t(`dashboard-widget-context-menu-height-${n}`),
    checked: currentRowSpan.value === n,
    disabled: n < minRowSpan.value,
    trailing: `⇧${n}`,
  })),
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
  ctx?.onSizeMenuToggle(true)
}

/** Map a size menu id to its preview intent; null for other items. */
function sizeIntentFor(id: string): ResizePreviewIntent | null {
  const m = /^(width|height)-([123])$/.exec(id)
  if (!m) return null
  const n = Number(m[2]) as WidgetSpan
  return m[1] === 'width' ? { span: n } : { rowSpan: n }
}

function onMenuSelect(id: string) {
  const intent = sizeIntentFor(id)
  if (intent) {
    ctx?.onPreviewResize(null)
    if (intent.span) onResize(intent.span)
    if (intent.rowSpan) onResizeRow(intent.rowSpan)
    return
  }
  if (id === 'hide') onHide()
}

function onMenuHighlight(id: string) {
  const intent = sizeIntentFor(id)
  if (intent) ctx?.onPreviewResize(intent)
}

function onMenuUnhighlight(id: string) {
  if (sizeIntentFor(id)) ctx?.onPreviewResize(null)
}

/** Menu dismissed without a selection (click-away, Esc, scroll):
 *  drop any in-flight preview so the grid reverts. */
function onMenuClose() {
  menuOpen.value = false
  ctx?.onPreviewResize(null)
  ctx?.onSizeMenuToggle(false)
}

/** Keyboard model on the focused card (edit mode only, and only
 *  when the event target is the widget root itself, not bubbled up
 *  from a focused control inside the body, so typing "1" into a
 *  filter input inside a widget doesn't silently resize the card):
 *
 *    arrows       move the widget (up/down target the vertically
 *                 adjacent widget on the packed lattice)
 *    1 / 2 / 3    set the column span
 *    ⇧1 / ⇧2 / ⇧3 set the row span
 *
 *  Digits match on `e.code` because Shift+1 reports `key: "!"` on
 *  most layouts. `stopPropagation` AND `preventDefault` are both
 *  required: the dashboard's document-level keybindings
 *  (useDashboardKeybindings) also listen, and arrows must not
 *  scroll the page. */
const articleEl = ref<HTMLElement | null>(null)

async function onCardKeydown(e: KeyboardEvent) {
  if (!editMode.value) return
  if (e.target !== e.currentTarget) return
  if (e.metaKey || e.ctrlKey || e.altKey) return

  const dir = arrowToDirection(e.key)
  if (dir) {
    e.preventDefault()
    e.stopPropagation()
    ctx?.onMove(dir)
    // The grid reflow can move this element in the DOM, which blurs
    // it; re-focus so repeated presses keep working, and keep the
    // moved card in view.
    await nextTick()
    articleEl.value?.focus({ preventScroll: true })
    articleEl.value?.scrollIntoView({ block: 'nearest' })
    return
  }

  const span = sizeCodeToSpan(e.code)
  if (span === null) return
  e.preventDefault()
  e.stopPropagation()
  if (e.shiftKey) {
    if (span >= minRowSpan.value) onResizeRow(span)
  } else {
    if (span >= minSpan.value) onResize(span)
  }
}

function arrowToDirection(key: string): MoveDirection | null {
  switch (key) {
    case 'ArrowLeft':
      return 'left'
    case 'ArrowRight':
      return 'right'
    case 'ArrowUp':
      return 'up'
    case 'ArrowDown':
      return 'down'
    default:
      return null
  }
}

function sizeCodeToSpan(code: string): WidgetSpan | null {
  switch (code) {
    case 'Digit1':
    case 'Numpad1':
      return 1
    case 'Digit2':
    case 'Numpad2':
      return 2
    case 'Digit3':
    case 'Numpad3':
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
    ref="articleEl"
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
        'min-h-0 flex flex-col',
        // Chart bodies (bodyAspect) derive height from width via aspect-ratio so
        // a fill-height plot never collapses on the indefinite-height mobile grid
        // (iOS WebKit), and fill the definite row on the xl lattice (where
        // aspect-ratio self-disables). Capped at the widget's own rowSpan height
        // so a wide 1-col card can't produce a giant plot. Non-chart bodies keep
        // flex-1 + scroll and size to their content.
        bodyAspect
          ? 'aspect-[var(--dash-aspect)] max-h-[var(--dash-max-h)] overflow-hidden xl:aspect-auto xl:max-h-none xl:flex-1'
          : 'flex-1 overflow-y-auto',
        // minBodyHeight is a desktop load-shift guard (keeps skeleton + sparse
        // states the same height on the fixed grid). It's xl-only so the 1-col
        // mobile layout collapses empty/sparse widgets to their content instead.
        minBodyHeight ? 'xl:min-h-[var(--dash-min-body)]' : '',
        densityPadding,
        dragging ? 'opacity-40 pointer-events-none' : '',
      ]"
      :style="bodyStyle"
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
      @highlight="onMenuHighlight"
      @unhighlight="onMenuUnhighlight"
      @close="onMenuClose"
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
