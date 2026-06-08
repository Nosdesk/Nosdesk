/**
 * Resolves the active ticket view from the URL + saved-views
 * store. Owns the canonicalisation pass that adds `?view=<id>`
 * on first mount so reload + share-link navigation always lands
 * on the same view that just rendered.
 *
 * Resolution precedence:
 *   1. `?view=` matches a built-in view id           -> built-in
 *   2. `?view=` matches a saved-view uuid            -> saved
 *   3. MY_OPEN_VIEW                                  -> built-in
 *
 * The earlier "workspace default saved view (is_default)" tier was
 * removed 2026-05-09 along with the `is_default` column itself —
 * see `models::SavedView` for the rationale. There is now exactly
 * one default (MY_OPEN, with smart fall-through to ALL_ACTIVE in
 * TicketsListView when empty), no per-saved-view default-promotion.
 *
 * Saved views the user can switch into are filtered to list /
 * calendar shapes only — board / gantt / cycles live on
 * different routes.
 */
import { computed, onMounted, type ComputedRef } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { useSavedViewsStore } from '@/stores/savedViews'
import {
  BUILTIN_VIEWS,
  findBuiltinView,
  MY_OPEN_VIEW,
  type BuiltInView,
} from '@/sync/views/builtinViews'
import type { ViewSwitcherItem } from '@/components/views/ViewSwitcher.vue'
import type { ViewTabItem } from '@/components/views/TicketsViewTabs.vue'
import type { IconName } from '@/components/common/icons'
import type { SavedView } from '@/services/savedViewsService'
import type {
  CalendarViewShape,
  FilterState,
  ListViewShape,
  ViewShape,
} from '@/sync/views/types'

export interface ResolvedView {
  id: string
  name: string
  description: string
  shape: ListViewShape | CalendarViewShape
  filter: FilterState
  source: 'builtin' | 'saved'
  uuid?: string
}

export interface UseTicketsViewResolution {
  activeView: ComputedRef<ResolvedView>
  savedViews: ComputedRef<SavedView[]>
  /** The PRIMARY built-in views, surfaced as a one-click tab strip
   * in the header (My Open / My Active / All Active / Triage — the
   * daily drivers). The remaining built-ins live behind the
   * `overflowItems` dropdown so the strip can't sprawl horizontally
   * past four tabs no matter how many built-ins ship. */
  tabItems: ComputedRef<ViewTabItem[]>
  /** Desktop "Views ▾" dropdown contents at lg+: the NON-primary
   * built-ins (icon-differentiated, no group heading) plus saved
   * views grouped by scope (Workspace / Project / Private).
   * Deliberately excludes the primary built-ins — those are always
   * visible as tabs, so listing them here too would be redundant and
   * would make the dropdown trigger read the active view's name even
   * when a tab is lit. */
  overflowItems: ComputedRef<ViewSwitcherItem[]>
  /** The full view set (every built-in, icon-differentiated and
   * ungrouped, then saved views grouped by scope) for the single
   * mobile dropdown, where there's no room for a tab strip. */
  allViewItems: ComputedRef<ViewSwitcherItem[]>
  selectViewById: (id: string) => void
}

/** The built-ins promoted to one-click tabs. Everything else falls
 * through to the "Views ▾" overflow dropdown. Kept as a Set so the
 * tab / overflow split is a single source of truth. */
const PRIMARY_VIEW_IDS = new Set<string>([
  'my-open',
  'my-active',
  'all-active',
  'triage',
])


export function useTicketsViewResolution(): UseTicketsViewResolution {
  const route = useRoute()
  const router = useRouter()
  const fluent = useFluent()
  const t = (k: string, fallback: string): string => fluent.$t(k) || fallback

  function fromBuiltin(view: BuiltInView): ResolvedView {
    return {
      id: view.id,
      name: t(view.nameKey, view.name),
      description: t(view.descriptionKey, view.description),
      shape: view.shape,
      filter: view.filter,
      source: 'builtin',
    }
  }

  function fromSaved(view: SavedView): ResolvedView {
    return {
      id: view.uuid,
      name: view.name,
      description: view.scope === 'private' ? 'Private view' : 'Workspace view',
      shape: view.shape as ListViewShape | CalendarViewShape,
      filter: view.filter,
      source: 'saved',
      uuid: view.uuid,
    }
  }

  const savedViewsStore = useSavedViewsStore()
  const savedViewsRef = savedViewsStore.viewsForProject(null)

  const savedViews = computed<SavedView[]>(() =>
    savedViewsRef.value.filter((v) => {
      const shapeType = (v.shape as ViewShape | null)?.type
      return shapeType === 'list' || shapeType === 'calendar'
    }),
  )

  const activeView = computed<ResolvedView>(() => {
    const requested = (route.query.view as string | undefined) ?? ''
    const builtin = findBuiltinView(requested)
    if (builtin) return fromBuiltin(builtin)
    const saved = savedViews.value.find((v) => v.uuid === requested)
    if (saved) return fromSaved(saved)
    return fromBuiltin(MY_OPEN_VIEW)
  })

  // Each built-in tab gets a slice-specific icon, not a shape
  // hint. Three list-shape views all painted with the same `list`
  // glyph defeats the differentiation icons are supposed to
  // provide; the goal is at-a-glance recognition of WHICH slice
  // you're on, not what renderer it uses.
  //
  //   me      — single user silhouette ("mine")
  //   userPlus— person + badge ("my active assignments")
  //   list    — queue of items ("everything in motion")
  //   ticket  — stub-and-tear ticket ("every ticket, all statuses")
  //   user    — generic person placeholder ("needs an assignee")
  //   warning — triangle ("past due, needs attention")
  //   inbox   — unsorted incoming tray
  //   calendar— tickets placed on dates
  const TAB_ICON: Record<string, IconName> = {
    'my-open': 'me',
    'my-active': 'userPlus',
    'all-active': 'list',
    'all-tickets': 'ticket',
    'unassigned': 'user',
    'overdue': 'warning',
    'triage': 'inbox',
    'calendar': 'calendar',
  }

  // Only the primary built-ins become tabs; the rest fall through
  // to the overflow dropdown (see PRIMARY_VIEW_IDS). Filtering off
  // BUILTIN_VIEWS keeps the tab order aligned with the canonical
  // view ordering.
  const tabItems = computed<ViewTabItem[]>(() =>
    BUILTIN_VIEWS.filter((v) => PRIMARY_VIEW_IDS.has(v.id)).map((v) => ({
      id: v.id,
      name: t(v.nameKey, v.name),
      // Fallback to a shape hint for any future built-in that
      // ships before we pick a bespoke icon for it.
      icon: TAB_ICON[v.id] ?? (v.shape.type === 'calendar' ? 'calendar' : 'list'),
    })),
  )

  /** Built-ins as switcher rows. No group heading — the per-slice
   * icon does the differentiating, which avoids lonely one-item
   * sections (e.g. a "Calendar" heading over a single row).
   * `include` lets the overflow menu drop the primary views (already
   * tabs) while the mobile menu keeps the full set. */
  function builtinSwitcherItems(
    include: (v: BuiltInView) => boolean,
  ): ViewSwitcherItem[] {
    return BUILTIN_VIEWS.filter(include).map((v) => ({
      id: v.id,
      name: t(v.nameKey, v.name),
      icon: TAB_ICON[v.id] ?? (v.shape.type === 'calendar' ? 'calendar' : 'list'),
      editable: false,
    }))
  }

  const savedSwitcherItems = computed<ViewSwitcherItem[]>(() => {
    const items: ViewSwitcherItem[] = []
    const groupLabel = {
      workspace: 'Workspace',
      project: 'Project',
      private: 'Private',
    } as const
    for (const scope of ['workspace', 'project', 'private'] as const) {
      const subset = savedViewsRef.value.filter((v) => v.scope === scope)
      for (const v of subset) {
        // Saved views fall back to a shape-hint icon (list / calendar)
        // since they have no bespoke slice glyph.
        const shapeType = (v.shape as ViewShape | null)?.type
        items.push({
          id: v.uuid,
          name: v.name,
          icon: shapeType === 'calendar' ? 'calendar' : 'list',
          group: groupLabel[scope],
          editable: true,
        })
      }
    }
    return items
  })

  // Desktop overflow: non-primary built-ins (Queues / Calendar) +
  // saved views. Excludes primary built-ins by design.
  const overflowItems = computed<ViewSwitcherItem[]>(() => [
    ...builtinSwitcherItems((v) => !PRIMARY_VIEW_IDS.has(v.id)),
    ...savedSwitcherItems.value,
  ])

  // Mobile: the whole catalogue in one dropdown (no tab strip there).
  const allViewItems = computed<ViewSwitcherItem[]>(() => [
    ...builtinSwitcherItems(() => true),
    ...savedSwitcherItems.value,
  ])

  function selectViewById(id: string): void {
    router.push({ path: route.path, query: { ...route.query, view: id } })
  }

  onMounted(() => {
    if (!route.query.view) {
      router.replace({
        path: route.path,
        query: { ...route.query, view: activeView.value.id },
      })
    }
  })

  return {
    activeView,
    savedViews,
    tabItems,
    overflowItems,
    allViewItems,
    selectViewById,
  }
}
