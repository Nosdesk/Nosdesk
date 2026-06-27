/**
 * `useListView` — the integrated wiring for a server-paginated
 * list view (assets, users, and any future surface that wants
 * the same UX).
 *
 * Pulls together the pieces that every list view needs in
 * lock-step:
 *
 *   useListControls + useListPage          — data layer
 *   useBulkSelection + bulk-table adapter   — selection
 *   useChipFiltersFromControls             — chip filter strip
 *   useListGrouping                         — group-by axis +
 *                                              bucket layout
 *   useDataTableColumns                     — column order +
 *                                              visibility
 *   useSavedListViews                       — server-backed
 *                                              saved views
 *
 * Most of the per-view boilerplate (capture / apply for the
 * standard shape, save-as-modal state, rename / delete error
 * toasts, default-save-name computed) lives here so each view
 * stays focused on its data-shape specifics: facet config,
 * group axes, column registry, cell renderers, bulk actions.
 *
 * Views are NOT forced to use this composable; tickets sticks
 * with its bespoke setup because the sync engine + split-view
 * + saved-view scope merging are tickets-specific. This shell
 * is for the non-ticket private-view surfaces.
 */
import { computed, ref, type ComputedRef, type Ref } from 'vue'
import { useQuery } from '@pinia/colada'
import { useListControls, type Filters } from '@/composables/useListControls'
import {
  useListPage,
  type ListPage,
  type ListPageFetchParams,
  type MobileSearchConfig,
} from '@/composables/useListPage'
import { useBulkSelection } from '@/composables/useBulkSelection'
import { useBulkSelectionForDataTable } from '@/composables/useBulkSelectionForDataTable'
import {
  useChipFiltersFromControls,
  type ChipFacetDef,
} from '@/composables/useChipFiltersFromControls'
import {
  useListGrouping,
  type GroupAxisDef,
  type GroupBucket,
} from '@/composables/useListGrouping'
import {
  useDataTableColumns,
  type DataTableColumnLike,
} from '@/composables/useDataTableColumns'
import { useSavedListViews } from '@/composables/useSavedListViews'
import type { SyncAggregate } from '@nosdesk/core/sync/types'
import type { ListKeys } from '@nosdesk/core/queries/listKeys'
import type { SavedView, SavedViewDataset } from '@/services/savedViewsService'
import { useToastStore } from '@nosdesk/core/stores/toast'

/** Standard fields the shell round-trips through saved views.
 *  Consumers that need extras pass `captureExtras` / `applyExtras`. */
export interface BaseListViewShape {
  groupBy: string
  sortField: string
  sortDirection: 'asc' | 'desc'
  /** Column order + hidden ids + per-column px width overrides.
   *  Optional so views saved before the column UX landed still
   *  load cleanly; `widths` is optional for the same reason
   *  against views saved before resize landed. */
  columns?: {
    order: string[]
    hidden: string[]
    widths?: Record<string, number>
  }
}

export type ListViewDataset = Exclude<SavedViewDataset, 'tickets'>

export interface UseListViewOptions<T extends object, C extends DataTableColumnLike, S extends BaseListViewShape = BaseListViewShape> {
  // -- Identity / persistence ---------------------------------------
  dataset: ListViewDataset
  /** Reactive accessor for the signed-in user's uuid. Saved
   *  views are private + scoped to this uuid. */
  userUuid: Ref<string | null>
  /** Translate function. Pass `fluent.$t` from the consumer. */
  t: (key: string, args?: Record<string, string | number>) => string

  // -- Controls + data layer ----------------------------------------
  itemIdField?: string
  itemId?: (item: T) => string
  defaultSortField: string
  defaultSortDirection?: 'asc' | 'desc'
  defaultPageSize?: number
  pageKeys: ListKeys<string>
  fetchPage: (params: ListPageFetchParams) => Promise<ListPage<T>>
  syncAggregates?: readonly SyncAggregate[]
  mobileSearch?: MobileSearchConfig
  urlSyncParamKeys?: readonly string[]
  /** Scroll container ref from the layout — usually
   *  `computed(() => layoutRef.value?.scrollContainerRef ?? null)`. */
  scrollContainerRef: Ref<HTMLElement | null> | ComputedRef<HTMLElement | null>

  // -- Filters ------------------------------------------------------
  facets: ComputedRef<ChipFacetDef[]>

  // -- Grouping -----------------------------------------------------
  groupAxes: GroupAxisDef<T>[]

  // -- Complete-dataset source (optional) ---------------------------
  /** Some group axes (e.g. fleet-planning lenses) only make sense
   *  over the WHOLE filtered set: their bucket counts and
   *  "select all in a bucket" must be fleet-true, not limited to the
   *  rows scrolled into view. When one of `axes` is the active
   *  group axis, the list sources its rows from `fetch` (the complete
   *  filtered set, one request) instead of the paginated page.
   *  Selection and grouping rebind to that set automatically. */
  completeDataset?: {
    axes: readonly string[]
    fetch: (params: ListPageFetchParams) => Promise<T[]>
    /** Cache key for the complete-dataset query, keyed by the
     *  controls' filter cache-key part so it refetches on filter
     *  change and dedupes across remounts. */
    keyFor: (cacheKeyPart: string) => (string | number)[]
  }

  // -- Columns ------------------------------------------------------
  columns: ComputedRef<readonly C[]>
  pinnedColumnIds?: readonly string[]

  // -- Saved-view shape extension (optional) ------------------------
  /** Snapshot fields beyond the base shape (BaseListViewShape).
   *  Merged into the captured shape on save-as. */
  captureExtras?: () => Omit<S, keyof BaseListViewShape>
  /** Apply extras from a restored shape. Called after the base
   *  fields have been applied. */
  applyExtras?: (shape: S) => void

  // -- Error copy keys ----------------------------------------------
  /** Toast copy key for save-as success. Receives `{ name }`. */
  saveAsSuccessKey?: string
}

export interface UseListView<T extends object, C extends DataTableColumnLike, S extends BaseListViewShape = BaseListViewShape> {
  controls: ReturnType<typeof useListControls<T>>
  page: ReturnType<typeof useListPage<T, string>>
  selection: ReturnType<typeof useBulkSelection<T>>
  dt: ReturnType<typeof useBulkSelectionForDataTable<T>>
  chipFilters: ReturnType<typeof useChipFiltersFromControls>
  grouping: ReturnType<typeof useListGrouping<T>>
  tableColumns: ReturnType<typeof useDataTableColumns<C>>
  savedViews: ReturnType<typeof useSavedListViews<S, Filters>>
  buckets: ComputedRef<GroupBucket<T>[]>
  /** Rows currently displayed: the complete dataset when a planning
   *  axis is active, otherwise the paginated page. */
  effectiveItems: ComputedRef<T[]>
  /** True while a complete-dataset (planning-lens) source is active. */
  completeActive: ComputedRef<boolean>
  /** True while the complete-dataset query is loading. */
  completeLoading: ComputedRef<boolean>
  // Save-view modal state + handlers
  showSaveModal: Ref<boolean>
  editingView: Ref<SavedView<S, Filters> | null>
  defaultSaveName: ComputedRef<string>
  openEditor: (uuid: string) => void
  closeEditor: () => void
  closeSaveModal: () => void
  handleSaveAs: (name: string) => Promise<boolean>
  handleRename: (uuid: string, name: string) => Promise<boolean>
  handleDelete: (uuid: string) => Promise<boolean>
}

export function useListView<
  T extends object,
  C extends DataTableColumnLike,
  S extends BaseListViewShape = BaseListViewShape,
>(options: UseListViewOptions<T, C, S>): UseListView<T, C, S> {
  const {
    dataset,
    userUuid,
    t,
    itemIdField,
    itemId,
    defaultSortField,
    defaultSortDirection = 'asc',
    defaultPageSize = 0,
    pageKeys,
    fetchPage,
    syncAggregates,
    mobileSearch,
    urlSyncParamKeys,
    scrollContainerRef,
    facets,
    groupAxes,
    completeDataset,
    columns,
    pinnedColumnIds,
    captureExtras,
    applyExtras,
    saveAsSuccessKey,
  } = options

  const toast = useToastStore()

  // ---- Controls + data layer ------------------------------------
  const controls = useListControls<T>({
    itemIdField,
    defaultSortField,
    defaultSortDirection,
    defaultPageSize,
  })

  const page = useListPage<T, string>({
    controls,
    keys: pageKeys,
    fetchPage,
    scrollContainerRef,
    syncAggregates,
    mobileSearch,
    urlSync: urlSyncParamKeys ? { paramKeys: urlSyncParamKeys } : undefined,
  })

  // ---- Grouping -------------------------------------------------
  const grouping = useListGrouping<T>({
    axes: groupAxes,
    storageNamespace: dataset,
    // Per-view scope id will become activeViewId once saved
    // views land their own per-view layout namespace. For now
    // every saved view shares 'default' because the saved-view
    // apply path already restores the layout from the shape.
    getViewId: () => 'default',
    t,
  })

  // ---- Complete-dataset source ----------------------------------
  // Active when the current group axis is one that needs the whole
  // filtered set (planning lenses). The query only runs while active,
  // so the normal paginated path pays nothing for this.
  const completeActive = computed(
    () => !!completeDataset && completeDataset.axes.includes(grouping.groupBy.value),
  )
  const completeQuery = completeDataset
    ? useQuery<T[]>({
        key: () => completeDataset.keyFor(controls.cacheKeyPart.value),
        query: () => completeDataset.fetch(controls.requestParams.value),
        enabled: () => completeActive.value,
      })
    : null
  const completeItems = computed<T[]>(() => completeQuery?.data.value ?? [])
  const completeLoading = computed(
    () => completeActive.value && completeQuery?.asyncStatus.value === 'loading',
  )

  /** Rows the selection + grouping operate on: the complete set when
   *  a planning axis is active, otherwise the paginated page. */
  const effectiveItems = computed<T[]>(() =>
    completeActive.value ? completeItems.value : page.items.value,
  )

  // ---- Selection ------------------------------------------------
  const selection = useBulkSelection<T>({
    items: effectiveItems,
    // Fold the source mode into the cache key so switching into or
    // out of a planning lens clears the selection (the "selected
    // matching this query" set stops being meaningful).
    cacheKey: computed(
      () => `${controls.cacheKeyPart.value}|${completeActive.value ? 'complete' : 'page'}`,
    ),
    totalCount: computed(() =>
      completeActive.value ? effectiveItems.value.length : page.totalItems.value,
    ),
    itemId,
  })
  const dt = useBulkSelectionForDataTable(selection)

  // ---- Chip filters ---------------------------------------------
  const chipFilters = useChipFiltersFromControls({
    controls,
    facets,
    t,
  })

  // ---- Columns --------------------------------------------------
  const tableColumns = useDataTableColumns({
    columns,
    storageNamespace: dataset,
    getViewId: () => 'default',
    pinnedIds: pinnedColumnIds,
  })

  // ---- Derived: buckets ----------------------------------------
  const buckets = grouping.buckets(effectiveItems)

  // ---- Saved views ---------------------------------------------
  const savedViews = useSavedListViews<S, Filters>({
    dataset,
    userUuid,
    captureShape: () => {
      const base: BaseListViewShape = {
        groupBy: grouping.groupBy.value,
        sortField: controls.sortField.value,
        sortDirection: controls.sortDirection.value,
        columns: tableColumns.captureLayout(),
      }
      const extras = captureExtras ? captureExtras() : ({} as Omit<S, keyof BaseListViewShape>)
      return { ...base, ...extras } as S
    },
    captureFilter: () => ({ ...controls.filters.value }),
    applyShape: (shape) => {
      grouping.setGroupBy(shape.groupBy)
      controls.handleSortUpdate(shape.sortField, shape.sortDirection)
      if (shape.columns) {
        tableColumns.applyLayout(
          shape.columns.order,
          shape.columns.hidden,
          shape.columns.widths,
        )
      }
      if (applyExtras) applyExtras(shape)
    },
    applyFilter: (filter) => {
      // Replace the whole filter map so axes the saved view
      // didn't touch get cleared. Search query (a separate ref)
      // stays alone so the user can search inside a saved view
      // without retyping.
      controls.filters.value = { ...filter }
    },
    t,
  })

  // ---- Save-as / edit modal state ------------------------------
  const showSaveModal = ref(false)
  // The generic S widens to its constraint shape inside `ref()`,
  // which TypeScript can't narrow back to S without help. The
  // cast is safe because we only ever assign SavedView<S, F>
  // values to this ref.
  const editingView = ref<SavedView<S, Filters> | null>(null) as Ref<
    SavedView<S, Filters> | null
  >


  function openEditor(uuid: string): void {
    editingView.value = savedViews.views.value.find((v) => v.uuid === uuid) ?? null
  }

  function closeEditor(): void {
    editingView.value = null
  }

  function closeSaveModal(): void {
    showSaveModal.value = false
  }

  async function handleSaveAs(name: string): Promise<boolean> {
    const created = await savedViews.saveAs(name)
    if (!created) {
      toast.error(t('views-save-as-error'))
      return false
    }
    const successKey = saveAsSuccessKey ?? 'views-save-as-success'
    toast.success(t(successKey, { name: created.name }))
    return true
  }

  async function handleRename(uuid: string, name: string): Promise<boolean> {
    const ok = await savedViews.rename(uuid, name)
    if (!ok) toast.error(t('views-saved-editor-rename-error'))
    return ok
  }

  async function handleDelete(uuid: string): Promise<boolean> {
    const ok = await savedViews.deleteView(uuid)
    if (!ok) toast.error(t('views-saved-editor-delete-error'))
    return ok
  }

  /** Default name pre-filled in the SaveViewModal: when the
   *  user is currently viewing a saved view, suggest "<name>
   *  (copy)"; otherwise an empty string. */
  const defaultSaveName = computed<string>(() => {
    const active = savedViews.activeView.value
    if (!active) return ''
    return `${active.name} ${t('views-save-default-suffix')}`
  })

  return {
    controls,
    page,
    selection,
    dt,
    chipFilters,
    grouping,
    tableColumns,
    savedViews,
    buckets,
    effectiveItems,
    completeActive,
    completeLoading,
    showSaveModal,
    editingView,
    defaultSaveName,
    openEditor,
    closeEditor,
    closeSaveModal,
    handleSaveAs,
    handleRename,
    handleDelete,
  }
}
