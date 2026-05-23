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
import type { ListKeys } from '@/queries/listKeys'
import type { SavedView, SavedViewDataset } from '@/services/savedViewsService'
import { useToastStore } from '@/stores/toast'

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
  sseEvents?: readonly string[]
  mobileSearch?: MobileSearchConfig
  urlSyncParamKeys?: readonly string[]
  /** Scroll container ref from the layout — usually
   *  `computed(() => layoutRef.value?.scrollContainerRef ?? null)`. */
  scrollContainerRef: Ref<HTMLElement | null> | ComputedRef<HTMLElement | null>

  // -- Filters ------------------------------------------------------
  facets: ComputedRef<ChipFacetDef[]>

  // -- Grouping -----------------------------------------------------
  groupAxes: GroupAxisDef<T>[]

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
    sseEvents,
    mobileSearch,
    urlSyncParamKeys,
    scrollContainerRef,
    facets,
    groupAxes,
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
    sseEvents,
    mobileSearch,
    urlSync: urlSyncParamKeys ? { paramKeys: urlSyncParamKeys } : undefined,
  })

  // ---- Selection ------------------------------------------------
  const selection = useBulkSelection<T>({
    items: page.items,
    cacheKey: controls.cacheKeyPart,
    totalCount: page.totalItems,
    itemId,
  })
  const dt = useBulkSelectionForDataTable(selection)

  // ---- Chip filters ---------------------------------------------
  const chipFilters = useChipFiltersFromControls({
    controls,
    facets,
    t,
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

  // ---- Columns --------------------------------------------------
  const tableColumns = useDataTableColumns({
    columns,
    storageNamespace: dataset,
    getViewId: () => 'default',
    pinnedIds: pinnedColumnIds,
  })

  // ---- Derived: buckets ----------------------------------------
  const itemsRef = computed(() => page.items.value)
  const buckets = grouping.buckets(itemsRef)

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
