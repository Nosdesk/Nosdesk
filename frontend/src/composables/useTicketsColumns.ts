/**
 * Column visibility + layout for the tickets table.
 *
 * Owns the three-tier persistence precedence:
 *   1. localStorage override for the active view (user toggled
 *      / dragged in this browser)
 *   2. The view's canonical shape.visible_card_fields + columns
 *      (saved-view authoritative layout)
 *   3. DEFAULT_VISIBLE_COLUMNS factory default
 *
 * Composes `useColumnLayout` for the resize / reorder handlers +
 * per-view widths localStorage. The view binds its DnD handlers
 * directly to `layout.*` so the keystroke surface stays narrow.
 *
 * `saveLayoutToView` promotes the local choice into the saved
 * view's shape (visible_card_fields + columns) and clears the
 * local override so the canonical layout takes over.
 */
import { computed, ref, watch, type ComputedRef, type Ref } from 'vue'
import { useSavedViewsStore } from '@/stores/savedViews'
import {
  TICKET_COLUMNS,
  DEFAULT_VISIBLE_COLUMNS,
  type ColumnId,
  type ListColumn,
} from '@/sync/views/ticketColumns'
import { useColumnLayout } from '@/composables/useColumnLayout'
import type { ResolvedView } from '@/composables/useTicketsViewResolution'
import type { CardData, ListViewShape } from '@/sync/views/types'

const STORAGE_PREFIX = 'tickets-columns:'

function storageKeyFor(viewId: string): string {
  return `${STORAGE_PREFIX}${viewId}`
}

function loadColumns(viewId: string): ColumnId[] | null {
  if (typeof localStorage === 'undefined') return null
  const raw = localStorage.getItem(storageKeyFor(viewId))
  if (!raw) return null
  try {
    const parsed = JSON.parse(raw)
    if (!Array.isArray(parsed)) return null
    const valid: ColumnId[] = []
    for (const item of parsed) {
      if (TICKET_COLUMNS.some((c) => c.id === item)) valid.push(item as ColumnId)
    }
    return valid.length ? valid : null
  } catch {
    return null
  }
}

function persistColumns(viewId: string, ids: ColumnId[]): void {
  if (typeof localStorage === 'undefined') return
  localStorage.setItem(storageKeyFor(viewId), JSON.stringify(ids))
}

function clearColumns(viewId: string): void {
  if (typeof localStorage === 'undefined') return
  localStorage.removeItem(storageKeyFor(viewId))
}

/** Map a CardData field name onto a ColumnId. The saved-view
 * spec stores `visible_card_fields` in field-space for forward
 * compat; we render in column-space. */
function mapFieldToColumnId(field: string): ColumnId | null {
  if (field === 'workflow_state') return 'workflow_state'
  if (field === 'priority') return 'priority'
  if (field === 'assignee_uuid') return 'assignee'
  if (field === 'requester_uuid') return 'requester'
  if (field === 'category_id') return 'category'
  if (field === 'cycle_id') return 'cycle'
  if (field === 'due_date') return 'due_date'
  if (field === 'last_activity_at') return 'last_activity'
  if (field === 'created_at') return 'created_at'
  if (field === 'sla') return 'sla'
  if (field === 'kb_gap_signal') return 'kb_gap'
  if (field === 'affected_devices') return 'devices'
  if (field === 'recurrence_rule') return 'recurrence'
  if (field === 'id' || field === 'title') return field as ColumnId
  return null
}

function columnIdToField(id: ColumnId): string | null {
  switch (id) {
    case 'workflow_state': return 'workflow_state'
    case 'priority': return 'priority'
    case 'assignee': return 'assignee_uuid'
    case 'requester': return 'requester_uuid'
    case 'category': return 'category_id'
    case 'cycle': return 'cycle_id'
    case 'due_date': return 'due_date'
    case 'last_activity': return 'last_activity_at'
    case 'created_at': return 'created_at'
    case 'sla': return 'sla'
    case 'kb_gap': return 'kb_gap_signal'
    case 'devices': return 'affected_devices'
    case 'recurrence': return 'recurrence_rule'
    case 'id': return 'id'
    case 'title': return 'title'
    default: return null
  }
}

function viewCanonicalWidths(view: ResolvedView): Map<ColumnId, number> {
  const map = new Map<ColumnId, number>()
  const columns = (view.shape as ListViewShape).columns
  if (!columns) return map
  for (const c of columns) {
    if (typeof c.width !== 'number') continue
    const id = mapFieldToColumnId(String(c.field))
    if (id) map.set(id, c.width)
  }
  return map
}

export interface UseTicketsColumns {
  visibleColumnIds: ComputedRef<ColumnId[]>
  visibleColumns: ComputedRef<ListColumn[]>
  layoutDirty: ComputedRef<boolean>
  canSaveLayoutToView: ComputedRef<boolean>
  layout: ReturnType<typeof useColumnLayout>
  toggleColumn: (id: ColumnId) => void
  resetColumns: () => void
  saveLayoutToView: () => Promise<void>
  /** Inline style helper for `<th>` / `<td>`. The title column
   * flexes; everything else takes its pixel width from the
   * layout composable so `table-layout: fixed` is authoritative. */
  colStyle: (col: ListColumn) => Record<string, string>
}

export function useTicketsColumns(activeView: ComputedRef<ResolvedView>): UseTicketsColumns {
  const savedViewsStore = useSavedViewsStore()

  const localOverride: Ref<ColumnId[] | null> = ref(loadColumns(activeView.value.id))

  const layout = useColumnLayout(
    () => activeView.value.id,
    (next) => {
      localOverride.value = next
      persistColumns(activeView.value.id, next)
    },
    () => visibleColumnIds.value,
  )

  layout.loadFor(activeView.value.id, viewCanonicalWidths(activeView.value))

  watch(activeView, (next) => {
    localOverride.value = loadColumns(next.id)
    layout.loadFor(next.id, viewCanonicalWidths(next))
  })

  const viewCanonicalColumns = computed<ColumnId[]>(() => {
    const fields = (activeView.value.shape as ListViewShape).visible_card_fields
    if (!fields || fields.length === 0) return [...DEFAULT_VISIBLE_COLUMNS]
    const out: ColumnId[] = []
    if (!fields.some((f) => String(f) === 'title')) out.push('title')
    for (const f of fields) {
      const id = mapFieldToColumnId(String(f))
      if (id && !out.includes(id)) out.push(id)
    }
    return out.length ? out : [...DEFAULT_VISIBLE_COLUMNS]
  })

  const visibleColumnIds = computed<ColumnId[]>(
    () => localOverride.value ?? viewCanonicalColumns.value,
  )

  const visibleColumns = computed<ListColumn[]>(() =>
    visibleColumnIds.value
      .map((id) => TICKET_COLUMNS.find((c) => c.id === id))
      .filter((c): c is ListColumn => Boolean(c)),
  )

  const layoutDirty = computed<boolean>(() => {
    if (!localOverride.value) return false
    const canonical = viewCanonicalColumns.value
    if (localOverride.value.length !== canonical.length) return true
    return localOverride.value.some((id, i) => id !== canonical[i])
  })

  const canSaveLayoutToView = computed<boolean>(
    () => activeView.value.source === 'saved' && !!activeView.value.uuid,
  )

  function toggleColumn(id: ColumnId): void {
    if (id === 'title') return
    const current = visibleColumnIds.value
    const next = current.includes(id)
      ? current.filter((c) => c !== id)
      : [...current, id]
    if (!next.includes('title')) next.unshift('title')
    localOverride.value = next
    persistColumns(activeView.value.id, next)
  }

  function resetColumns(): void {
    localOverride.value = null
    clearColumns(activeView.value.id)
    layout.clearWidths()
  }

  async function saveLayoutToView(): Promise<void> {
    if (!canSaveLayoutToView.value || !activeView.value.uuid) return
    const ids = visibleColumnIds.value
    const fields = ids.map(columnIdToField).filter((f): f is string => !!f)
    const columnsConfig = ids.map((id) => {
      const field = columnIdToField(id)
      const widthOverride = layout.widthOverrides.value.get(id)
      const col = TICKET_COLUMNS.find((c) => c.id === id)
      return {
        field: field as keyof CardData,
        width: widthOverride ?? col?.defaultWidthPx,
        sortable: !!col?.sortKey,
      }
    })
    const shape = {
      ...(activeView.value.shape as ListViewShape),
      visible_card_fields: fields as (keyof CardData)[],
      columns: columnsConfig as ListViewShape['columns'],
    }
    await savedViewsStore.update(activeView.value.uuid, { shape })
    resetColumns()
  }

  function colStyle(col: ListColumn): Record<string, string> {
    if (col.flex) {
      // Title (the only flex column today) absorbs leftover width
      // and clamps at a hard 280px floor — below that ticket
      // titles stop being readable. The earlier 160px floor was
      // the original bug-in-miniature: title was the only column
      // that *could* shrink, and the floor was too low to stop
      // it. The 60ch max-cap is dropped — there's no good reason
      // to throttle title width on wide monitors.
      //
      // The truncation lives on an inner span that needs its
      // own `min-width: 0` (see TicketsTable.vue's title cell)
      // because flex / table cells with a min-width otherwise
      // refuse to ellipsis their text content.
      return { width: 'auto', 'min-width': '280px' }
    }
    const w = layout.widthFor(col)
    return { width: `${w}px`, 'min-width': `${w}px`, 'max-width': `${w}px` }
  }

  return {
    visibleColumnIds,
    visibleColumns,
    layoutDirty,
    canSaveLayoutToView,
    layout,
    toggleColumn,
    resetColumns,
    saveLayoutToView,
    colStyle,
  }
}
