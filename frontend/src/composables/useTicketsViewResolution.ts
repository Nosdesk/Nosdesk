/**
 * Resolves the active ticket view from the URL + saved-views
 * store. Owns the canonicalisation pass that adds `?view=<id>`
 * on first mount so reload + share-link navigation always lands
 * on the same view that just rendered.
 *
 * Resolution precedence:
 *   1. `?view=` matches a built-in view id           -> built-in
 *   2. `?view=` matches a saved-view uuid            -> saved
 *   3. Workspace default saved view (is_default)     -> saved
 *   4. MY_QUEUE_VIEW                                 -> built-in
 *
 * Saved views the user can switch into are filtered to list /
 * calendar shapes only — board / gantt / cycles live on
 * different routes.
 */
import { computed, onMounted, type ComputedRef } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useSavedViewsStore } from '@/stores/savedViews'
import {
  BUILTIN_VIEWS,
  findBuiltinView,
  MY_QUEUE_VIEW,
  type BuiltInView,
} from '@/sync/views/builtinViews'
import type { ViewSwitcherItem } from '@/components/views/ViewSwitcher.vue'
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
  switcherItems: ComputedRef<ViewSwitcherItem[]>
  selectViewById: (id: string) => void
}

function fromBuiltin(view: BuiltInView): ResolvedView {
  return {
    id: view.id,
    name: view.name,
    description: view.description,
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

export function useTicketsViewResolution(): UseTicketsViewResolution {
  const route = useRoute()
  const router = useRouter()
  const savedViewsStore = useSavedViewsStore()
  const savedViewsRef = savedViewsStore.viewsForProject(null)

  const savedViews = computed<SavedView[]>(() =>
    savedViewsRef.value.filter((v) => {
      const t = (v.shape as ViewShape | null)?.type
      return t === 'list' || t === 'calendar'
    }),
  )

  const workspaceDefault = computed<SavedView | null>(
    () =>
      savedViewsRef.value.find(
        (v) => v.scope === 'workspace' && v.is_default && v.archived_at == null,
      ) ?? null,
  )

  const activeView = computed<ResolvedView>(() => {
    const requested = (route.query.view as string | undefined) ?? ''
    const builtin = findBuiltinView(requested)
    if (builtin) return fromBuiltin(builtin)
    const saved = savedViews.value.find((v) => v.uuid === requested)
    if (saved) return fromSaved(saved)
    if (workspaceDefault.value) return fromSaved(workspaceDefault.value)
    return fromBuiltin(MY_QUEUE_VIEW)
  })

  const switcherItems = computed<ViewSwitcherItem[]>(() => {
    const items: ViewSwitcherItem[] = []
    for (const v of BUILTIN_VIEWS) {
      items.push({ id: v.id, name: v.name, group: 'Built-in' })
    }
    const groupLabel = {
      workspace: 'Workspace',
      project: 'Project',
      private: 'Private',
    } as const
    for (const scope of ['workspace', 'project', 'private'] as const) {
      const subset = savedViewsRef.value.filter(
        (v) => v.scope === scope && v.archived_at == null,
      )
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

  return { activeView, savedViews, switcherItems, selectViewById }
}
