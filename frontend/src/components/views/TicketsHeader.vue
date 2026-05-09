<script setup lang="ts">
/**
 * Tickets page header. Two-row layout matching the reference
 * design (Linear / Plain conventions):
 *
 *   Row 1: [View name ▾]   12 open · 3 paused · 2 breached   [Pill] [Pill]   ░ ░ ░   [+ New]
 *   Row 2: [+ Add filter]
 *
 * The toolbar stays quiet by default — when no filters are
 * applied, the chip strip is empty and only the dashed "+ Add
 * filter" affordance is visible. Filters appear inline with
 * the title as removable amber pills, putting the active
 * filter state in the user's primary read zone instead of
 * burying it in dropdown labels.
 *
 * Density toggles surface as three icon buttons on the right —
 * one-click switching between compact / cosy / comfortable so
 * power users don't have to dig into a menu for the most
 * common display change.
 */
import { computed, ref } from 'vue'
import ViewSwitcher, {
  type ViewSwitcherItem,
} from '@/components/views/ViewSwitcher.vue'
import TicketsViewTabs, {
  type ViewTabItem,
} from '@/components/views/TicketsViewTabs.vue'
import DisplayMenu from '@/components/views/DisplayMenu.vue'
import FilterPill from '@/components/views/FilterPill.vue'
import AddFilterMenu from '@/components/views/AddFilterMenu.vue'
import Icon from '@/components/common/Icon.vue'
import {
  FACET_META,
  getOptionsFor,
  selectedAsStringSet,
  summariseSelected,
  type FilterOption,
} from '@/components/views/filterFacets'
import { useUsersDirectory } from '@/composables/useUsersDirectory'
import type { ColumnId, ListColumn } from '@/sync/views/ticketColumns'
import type { Density } from '@/composables/useTicketsDensity'
import type { GroupBy } from '@/composables/useTicketsGrouping'
import type {
  FilterFacet,
  SlaFilter,
} from '@/composables/useTicketsFilters'
import type { CardData, Priority } from '@/sync/views/types'

const props = defineProps<{
  /** Built-in views (My Open / All Active / Triage / Calendar)
   * rendered as a primary tab strip on tablet+. The same items
   * also slot into the mobile dropdown so phone users still
   * reach them. */
  tabItems: ViewTabItem[]
  /** Saved / project / private views — rendered behind a smaller
   * `Saved ▾` dropdown next to the tabs. Empty array hides the
   * dropdown. */
  savedItems: ViewSwitcherItem[]
  activeViewId: string
  /** Source set used to derive option lists for status / assignee
   * / cycle pickers — should be the post-view, pre-filter card
   * set so filters don't self-erase as they're applied. */
  sourceCards: CardData[]
  density: Density
  groupBy: GroupBy
  visibleColumns: ColumnId[]
  canSaveLayoutToView: boolean
  layoutDirty: boolean
  /** Summary segments rendered inline with the title. */
  summarySegments: { label: string; tone: 'default' | 'amber' | 'red' }[]
  /** Active filter state — both the per-facet sets / strings
   * (so the picker can pre-check) and the list of currently
   * active facets (so the header knows which pills to render). */
  activeFacets: FilterFacet[]
  filterTitle: string
  filterStatus: Set<number>
  filterPriority: Set<Priority>
  filterAssignee: Set<string>
  filterSla: Set<SlaFilter>
  filterCycle: Set<number>
  /** Whether the right preview pane is open. Drives the
   * split-view toggle's active state. */
  splitViewEnabled: boolean
  /** Filter facets the AddFilterMenu offers. Pre-filtered by the
   * shell against workspace capabilities so disabled features
   * (eg. SLA when no policies exist) never appear in the picker. */
  facetOrder: FilterFacet[]
  /** Columns the DisplayMenu offers in the Properties checkbox
   * list. Same gating principle as facetOrder: shell pre-filters
   * by capabilities so togglable properties for disabled features
   * never appear. */
  availableColumns: readonly ListColumn[]
}>()

const emit = defineEmits<{
  (e: 'select-view', id: string): void
  (e: 'edit-view', id: string): void
  (e: 'save-as-view'): void
  (e: 'set-density', value: Density): void
  (e: 'set-group-by', value: GroupBy): void
  (e: 'toggle-column', id: ColumnId): void
  (e: 'reset-layout'): void
  (e: 'save-layout-to-view'): void
  (e: 'new-ticket'): void
  (e: 'toggle-filter', facet: FilterFacet, value: string): void
  (e: 'clear-filter', facet: FilterFacet): void
  (e: 'set-filter-text', facet: FilterFacet, value: string): void
  (e: 'toggle-split-view'): void
}>()

// Mobile fallback: the dropdown carries the full set (built-ins
// + saved) so phone users still reach every view from one
// affordance. The desktop split (tabs + saved-only dropdown)
// would crowd phone-width headers.
const mobileSwitcherItems = computed<ViewSwitcherItem[]>(() => [
  ...props.tabItems.map((t) => ({ id: t.id, name: t.name, group: 'Built-in' })),
  ...props.savedItems,
])

const { getUserHandle } = useUsersDirectory()
const addFilterRef = ref<InstanceType<typeof AddFilterMenu> | null>(null)

// ---------------------------------------------------------------
// Pickers and pills resolve options through the same helpers
// in `filterFacets.ts`, so the +Add menu and the click-to-edit
// flow stay in lockstep.
// ---------------------------------------------------------------
function resolveUser(uuid: string) {
  // Reading through the directory's handle keeps the pills computed
  // reactive on user-data arrival: when the sync engine's user pool
  // gains a row for this uuid (bootstrap or user.created SSE), the
  // handle's `user` computed updates and any computed that touched
  // `.value` re-evaluates.
  return getUserHandle(uuid).user.value
}

function optionsFor(facet: FilterFacet): FilterOption[] {
  return getOptionsFor(facet, props.sourceCards, resolveUser)
}

function selectedFor(facet: FilterFacet): Set<string> {
  return selectedAsStringSet(
    facet,
    props.filterStatus,
    props.filterPriority,
    props.filterAssignee,
    props.filterSla,
    props.filterCycle,
  )
}

function textValueFor(facet: FilterFacet): string {
  return facet === 'title' ? props.filterTitle : ''
}

function pillValueSummary(facet: FilterFacet): string {
  if (facet === 'title') {
    return summariseSelected(facet, props.filterTitle, [])
  }
  return summariseSelected(facet, selectedFor(facet), optionsFor(facet))
}

/**
 * Pre-computed pill descriptors. Each entry materialises options
 * + selected + summary + text value per active facet exactly once
 * per change of any underlying dep (`activeFacets`, `sourceCards`,
 * the per-facet filter values), rather than re-running the four
 * resolver functions inline on every parent re-render.
 *
 * Why it matters here: `optionsFor(facet)` reads `dataStore` which
 * holds reactive caches of users / categories / cycles; an SSE
 * burst that touches the user cache used to re-run every facet's
 * option resolution on every TicketsHeader render even when the
 * facet's value hadn't changed. Pinning the materialisation to a
 * single computed lets Vue's tracking collapse the redundant work
 * and re-renders only when the dep set actually changes.
 *
 * Keep `optionsFor` / `selectedFor` / `textValueFor` as standalone
 * functions because AddFilterMenu receives them as prop functions
 * and calls them in its own render scope — that surface stays a
 * function-based API.
 */
const pills = computed(() => {
  return props.activeFacets.map((facet) => {
    const options = optionsFor(facet)
    const selected = selectedFor(facet)
    const textValue = textValueFor(facet)
    const valueSummary = facet === 'title'
      ? summariseSelected(facet, props.filterTitle, [])
      : summariseSelected(facet, selected, options)
    return {
      facet,
      label: FACET_META[facet].label,
      options,
      selected,
      textValue,
      valueSummary,
    }
  })
})

const toneClass = computed<(tone: 'default' | 'amber' | 'red') => string>(() => (tone) => {
  if (tone === 'red') return 'text-rose-600 dark:text-rose-400 font-medium'
  if (tone === 'amber') return 'text-amber-600 dark:text-amber-400'
  return 'text-tertiary'
})

const densityOptions: ReadonlyArray<{ value: Density; title: string; svg: string }> = [
  {
    value: 'compact',
    title: 'Compact',
    // Four tight horizontal lines.
    svg: 'M3 5h14M3 9h14M3 13h14M3 17h14',
  },
  {
    value: 'cosy',
    title: 'Cosy',
    // Three medium-spaced lines.
    svg: 'M3 5h14M3 10h14M3 15h14',
  },
  {
    value: 'comfortable',
    title: 'Comfortable',
    // Two widely-spaced lines.
    svg: 'M3 6h14M3 14h14',
  },
]

/** Public method exposed on the host element so the shell can
 * trigger the AddFilter popover from a `/` keystroke without
 * mounting the menu's trigger button itself. */
function openAddFilter(facet: FilterFacet): void {
  addFilterRef.value?.openWithFacet(facet)
}

defineExpose({ openAddFilter })
</script>

<template>
  <header
    class="flex flex-col gap-1.5 px-4 py-3 border-b border-subtle bg-surface shrink-0"
  >
    <!-- Row 1: title + summary stats + filter pills + display + new
         Mobile: power-user chrome (density toggle, split-view, display
         menu) hides below md: to keep the toolbar reachable on phones.
         Title + summary + filter pills + New ticket stay across all
         widths. -->
    <div class="flex items-center gap-2 sm:gap-3 flex-wrap">
      <!-- Desktop (sm:+): primary tab strip for built-in views.
           Calendar in particular needed first-class access — it's
           a different shape (CalendarBoard vs the list table) and
           used often enough that one click should land it. The
           saved-view dropdown sits alongside as the secondary
           affordance for user-curated subsets.

           Mobile: collapse to one dropdown carrying the full set
           (built-ins + saved). Four 90px tabs don't fit on a
           phone-width header, and the dropdown is the convention
           every other surface uses there too. -->
      <TicketsViewTabs
        :items="tabItems"
        :active-id="activeViewId"
        @select="(id) => emit('select-view', id)"
      />
      <!-- Saved-views-only dropdown next to the tabs. Hidden when
           the workspace has no saved views (the tab strip already
           covers everything). When the active view IS a saved one,
           the dropdown's trigger label flips to that view's name
           so the active state stays visible somewhere on screen. -->
      <ViewSwitcher
        v-if="savedItems.length > 0"
        class="hidden sm:inline-flex"
        :items="savedItems"
        :active-id="activeViewId"
        size="sm"
        placeholder="Saved"
        @select="(id) => emit('select-view', id)"
        @edit="(id) => emit('edit-view', id)"
      />
      <!-- Mobile fallback: full switcher (built-ins + saved). -->
      <ViewSwitcher
        class="sm:hidden"
        :items="mobileSwitcherItems"
        :active-id="activeViewId"
        size="lg"
        @select="(id) => emit('select-view', id)"
        @edit="(id) => emit('edit-view', id)"
      />

      <!-- Summary stats. Inline · separators between non-zero
           categories. Empty when no tickets, so we don't render
           a lonely "0 open" tail. -->
      <div
        v-if="summarySegments.length > 0"
        class="flex items-center gap-2 text-xs"
      >
        <template v-for="(seg, i) in summarySegments" :key="i">
          <span
            v-if="i > 0"
            class="text-tertiary/40"
            aria-hidden="true"
          >·</span>
          <span :class="toneClass(seg.tone)">{{ seg.label }}</span>
        </template>
      </div>

      <!-- Active filter pills — only render when active. Quiet
           toolbar by default. TransitionGroup gives each pill a
           subtle slide-in / scale-out so adding and removing
           filters feels intentional rather than glitchy. -->
      <TransitionGroup
        v-if="activeFacets.length > 0"
        name="filter-pill"
        tag="div"
        class="flex items-center gap-1.5 flex-wrap"
      >
        <FilterPill
          v-for="pill in pills"
          :key="pill.facet"
          :facet="pill.facet"
          :label="pill.label"
          :value-summary="pill.valueSummary"
          :options="pill.options"
          :selected="pill.selected"
          :text-value="pill.textValue"
          @toggle="(v) => emit('toggle-filter', pill.facet, v)"
          @clear="emit('clear-filter', pill.facet)"
          @set-text="(v) => emit('set-filter-text', pill.facet, v)"
          @remove="emit('clear-filter', pill.facet)"
        />
      </TransitionGroup>

      <div class="flex-1 min-w-2" />

      <!-- Density quick-toggle. Three icon buttons. Hidden below
           md: — phones don't have meaningful row density choices. -->
      <div
        class="hidden md:inline-flex items-center rounded-md border border-subtle overflow-hidden h-7"
        role="group"
        aria-label="Row density"
      >
        <button
          v-for="opt in densityOptions"
          :key="opt.value"
          type="button"
          class="h-full w-7 flex items-center justify-center transition-colors"
          :class="density === opt.value
            ? 'bg-accent/15 text-accent'
            : 'text-tertiary hover:text-primary hover:bg-surface-hover'"
          :aria-pressed="density === opt.value"
          :title="opt.title"
          @click="emit('set-density', opt.value)"
        >
          <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" class="w-3.5 h-3.5">
            <path :d="opt.svg" />
          </svg>
        </button>
      </div>

      <!-- Split-view toggle. Two-pane SVG so the icon reads as
           "list + preview" not just generic 'split'. Hidden below
           md: — split-view doesn't fit on phones, the list view
           collapses to single-pane on small screens regardless. -->
      <button
        type="button"
        class="hidden md:inline-flex items-center justify-center w-7 h-7 rounded-md transition-colors"
        :class="splitViewEnabled
          ? 'bg-accent/15 text-accent'
          : 'text-tertiary hover:text-primary hover:bg-surface-hover'"
        :aria-pressed="splitViewEnabled"
        :title="splitViewEnabled ? 'Hide preview pane' : 'Show preview pane'"
        @click="emit('toggle-split-view')"
      >
        <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" class="w-4 h-4">
          <rect x="2.5" y="3.5" width="15" height="13" rx="1.5" />
          <line x1="11.5" y1="3.5" x2="11.5" y2="16.5" />
          <line x1="4.5" y1="7" x2="9.5" y2="7" />
          <line x1="4.5" y1="9.5" x2="9.5" y2="9.5" />
          <line x1="4.5" y1="12" x2="9.5" y2="12" />
        </svg>
      </button>

      <!-- Column / density / grouping controls. Power-user chrome,
           hidden below md: to keep the mobile toolbar shallow. -->
      <DisplayMenu
        class="hidden md:inline-flex"
        :visible="visibleColumns"
        :density="density"
        :group-by="groupBy"
        :can-save-to-view="canSaveLayoutToView"
        :layout-dirty="layoutDirty"
        :available-columns="availableColumns"
        @toggle-column="(id) => emit('toggle-column', id)"
        @set-density="(v) => emit('set-density', v)"
        @set-group-by="(v) => emit('set-group-by', v)"
        @reset="emit('reset-layout')"
        @save="emit('save-layout-to-view')"
      />

      <!-- New ticket — icon-only below sm: to free header space
           when the toolbar is already crowded; full label everywhere
           else so the primary CTA stays unambiguous. -->
      <button
        type="button"
        class="inline-flex items-center gap-1 text-xs font-medium px-2 sm:px-2.5 h-9 sm:h-7 rounded-md bg-accent text-on-accent hover:bg-accent/90 transition-colors"
        :title="'New ticket'"
        @click="emit('new-ticket')"
      >
        <Icon name="add" class="w-4 h-4 sm:w-3.5 sm:h-3.5" />
        <span class="hidden sm:inline">New ticket</span>
      </button>
    </div>

    <!-- Row 2: + Add filter (always visible) + Save as view (small) -->
    <div class="flex items-center gap-2">
      <AddFilterMenu
        ref="addFilterRef"
        :facet-order="facetOrder"
        :active-facets="activeFacets"
        :options-for="optionsFor"
        :selected-for="selectedFor"
        :text-value-for="textValueFor"
        @toggle="(facet, v) => emit('toggle-filter', facet, v)"
        @clear="(facet) => emit('clear-filter', facet)"
        @set-text="(facet, v) => emit('set-filter-text', facet, v)"
      />
      <button
        type="button"
        class="text-[11px] text-tertiary hover:text-primary px-2 h-6 rounded-md hover:bg-surface-hover transition-colors"
        title="Save current state as a private view"
        @click="emit('save-as-view')"
      >
        Save as view
      </button>
    </div>
  </header>
</template>

<style>
/* Pill insertion / removal animation. Slide in from the left
   (the direction of the +Add filter button) so the pill feels
   like it's being placed by the user; collapse to scale 0 on
   removal so the surrounding pills slide into the freed space.
   `filter-pill-move` handles the lateral shift of neighbours
   when one is removed mid-row, giving a settled feel. */
.filter-pill-enter-active,
.filter-pill-leave-active {
  transition:
    opacity 160ms cubic-bezier(0.16, 1, 0.3, 1),
    transform 160ms cubic-bezier(0.16, 1, 0.3, 1);
}
.filter-pill-enter-from {
  opacity: 0;
  transform: translateX(-6px) scale(0.92);
}
.filter-pill-leave-to {
  opacity: 0;
  transform: scale(0.92);
}
.filter-pill-leave-active {
  position: absolute;
}
.filter-pill-move {
  transition: transform 200ms cubic-bezier(0.16, 1, 0.3, 1);
}

@media (prefers-reduced-motion: reduce) {
  .filter-pill-enter-active,
  .filter-pill-leave-active,
  .filter-pill-move {
    transition: opacity 80ms linear;
  }
  .filter-pill-enter-from,
  .filter-pill-leave-to {
    transform: none;
  }
}
</style>
