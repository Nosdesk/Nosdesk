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
  /** Built-in views, surfaced as a tab strip in the header. The
   * shape-icon hint (`list` / `calendar`) makes the calendar tab
   * visually distinct so users don't have to learn that "Calendar"
   * is a different renderer from the list views. */
  tabItems: ComputedRef<ViewTabItem[]>
  /** User-curated saved views (workspace / project / private),
   * grouped for the secondary `<ViewSwitcher>` dropdown. Empty
   * when the workspace hasn't created any. */
  savedItems: ComputedRef<ViewSwitcherItem[]>
  selectViewById: (id: string) => void
}

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
  //   list    — queue of items ("everything in motion")
  //   inbox   — unsorted incoming tray
  //   calendar— tickets placed on dates
  const TAB_ICON: Record<string, IconName> = {
    'my-open': 'me',
    'all-active': 'list',
    'triage': 'inbox',
    'calendar': 'calendar',
  }

  const tabItems = computed<ViewTabItem[]>(() =>
    BUILTIN_VIEWS.map((v) => ({
      id: v.id,
      name: t(v.nameKey, v.name),
      // Fallback to a shape hint for any future built-in that
      // ships before we pick a bespoke icon for it.
      icon: TAB_ICON[v.id] ?? (v.shape.type === 'calendar' ? 'calendar' : 'list'),
    })),
  )

  const savedItems = computed<ViewSwitcherItem[]>(() => {
    const items: ViewSwitcherItem[] = []
    const groupLabel = {
      workspace: 'Workspace',
      project: 'Project',
      private: 'Private',
    } as const
    for (const scope of ['workspace', 'project', 'private'] as const) {
      const subset = savedViewsRef.value.filter((v) => v.scope === scope)
      for (const v of subset) {
        items.push({
          id: v.uuid,
          name: v.name,
          group: groupLabel[scope],
          editable: true,
        })
      }
    }
    return items
  })

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

  return { activeView, savedViews, tabItems, savedItems, selectViewById }
}
