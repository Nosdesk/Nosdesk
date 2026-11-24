<script setup lang="ts">
/**
 * The asset page's Timeline card: everything that happened to the
 * equipment, newest first.
 *
 * One grid for the whole card (`.timeline-grid`, at the foot of this
 * file): the date at the left edge, where the title also starts; a narrow
 * gutter for the lifecycle state the asset moved to; the text; figures and
 * actions at the right edge. The summary, the year labels, every entry and
 * "Show older" sit on those lines at every width, so nothing is
 * repositioned by a breakpoint.
 *
 * The header holds the title, one filter, Add record and a menu for the
 * rest. Reads `GET /assets/{id}/timeline` a page at a time, keeps open
 * records at the top, and owns the record form and the record view. A
 * `?record=<id>` in the address opens that record.
 */
import { computed, nextTick, reactive, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import Modal from '@/components/Modal.vue'
import Button from '@/components/common/Button.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import Icon from '@/components/common/Icon.vue'
import MenuList, { type MenuItem } from '@/components/common/MenuList.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import { useToastStore } from '@nosdesk/core/stores/toast'
import { getDateConfig } from '@nosdesk/core/utils/dateUtils'
import { formatMoney } from '@nosdesk/core/utils/money'
import type { AssetTimelineFilter } from '@nosdesk/core/services/assetTimelineService'
import { assetRecordService } from '@nosdesk/core/services/assetRecordService'
import { assetMediaService } from '@nosdesk/core/services/assetMediaService'
import { assetStatusLabel } from '@/utils/assetStatusMeta'
import { extractErrorMessage } from '@/utils/errors'
import { downloadAssetRecordCard, updateAsset } from '@/services/assetService'
import { useAssetLoans } from '@/composables/useAssetLoans'
import {
  useAssetOpenRecords,
  useAssetRecordSummary,
  useAssetRecordTypes,
  useAssetTimelinePages,
  useAssetTimelineSource,
  useAssetTimelineSync,
} from '@/composables/useAssetTimeline'
import type {
  Asset,
  AssetRecord,
  AssetRecordTotals,
  AssetRecordType,
  AssetRecordWrite,
  AssetTimelineEntry,
  AssetTimelineTicket,
} from '@nosdesk/core/types/asset'
import AssetRecordDetail from './AssetRecordDetail.vue'
import AssetRecordFormModal from './AssetRecordFormModal.vue'
import AssetTimelineEntryRow from './AssetTimelineEntry.vue'
import { formatDay, typeName } from './assetTimeline'

const props = defineProps<{
  asset: Asset
  /** Staff add, edit and complete records; anyone else only reads. */
  canEdit: boolean
  /** The signed-in person, who may delete a record they made today. */
  me: string | null
  /** A workspace admin may delete any record. */
  isAdmin?: boolean
}>()

/** The asset itself changed from here (its serial, or its status by a
 *  carry-over); our own writes don't come back through sync. */
const emit = defineEmits<{ 'asset-changed': [] }>()

/** Entries a page of the timeline holds. */
const PAGE_SIZE = 100

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const toasts = useToastStore()
const route = useRoute()
const router = useRouter()
const assetId = () => props.asset.id

// --- Filter -----------------------------------------------------------

type Scope = 'all' | 'record' | 'status' | 'loan' | 'ticket'
const scope = ref<Scope>('all')
const recordType = ref('')
const withPredecessors = ref(false)

const recordTypes = useAssetRecordTypes(() => props.canEdit)
const types = recordTypes.types
const typeOf = (slug: string | null | undefined) => types.value.find((x) => x.slug === slug)

const filter = computed<AssetTimelineFilter>(() => {
  const out: AssetTimelineFilter = { limit: PAGE_SIZE }
  if (scope.value === 'record') out.sources = ['record', 'replacement']
  else if (scope.value !== 'all') out.sources = [scope.value]
  if (recordType.value) out.recordType = recordType.value
  if (withPredecessors.value) out.includePredecessors = true
  return out
})

const SCOPE_LABEL: Record<Scope, string> = {
  all: 'asset-timeline-filter-all',
  record: 'asset-timeline-filter-records',
  status: 'asset-timeline-filter-status',
  loan: 'asset-timeline-filter-loans',
  ticket: 'asset-timeline-filter-tickets',
}

/** What the filter button says: the one thing being shown. */
const filterLabel = computed(() =>
  recordType.value ? typeName(t, typeOf(recordType.value), recordType.value) : t(SCOPE_LABEL[scope.value]),
)

// --- Data -------------------------------------------------------------

const timeline = useAssetTimelinePages(assetId, filter)
const entries = timeline.entries
const loaded = timeline.loaded
const hasMore = timeline.hasMore
const loadingMore = timeline.loadingMore
const loadMore = timeline.loadMore
// Records on the assets this one replaced, shown with "include history",
// and records naming this asset as their replacement, refresh it too.
const refreshAll = useAssetTimelineSync(assetId, () =>
  withPredecessors.value ? predecessors.value.map((p) => p.id) : [],
)

const openRecords = useAssetOpenRecords(assetId, () => props.canEdit)
const summaryAll = useAssetRecordSummary(assetId, '', () => props.canEdit)
const summaryShown = useAssetRecordSummary(assetId, recordType, () => props.canEdit)
const { loans } = useAssetLoans(assetId)
/** Tickets linked to the asset: said on the records that name them, and
 *  offered in the record form. */
const linkedTickets = useAssetTimelineSource(assetId, 'ticket', 200, () => props.canEdit)
const tickets = computed<AssetTimelineTicket[]>(() =>
  linkedTickets.value.flatMap((e) => (e.source === 'ticket' ? [e.data] : [])),
)
/** Replacements either way, known up front so a predecessor's history can
 *  be offered before the page holding its "Replaces" entry is loaded. */
const replacements = useAssetTimelineSource(assetId, 'replacement', 20, () => props.canEdit)
const predecessors = computed(() =>
  replacements.value.flatMap((e) =>
    e.source === 'replacement' && e.kind === 'replaces' && e.data.other_asset ? [e.data.other_asset] : [],
  ),
)

/** Refetch the card, then the page in view. */
async function reload() {
  await refreshAll()
  await timeline.query.refresh()
}

/** The types this asset has records of. */
const usedTypes = computed(
  () => summaryAll.value?.by_type.flatMap((b) => (b.record_type ? [b.record_type] : [])) ?? [],
)

const filterItems = computed<MenuItem[]>(() => {
  const pick = (value: Scope, divider = false): MenuItem => ({
    id: value,
    label: t(SCOPE_LABEL[value]),
    checked: scope.value === value && !recordType.value,
    divider,
  })
  if (!props.canEdit) return [pick('all'), pick('status', true), pick('loan'), pick('ticket')]
  return [
    pick('all'),
    pick('record', true),
    ...(usedTypes.value.length > 1
      ? usedTypes.value.map((slug) => ({
          id: `type:${slug}`,
          label: typeName(t, typeOf(slug), slug),
          checked: recordType.value === slug,
        }))
      : []),
    pick('status', true),
    pick('loan'),
    pick('ticket'),
  ]
})

function setFilter(id: string) {
  if (id.startsWith('type:')) {
    scope.value = 'record'
    recordType.value = id.slice(5)
  } else {
    scope.value = id as Scope
    recordType.value = ''
  }
  filterOpen.value = false
}

const filterOpen = ref(false)
const filterTrigger = ref<HTMLElement | null>(null)
const filterAnchor = computed(() => ({ type: 'element' as const, element: () => filterTrigger.value }))

// --- The menu for the rest ----------------------------------------------

const moreOpen = ref(false)
const moreTrigger = ref<HTMLElement | null>(null)
const moreAnchor = computed(() => ({ type: 'element' as const, element: () => moreTrigger.value }))

const moreItems = computed<MenuItem[]>(() => [
  ...predecessors.value.map((p) => ({
    id: 'predecessors',
    label: t('asset-timeline-include-predecessor', { name: p.asset_tag ?? p.name }),
    checked: withPredecessors.value,
  })),
  {
    id: 'record-card',
    label: t('asset-detail-record-card'),
    icon: 'download' as const,
    divider: predecessors.value.length > 0,
  },
])

async function onMore(id: string) {
  moreOpen.value = false
  if (id === 'predecessors') withPredecessors.value = !withPredecessors.value
  if (id === 'record-card') {
    try {
      await downloadAssetRecordCard(props.asset.id)
    } catch (e) {
      toasts.error(t('asset-detail-record-card'), extractErrorMessage(e, t('asset-record-form-error')))
    }
  }
}

// --- Rows -------------------------------------------------------------

const showsRecords = computed(() => scope.value === 'all' || scope.value === 'record')

const openEntries = computed<AssetTimelineEntry[]>(() =>
  showsRecords.value
    ? openRecords.value
        .filter((r) => !recordType.value || r.record_type === recordType.value)
        .map((r) => ({
          key: `record:${r.id}`,
          source: 'record' as const,
          asset_id: r.asset_id,
          day: r.occurred_on,
          at: r.occurred_at,
          recorded_at: r.recorded_at,
          recorded_by: r.recorded_by,
          actor_uuid: r.performed_by_uuid ?? r.recorded_by,
          data: r,
        }))
    : [],
)

/** This asset's records in view, open ones included. */
const recordsInView = computed(() => [
  ...openRecords.value,
  ...entries.value.flatMap((e) => (e.source === 'record' && e.asset_id === props.asset.id ? [e.data] : [])),
])
const recordIdsInView = computed(() => new Set(recordsInView.value.map((r) => r.id)))

// With everything shown, a ticket, a status change or a replacement that a
// record in view already points at is said on that record instead of
// taking a row of its own. Filtered to tickets or status, they are rows.
const foldedTickets = computed(
  () => new Set(scope.value === 'all' ? recordsInView.value.flatMap((r) => (r.ticket_id ? [r.ticket_id] : [])) : []),
)
function ticketOf(record: AssetRecord): AssetTimelineTicket | null {
  return record.ticket_id ? (tickets.value.find((x) => x.id === record.ticket_id) ?? null) : null
}

/** Within two minutes: the loan and the status change it made. */
function together(a: string | null, b: string | null): boolean {
  return a !== null && b !== null && Math.abs(Date.parse(a) - Date.parse(b)) < 120_000
}

/**
 * Where each move between lifecycle states is told. A status change is a
 * row of its own unless another entry in view already tells of it: the
 * record a transition opened, the loan that put the asset On loan or took
 * it back, the "Added" marker for the state it arrived in. That entry then
 * carries the state as its mark, and the status row is left out.
 */
const moves = computed(() => {
  const marks = new Map<string, string>()
  const folded = new Set<string>()
  /** The arrival marker of an imported asset, and the state it came in. */
  const imported = new Map<string, string>()
  const own = entries.value.filter((e) => e.asset_id === props.asset.id)
  for (const e of entries.value) {
    if (e.source !== 'status') continue
    marks.set(e.key, e.data.to_status)
    if (scope.value !== 'all' || e.asset_id !== props.asset.id) continue
    const opened = e.data.record_ids ?? []
    const record = recordsInView.value.find((r) => r.lifecycle_event_id === e.data.id || opened.includes(r.id))
    const loan = own.find(
      (l) =>
        l.source === 'loan' &&
        together(l.at, e.at) &&
        (l.kind === 'lent' ? e.data.to_status === 'on_loan' : e.data.from_status === 'on_loan'),
    )
    const arrival = !e.data.from_status ? own.find((m) => m.source === 'marker' && m.kind === 'added') : undefined
    const teller = record ? `record:${record.id}` : (loan?.key ?? arrival?.key)
    if (teller) {
      marks.set(teller, e.data.to_status)
      folded.add(e.key)
      if (arrival && teller === arrival.key && typeof e.data.metadata?.import_job_id === 'string') {
        imported.set(teller, e.data.to_status)
      }
    }
  }
  // A loan puts the asset On loan by definition, whether or not the page
  // holding its status change is loaded.
  for (const e of entries.value) {
    if (e.source === 'loan' && e.kind === 'lent' && !marks.has(e.key)) marks.set(e.key, 'on_loan')
  }
  return { marks, folded, imported }
})

/** The asset that replaced this one, on the record that says so. */
function replacedByOf(record: AssetRecord) {
  if (!record.replacement_asset_id || record.asset_id !== props.asset.id) return null
  const found = [...replacements.value, ...entries.value].find(
    (e) => e.source === 'replacement' && e.kind === 'replaced_by' && e.data.record_id === record.id,
  )
  return found?.source === 'replacement' ? found.data.other_asset : null
}

interface Row {
  entry: AssetTimelineEntry
  type?: AssetRecordType
  predecessorTag: string | null
}
interface Group {
  year: string
  totals: string | null
  rows: Row[]
}

function rowOf(entry: AssetTimelineEntry): Row {
  const slug = entry.source === 'record' || entry.source === 'replacement' ? entry.data.record_type : null
  return {
    entry,
    type: typeOf(slug),
    predecessorTag:
      entry.asset_id !== props.asset.id
        ? (predecessors.value.find((p) => p.id === entry.asset_id)?.asset_tag ?? `#${entry.asset_id}`)
        : null,
  }
}

function costsText(totals: AssetRecordTotals): string {
  // A code the server couldn't format comes as minor units alone.
  const amounts = totals.costs.map((c) =>
    c.amount !== null ? formatMoney(c.amount, c.currency) : `${c.amount_minor} ${c.currency}`,
  )
  return new Intl.ListFormat(getDateConfig().defaultLocale, { style: 'long', type: 'conjunction' }).format(amounts)
}

/** A year's records, as its label's right-hand note: "2 records, $38.50". */
function yearTotals(totals: AssetRecordTotals | undefined): string | null {
  if (!totals || totals.count === 0) return null
  const records = t('asset-timeline-records-count', { count: totals.count })
  return totals.costs.length ? t('asset-timeline-year-totals', { records, costs: costsText(totals) }) : records
}

const groups = computed<Group[]>(() => {
  const out: Group[] = []
  for (const entry of entries.value) {
    if (entry.source === 'record' && entry.data.completed_on === null && entry.asset_id === props.asset.id) continue
    if (entry.source === 'replacement' && entry.kind === 'replaced_by' && recordIdsInView.value.has(entry.data.record_id)) continue
    if (entry.source === 'ticket' && foldedTickets.value.has(entry.data.id)) continue
    if (moves.value.folded.has(entry.key)) continue
    const year = entry.day.slice(0, 4)
    let group = out[out.length - 1]
    if (!group || group.year !== year) {
      group = {
        year,
        totals: showsRecords.value && !withPredecessors.value ? yearTotals(summaryShown.value?.by_year[year]) : null,
        rows: [],
      }
      out.push(group)
    }
    group.rows.push(rowOf(entry))
  }
  return out
})

/** Nothing recorded yet: the card invites the first record. */
const inviteFirst = computed(
  () => props.canEdit && loaded.value && summaryAll.value !== null && summaryAll.value.count === 0 && scope.value === 'all',
)

/** What the records add up to, as one sentence. */
const sentence = computed(() => {
  const s = summaryShown.value
  if (!s || s.count === 0 || !showsRecords.value) return null
  const records = t('asset-timeline-records-count', { count: s.count })
  const args = { records, costs: costsText(s), date: s.last_on ? formatDay(s.last_on) : '' }
  if (s.costs.length && s.last_on) return t('asset-timeline-sentence-cost-last', args)
  if (s.costs.length) return t('asset-timeline-sentence-cost', args)
  if (s.last_on) return t('asset-timeline-sentence-last', args)
  return t('asset-timeline-sentence', args)
})

// --- Record view and form ---------------------------------------------

const viewing = ref<AssetRecord | null>(null)
const form = reactive({
  show: false,
  record: null as AssetRecord | null,
  completing: false,
})
const confirmDelete = ref<AssetRecord | null>(null)
const highlightKey = ref<string | null>(null)
const listEl = ref<HTMLElement | null>(null)

async function findRecord(id: number): Promise<AssetRecord | null> {
  const local =
    openRecords.value.find((r) => r.id === id) ??
    entries.value.flatMap((e) => (e.source === 'record' ? [e.data] : [])).find((r) => r.id === id)
  if (local) return local
  try {
    // A record on a predecessor is read from that asset.
    const owner = entries.value.find((e) => e.source === 'replacement' && e.data.record_id === id)
    const ownerId =
      owner?.source === 'replacement' && owner.kind === 'replaces'
        ? (owner.data.other_asset?.id ?? props.asset.id)
        : props.asset.id
    return await assetRecordService.get(ownerId, id)
  } catch {
    return null
  }
}

async function openRecord(id: number) {
  viewing.value = await findRecord(id)
}

/** Close the record view, and drop a `?record=` that opened it. */
function closeRecord() {
  viewing.value = null
  if (route.query.record !== undefined) {
    const { record: _record, ...rest } = route.query
    void router.replace({ query: rest })
  }
}

// A link to a record (from the record types admin, say) opens it.
watch(
  () => [route.query.record, props.canEdit] as const,
  ([raw, staff]) => {
    const id = Number(Array.isArray(raw) ? raw[0] : raw)
    if (staff && Number.isInteger(id) && id > 0 && viewing.value?.id !== id) void openRecord(id)
  },
  { immediate: true },
)

function openAdd() {
  form.record = null
  form.completing = false
  form.show = true
}

async function openEdit(id: number, completing = false) {
  // Read fresh, so an edit starts from the current version.
  const record = await assetRecordService.get(props.asset.id, id).catch(() => findRecord(id))
  if (!record) return
  closeRecord()
  form.record = record
  form.completing = completing
  form.show = true
}

function canDelete(record: AssetRecord): boolean {
  if (!props.canEdit || record.asset_id !== props.asset.id) return false
  if (props.isAdmin) return true
  return (
    record.recorded_by !== null &&
    record.recorded_by === props.me &&
    Date.now() - Date.parse(record.recorded_at) < 24 * 60 * 60 * 1000
  )
}

async function deleteRecord() {
  const record = confirmDelete.value
  if (!record) return
  try {
    await assetRecordService.delete(props.asset.id, record.id)
  } catch (e) {
    toasts.error(t('asset-record-delete-failed'), extractErrorMessage(e, t('asset-record-form-error')))
    return
  }
  confirmDelete.value = null
  closeRecord()
  await reload()
  toasts.success(t('asset-record-deleted'))
}

/** Bring a saved record into view: load older pages until it is listed,
 *  then scroll to it and mark it for a moment. A 2023 repair lands under
 *  2023, which may be a page or two down. */
async function reveal(record: AssetRecord) {
  const key = `record:${record.id}`
  if (record.completed_on !== null) {
    let guard = 0
    while (!entries.value.some((e) => e.key === key) && timeline.hasMore.value && guard < 40) {
      await timeline.loadMore()
      guard += 1
    }
  }
  await nextTick()
  highlightKey.value = key
  const row = listEl.value?.querySelector(`[data-entry="${key}"]`)
  row?.scrollIntoView({ block: 'center', behavior: 'smooth' })
  window.setTimeout(() => {
    if (highlightKey.value === key) highlightKey.value = null
  }, 2600)
}

// --- Same-tag exchange ------------------------------------------------

const updatingSerial = ref(false)

/** The record's asset is this one, and its serial is Nosdesk's to change:
 *  a synced asset's serial is the sync's. */
function serialEditable(record: AssetRecord): boolean {
  return props.canEdit && record.asset_id === props.asset.id && !props.asset.external_sync_source
}

/** Make the serial a record received the asset's own, as an edit of the
 *  asset would. */
async function updateSerial(serial: string) {
  updatingSerial.value = true
  try {
    await updateAsset(props.asset.id, { serial_number: serial })
    emit('asset-changed')
    toasts.success(t('asset-record-serial-updated', { serial }))
  } catch (e) {
    toasts.error(t('asset-record-serial-failed'), extractErrorMessage(e, t('asset-record-form-error')))
  } finally {
    updatingSerial.value = false
  }
}

/** The carry-overs ran: this asset may have moved status. */
function onCarried(replacement: Pick<Asset, 'id' | 'name' | 'asset_tag'>) {
  emit('asset-changed')
  toasts.success(t('asset-record-carry-done', { asset: replacement.asset_tag ?? replacement.name }))
}

async function onSaved(write: AssetRecordWrite, files: File[], addAnother: boolean) {
  const wasEdit = !!form.record
  if (!addAnother) form.show = false
  if (files.length > 0) {
    try {
      await assetMediaService.upload(props.asset.id, files, write.record.id)
    } catch (e) {
      toasts.error(t('asset-record-files-failed'), extractErrorMessage(e, t('asset-record-form-error')))
    }
  }
  // A record outside the current filter would save and vanish.
  if (scope.value !== 'all' && scope.value !== 'record') scope.value = 'all'
  if (recordType.value && recordType.value !== write.record.record_type) recordType.value = ''
  await nextTick()
  await reload()

  const name = typeName(t, typeOf(write.record.record_type), write.record.record_type)
  const status = assetStatusLabel(t, props.asset.status)
  const asset = props.asset.asset_tag ?? props.asset.name
  const notes: string[] = [t('asset-record-saved-status', { asset, status })]
  for (const warning of write.warnings) {
    if (warning.code === 'before_purchase') {
      notes.push(t('asset-record-warning-before-purchase', { date: formatDay(warning.purchase_date) }))
    } else if (warning.code === 'completed_long_after') {
      notes.push(t('asset-record-warning-completed-long-after'))
    }
  }
  toasts.success(
    wasEdit
      ? t('asset-record-saved-edit', { type: name })
      : t('asset-record-saved-new', { type: name, date: formatDay(write.record.occurred_on) }),
    notes.join(' '),
  )
  if (!addAnother) void reveal(write.record)
}

const viewingTitle = computed(() => {
  const r = viewing.value
  if (!r) return ''
  return t('asset-record-detail-title', {
    type: typeName(t, typeOf(r.record_type), r.record_type),
    date: formatDay(r.occurred_on),
  })
})

const oldestLoaded = computed(() => {
  const last = entries.value[entries.value.length - 1]
  return last ? formatDay(last.day, 'monthYear') : ''
})

/** Quiet header controls: text and an icon, a 44px hit area on touch. */
const HEADER_BUTTON =
  'relative inline-flex items-center gap-1 h-7 rounded-md text-xs text-secondary hover:text-primary hover:bg-surface-hover transition-colors pointer-coarse:after:absolute pointer-coarse:after:-inset-2'

defineExpose({ openAdd, openRecord, reload })
</script>

<template>
  <!-- A container in rem: when the text is set very large, Add record
       drops to its icon before the title is squeezed. The one place the
       card's structure changes with width. -->
  <div class="@container min-w-0">
    <SectionCard content-padding="">
      <template #title>{{ t('asset-timeline-heading') }}</template>
      <template #headerActions>
        <button
          ref="filterTrigger"
          type="button"
          :class="[HEADER_BUTTON, 'px-2', scope !== 'all' ? 'text-primary font-medium' : '']"
          :aria-label="t('asset-timeline-filter-aria', { value: filterLabel })"
          aria-haspopup="menu"
          :aria-expanded="filterOpen"
          @click="filterOpen = !filterOpen"
        >
          <span class="truncate max-w-[9rem]">{{ filterLabel }}</span>
          <Icon name="chevronDown" size="xs" class="text-tertiary" />
        </button>
        <Button
          v-if="canEdit"
          variant="secondary"
          size="xs"
          icon="add"
          :aria-label="t('asset-timeline-add')"
          class="relative pointer-coarse:!min-h-7 pointer-coarse:after:absolute pointer-coarse:after:-inset-2"
          @click="openAdd"
        >
          <span class="hidden @[19rem]:inline">{{ t('asset-timeline-add') }}</span>
        </Button>
        <button
          ref="moreTrigger"
          type="button"
          :class="[HEADER_BUTTON, 'w-7 justify-center -mr-1']"
          :aria-label="t('asset-timeline-more')"
          aria-haspopup="menu"
          :aria-expanded="moreOpen"
          @click="moreOpen = !moreOpen"
        >
          <Icon name="moreHorizontal" size="sm" />
        </button>

        <ResponsiveMenu
          :open="filterOpen"
          :anchor="filterAnchor"
          :title="t('asset-timeline-filter-label')"
          placement="bottom-end"
          role="menu"
          popover-class="bg-surface border border-default rounded-lg shadow-lg py-1 min-w-[12rem]"
          @close="filterOpen = false"
        >
          <MenuList :items="filterItems" @select="setFilter" />
        </ResponsiveMenu>
        <ResponsiveMenu
          :open="moreOpen"
          :anchor="moreAnchor"
          :title="t('asset-timeline-heading')"
          placement="bottom-end"
          role="menu"
          popover-class="bg-surface border border-default rounded-lg shadow-lg py-1 min-w-[14rem]"
          @close="moreOpen = false"
        >
          <MenuList :items="moreItems" @select="onMore" />
        </ResponsiveMenu>
      </template>

      <div ref="listEl" class="flex flex-col pb-1">
        <!-- What the records add up to, in a sentence. -->
        <p v-if="sentence" class="timeline-grid pt-2.5 pb-1 text-xs leading-5 text-secondary">
          <span class="tl-full">{{ sentence }}</span>
        </p>

        <!-- Work still open leads the list. Each row says so itself. -->
        <ul v-if="openEntries.length > 0" class="flex flex-col" :aria-label="t('asset-timeline-in-progress')">
          <AssetTimelineEntryRow
            v-for="entry in openEntries"
            :key="entry.key"
            :entry="entry"
            :type="typeOf(entry.source === 'record' ? entry.data.record_type : null)"
            :can-edit="canEdit"
            :hide-type="!!recordType"
            :ticket="entry.source === 'record' ? ticketOf(entry.data) : null"
            :moved-to="moves.marks.get(entry.key) ?? null"
            :highlighted="highlightKey === entry.key"
            @open="openRecord"
            @complete="(id) => openEdit(id, true)"
          />
        </ul>

        <!-- Nothing recorded yet. -->
        <div v-if="inviteFirst" class="timeline-grid py-2.5">
          <div class="tl-full flex flex-col gap-0.5">
            <p class="text-sm leading-5 font-medium text-primary">{{ t('asset-timeline-empty-title') }}</p>
            <p class="text-xs leading-5 text-secondary max-w-prose">{{ t('asset-timeline-empty-description') }}</p>
          </div>
        </div>

        <!-- A year: its label where the dates are, its total where the
             costs are, and a hairline above. -->
        <section
          v-for="(group, g) in groups"
          :key="group.year"
          :aria-label="group.year"
          :class="g > 0 || openEntries.length > 0 ? 'border-t border-subtle mt-1 pt-1' : ''"
        >
          <h3 class="timeline-grid py-1 text-xs leading-5">
            <span class="tl-year font-medium text-secondary tabular-nums">{{ group.year }}</span>
            <span v-if="group.totals" class="tl-end text-tertiary tabular-nums text-right">{{ group.totals }}</span>
          </h3>
          <ul class="flex flex-col">
            <AssetTimelineEntryRow
              v-for="row in group.rows"
              :key="row.entry.key"
              :entry="row.entry"
              :type="row.type"
              :predecessor-tag="row.predecessorTag"
              :replaced-by="row.entry.source === 'record' ? replacedByOf(row.entry.data) : null"
              :hide-type="!!recordType"
              :ticket="row.entry.source === 'record' ? ticketOf(row.entry.data) : null"
              :moved-to="moves.marks.get(row.entry.key) ?? null"
              :imported-to="moves.imported.get(row.entry.key) ?? null"
              :can-edit="canEdit && row.entry.asset_id === asset.id"
              :highlighted="highlightKey === row.entry.key"
              @open="openRecord"
            />
          </ul>
        </section>

        <p
          v-if="loaded && groups.length === 0 && openEntries.length === 0 && !inviteFirst"
          class="timeline-grid py-2.5 text-xs leading-5 text-tertiary"
        >
          <span class="tl-full">{{ t('asset-timeline-nothing-matches') }}</span>
        </p>

        <!-- Older pages: on the text line, like an entry. -->
        <div v-if="hasMore" class="timeline-grid py-1.5 text-xs leading-5">
          <button
            type="button"
            class="tl-text relative justify-self-start font-medium text-accent hover:underline disabled:opacity-60 pointer-coarse:after:absolute pointer-coarse:after:-inset-3"
            :disabled="loadingMore"
            @click="loadMore"
          >
            {{ t('asset-timeline-show-older-than', { date: oldestLoaded }) }}
          </button>
        </div>
      </div>

      <!-- The record, opened from its row. A narrow dialog: it is a list. -->
      <Modal :show="viewing !== null" :title="viewingTitle" size="sm" @close="closeRecord">
        <AssetRecordDetail
          v-if="viewing"
          :record="viewing"
          :type="typeOf(viewing.record_type)"
          :replacement="replacedByOf(viewing)"
          :asset-serial="serialEditable(viewing) ? (asset.serial_number ?? null) : undefined"
          :updating-serial="updatingSerial"
          @update-serial="updateSerial"
        />
        <template v-if="viewing && canEdit && viewing.asset_id === asset.id" #footer>
          <div class="flex items-center justify-end gap-2">
            <Button
              v-if="canDelete(viewing)"
              variant="ghost-danger"
              size="sm"
              class="mr-auto"
              @click="confirmDelete = viewing"
            >
              {{ t('asset-record-delete') }}
            </Button>
            <Button variant="secondary" size="sm" @click="openEdit(viewing.id)">{{ t('asset-record-edit') }}</Button>
            <Button v-if="viewing.completed_on === null" size="sm" @click="openEdit(viewing.id, true)">
              {{ t('asset-timeline-complete') }}
            </Button>
          </div>
        </template>
      </Modal>

      <AssetRecordFormModal
        :show="form.show"
        :asset="asset"
        :record="form.record"
        :completing="form.completing"
        :loans="loans"
        :tickets="tickets"
        @close="form.show = false"
        @saved="onSaved"
        @carried="onCarried"
      />

      <ConfirmModal
        :show="confirmDelete !== null"
        :title="t('asset-record-delete-title')"
        :message="
          confirmDelete && confirmDelete.files.length > 0
            ? t('asset-record-delete-message-files', { count: confirmDelete.files.length })
            : t('asset-record-delete-message')
        "
        :confirm-label="t('asset-record-delete')"
        variant="danger"
        @confirm="deleteRecord"
        @close="confirmDelete = null"
      />
    </SectionCard>
  </div>
</template>

<style>
/* The card's grid. The same lines at every width:
   - the left edge, shared with the card title (the header's 0.75rem);
   - a gutter one icon wide, for the lifecycle state the asset moved to;
   - the text line;
   - the right edge, for a cost, a year's total or an action.
   It stops growing at 60rem so a figure never drifts far from its text. */
.timeline-grid {
  display: grid;
  grid-template-columns: 3.25rem 0.875rem minmax(0, 1fr) auto;
  column-gap: 0.5rem;
  padding-inline: 0.75rem;
  max-width: 60rem;
}
/* Where a child sits when it isn't simply the next cell. */
.timeline-grid > .tl-full {
  grid-column: 1 / -1;
}
.timeline-grid > .tl-text {
  grid-column: 3 / -1;
}
.timeline-grid > .tl-year {
  grid-column: 1 / 3;
}
.timeline-grid > .tl-end {
  grid-column: 4;
}
/* The one change of structure: with the text set very large (the card
   under 16rem of its own type), the date takes a line of its own so the
   text keeps its room. */
@container (max-width: 16rem) {
  .timeline-grid {
    grid-template-columns: 0.875rem minmax(0, 1fr) auto;
  }
  .timeline-grid > time {
    grid-column: 1 / -1;
  }
  .timeline-grid > .tl-text {
    grid-column: 2 / -1;
  }
  .timeline-grid > .tl-year {
    grid-column: 1 / 3;
  }
  .timeline-grid > .tl-end {
    grid-column: 3;
  }
}
</style>
