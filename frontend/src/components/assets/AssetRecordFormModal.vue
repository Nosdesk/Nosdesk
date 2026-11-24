<script setup lang="ts">
/**
 * Add, edit or complete an asset record. A narrow dialog on desktop and a
 * bottom sheet on phones (the app's Modal does both), with the same
 * two-column grid in each: a field takes one column when its value is
 * short (a date, a cost, a reference) and both when it isn't.
 *
 * Type first, then when the work was done; any past date is ordinary. How
 * it ended is one field: an outcome, or "Still in progress". The asset's
 * status is never touched, and the form says so under its title.
 *
 * Fields most records leave empty (notes, a ticket, our technician, who
 * had it then, the time of day) open from one line of links at the foot.
 * An edit sends only what changed, so a time or a cost nobody touched
 * keeps its precision.
 *
 * A save that newly names a replacement asset turns the form into one more
 * step: what follows the equipment to the new asset (its open tickets, who
 * holds it and where, and the old asset's status). Nothing moves unless
 * ticked; Skip, or closing, keeps the record and moves nothing.
 */
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import Modal from '@/components/Modal.vue'
import Autocomplete from '@/components/common/Autocomplete.vue'
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import Button from '@/components/common/Button.vue'
import Checkbox from '@/components/common/Checkbox.vue'
import DatePicker from '@/components/common/DatePicker.vue'
import FormInput from '@/components/common/FormInput.vue'
import FormTextarea from '@/components/common/FormTextarea.vue'
import SearchableDropdown from '@/components/common/SearchableDropdown.vue'
import Spinner from '@/components/common/Spinner.vue'
import TimePicker from '@/components/common/TimePicker.vue'
import { assetStatusLabel } from '@/utils/assetStatusMeta'
import { extractErrorMessage } from '@/utils/errors'
import { useUsersDirectory } from '@/composables/useUsersDirectory'
import { useAssetRecordTypes, useAssetTimelineInvalidate } from '@/composables/useAssetTimeline'
import { useWorkspaceQuery, useWorkspaceQueryCache } from '@/queries/workspaceQuery'
import { fetchEligibleUsers } from '@/services/eligibleUsers'
import { getAssetById, getPaginatedAssetsExcluding } from '@/services/assetService'
import { coarseStatusBucket } from '@nosdesk/core/types/workflow'
import { getDateConfig } from '@nosdesk/core/utils/dateUtils'
import { currencyName, readAmountInput, type AmountInputError } from '@nosdesk/core/utils/money'
import {
  assetRecordKeys,
  assetRecordService,
  REPLACED_ASSET_STATUSES,
  type CreateAssetRecordBody,
  type ReplacedAssetStatus,
  type ReplacementCarryOverBody,
  type UpdateAssetRecordBody,
} from '@nosdesk/core/services/assetRecordService'
import type {
  Asset,
  AssetLoan,
  AssetRecord,
  AssetRecordCoreField,
  AssetRecordWrite,
  AssetTimelineTicket,
} from '@nosdesk/core/types/asset'
import AssetRecordTypeFields from './AssetRecordTypeFields.vue'
import AssetDisposalFields, { disposalBody, emptyDisposal, type DisposalDraft } from './AssetDisposalFields.vue'
import {
  agoText,
  clockOf,
  dayOf,
  daysBetweenDays,
  fileSizeText,
  formatDay,
  momentOf,
  outcomeName,
  todayDay,
  typeName,
} from './assetTimeline'

const props = defineProps<{
  show: boolean
  asset: Pick<
    Asset,
    | 'id'
    | 'name'
    | 'asset_tag'
    | 'status'
    | 'kind'
    | 'purchase_date'
    | 'location'
    | 'primary_user_uuid'
    | 'managed_by_user_uuid'
    | 'asset_groups'
  >
  /** The record to edit; absent to add one. */
  record?: AssetRecord | null
  /** Open an in-progress record on its completion. */
  completing?: boolean
  /** The asset's loans, to say who had it on the day of the work. */
  loans: AssetLoan[]
  /** Tickets linked to the asset, offered in the ticket field. */
  tickets: AssetTimelineTicket[]
}>()

const emit = defineEmits<{
  close: []
  saved: [write: AssetRecordWrite, files: File[], addAnother: boolean]
  /** The carry-overs ran; the toast is the parent's. */
  carried: [replacement: Pick<Asset, 'id' | 'name' | 'asset_tag'>]
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const { getUserHandle } = useUsersDirectory()
const workspaceCache = useWorkspaceQueryCache()
const invalidateTimeline = useAssetTimelineInvalidate()

// --- What the workspace offers ---------------------------------------

const recordTypes = useAssetRecordTypes(() => props.show)
const types = recordTypes.types
const currency = recordTypes.currency
const suggestedCurrency = recordTypes.suggestedCurrency
/** Days and times on a record are the workspace's, as the server reads
 *  them, whatever zone the viewer is in. */
const zone = computed(() => recordTypes.timezone.value ?? getDateConfig().defaultTimezone)
const catalogueQuery = useWorkspaceQuery({
  key: [...assetRecordKeys.catalogue],
  query: () => assetRecordService.catalogue(),
  enabled: () => props.show,
})
const catalogue = computed(() => catalogueQuery.data.value ?? [])
const vendorsQuery = useWorkspaceQuery({
  key: [...assetRecordKeys.vendors('')],
  query: () => assetRecordService.vendors(),
  enabled: () => props.show,
})
const vendors = computed(() => vendorsQuery.data.value ?? [])
const adding = ref<string | null>(null)
const typesLoaded = computed(() => recordTypes.query.data.value !== undefined)

/** The live types offered for this asset's kind. */
const offeredTypes = computed(() =>
  types.value.filter(
    (type) =>
      !type.archived_at && (!type.applies_to_kinds || type.applies_to_kinds.includes(props.asset.kind)),
  ),
)

// --- The form ---------------------------------------------------------

const form = reactive({
  type: null as string | null,
  day: '',
  time: '',
  withTime: false,
  finished: 'done' as 'done' | 'open',
  completedDay: '',
  /** Completion follows the work date until someone sets it. */
  completedTouched: false,
  outcome: null as string | null,
  summary: '',
  vendor: '',
  technician: '' as string,
  reference: '',
  cost: '',
  ticket: '' as string,
  attributes: {} as Record<string, unknown>,
  /** Holder and location as entered. Sent only when changed from what the
   *  section opened with; otherwise the server derives them. */
  holder: '' as string,
  location: '',
  notes: '',
  replacement: '' as string,
  addAnother: false,
})
/** The optional fields shown: opened by their link, or already holding a
 *  value (or required by the type) when the form fills. */
const extra = reactive({ notes: false, ticket: false, technician: false, holder: false })
const LINK =
  'relative font-medium text-accent hover:underline pointer-coarse:after:absolute pointer-coarse:after:-inset-y-3 pointer-coarse:after:-inset-x-1'
function openExtras(record?: AssetRecord | null) {
  extra.notes = !!record?.notes
  extra.ticket = !!record?.ticket_id || needs('ticket')
  extra.technician = !!record?.performed_by_uuid
  extra.holder = false
  snapshotStart.value = null
  if (record?.snapshot_source === 'entered' || needs('holder') || needs('location')) openHolder()
}

/** What "Who had it then" opened with, to send it only once changed. */
const snapshotStart = ref<{ holder: string; location: string } | null>(null)

/** Open "Who had it then", prefilled with what the server would derive
 *  (or the record's own values), so leaving it untouched changes nothing. */
function openHolder() {
  if (!extra.holder && !form.holder && !form.location) {
    form.holder = derived.value.holder ?? ''
    form.location = derived.value.location ?? ''
  }
  extra.holder = true
  if (!snapshotStart.value) snapshotStart.value = { holder: form.holder, location: form.location.trim() }
}

const files = ref<File[]>([])
const fileInput = ref<HTMLInputElement | null>(null)
const submitting = ref(false)
const errorMessage = ref('')
const triedSubmit = ref(false)
/** What an edit started from, to send only what changed. */
const baseline = ref<Record<string, unknown> | null>(null)
/** The cost text an edit started from. */
const seededCost = ref('')

const editing = computed(() => !!props.record)
const type = computed(() => types.value.find((x) => x.slug === form.type))

function mode(field: AssetRecordCoreField) {
  return type.value?.core_fields[field] ?? 'optional'
}
const shows = (field: AssetRecordCoreField) => mode(field) !== 'hidden'
const needs = (field: AssetRecordCoreField) => mode(field) === 'required'

function reset(keep?: { type: string | null; day: string; vendor: string }) {
  Object.assign(form, {
    type: keep?.type ?? (offeredTypes.value.length === 1 ? offeredTypes.value[0].slug : null),
    day: keep?.day ?? '',
    time: '',
    withTime: false,
    finished: 'done',
    completedDay: keep?.day ?? '',
    completedTouched: false,
    outcome: null,
    summary: '',
    vendor: keep?.vendor ?? '',
    technician: '',
    reference: '',
    cost: '',
    ticket: '',
    attributes: {},
    holder: '',
    location: '',
    notes: '',
    replacement: '',
  })
  files.value = []
  errorMessage.value = ''
  triedSubmit.value = false
  baseline.value = null
  seededCost.value = ''
  holdTypeWatch()
  openExtras()
}

/** Filling the form sets its type; that isn't someone changing it. */
let filling = false
function holdTypeWatch() {
  filling = true
  void nextTick(() => (filling = false))
}

/** A stored amount (`"1250.50"`) as the reader's locale writes it, without
 *  grouping, so it reads back as it was. */
function localAmount(amount: string): string {
  const decimal =
    new Intl.NumberFormat(getDateConfig().defaultLocale)
      .formatToParts(1.5)
      .find((p) => p.type === 'decimal')?.value ?? '.'
  return amount.replace('.', decimal)
}

function fill(record: AssetRecord) {
  holdTypeWatch()
  // Completing today, or on the day of the work if that is later in the
  // workspace's calendar.
  const today = todayDay(zone.value)
  const completeOn = today < record.occurred_on ? record.occurred_on : today
  Object.assign(form, {
    type: record.record_type,
    day: record.occurred_at ? dayOf(record.occurred_at, zone.value) : record.occurred_on,
    time: clockOf(record.occurred_at, zone.value),
    withTime: !!record.occurred_at,
    finished: record.completed_on || props.completing ? 'done' : 'open',
    completedDay: record.completed_on ?? (props.completing ? completeOn : ''),
    completedTouched: true,
    outcome: record.outcome,
    summary: record.summary ?? '',
    vendor: record.vendor ?? '',
    technician: record.performed_by_uuid ?? '',
    reference: record.reference ?? '',
    cost: record.cost ? localAmount(record.cost.amount) : '',
    ticket: record.ticket_id ? String(record.ticket_id) : '',
    attributes: { ...record.attributes },
    holder: record.holder_uuid ?? '',
    location: record.location ?? '',
    notes: record.notes ?? '',
    replacement: record.replacement_asset_id ? String(record.replacement_asset_id) : '',
  })
  openExtras(record)
  files.value = []
  errorMessage.value = ''
  triedSubmit.value = false
  seededCost.value = form.cost
  // The record as it stood: completing an open one sends its completion.
  const start: Record<string, unknown> = { ...body() }
  baseline.value = record.completed_on ? start : { ...start, completed_on: null, outcome: null }
}

// The only type offered is picked once the types arrive.
watch(offeredTypes, (offered) => {
  if (props.show && !props.record && !form.type && offered.length === 1) form.type = offered[0].slug
})

watch(
  () => form.day,
  (day) => {
    if (!form.completedTouched) form.completedDay = day
  },
)

// A type change keeps what still fits: the outcome and the type's own
// fields belong to the old type. Fields the new type requires open.
watch(
  () => form.type,
  (next, previous) => {
    if (previous && next !== previous && !filling) {
      form.outcome = null
      form.attributes = {}
    }
    if (needs('ticket')) extra.ticket = true
    if (needs('holder') || needs('location')) openHolder()
  },
)

async function addFromCatalogue(slug: string) {
  adding.value = slug
  try {
    const added = await assetRecordService.addFromCatalogue(slug)
    await Promise.all([
      workspaceCache.invalidateQueries({ key: [...assetRecordKeys.types] }),
      workspaceCache.invalidateQueries({ key: [...assetRecordKeys.catalogue] }),
    ])
    await recordTypes.query.refetch()
    form.type = added.slug
  } catch (e) {
    errorMessage.value = extractErrorMessage(e, t('asset-record-form-error'))
  } finally {
    adding.value = null
  }
}

// --- When -------------------------------------------------------------

const today = computed(() => todayDay(zone.value))
/** One day of slack for timezones; later than that is refused. */
const latestDay = computed(() => {
  const d = new Date(`${today.value}T12:00:00Z`)
  d.setUTCDate(d.getUTCDate() + 1)
  return d.toISOString().slice(0, 10)
})

const daysAgo = computed(() => (form.day ? daysBetweenDays(form.day, today.value) : 0))
/** Said under the date once the work is in the past: where the record
 *  will sit, and that it is stamped as entered today. */
const backdatedNote = computed(() => {
  if (!form.day || daysAgo.value < 2 || editing.value) return null
  return t('asset-record-form-backdated', { ago: agoText(t, form.day, today.value), date: formatDay(form.day) })
})
const beforePurchase = computed(() => {
  const purchased = props.asset.purchase_date
  if (!form.day || !purchased || form.day >= purchased) return null
  return t('asset-record-form-before-purchase', { date: formatDay(purchased) })
})

const isOpen = computed(() => form.finished === 'open')

const outcomeIsReplaced = computed(
  () => type.value?.outcomes.find((o) => o.key === form.outcome)?.category === 'replaced',
)

// --- Who had it then ---------------------------------------------------

function person(uuid: string | null | undefined): string | null {
  return uuid ? (getUserHandle(uuid).user.value?.name ?? null) : null
}

interface Derived {
  source: 'loan' | 'current' | 'none'
  text: string
  holder: string | null
  location: string | null
}

/** What the server will derive for the day, shown before saving so nobody
 *  has to guess. */
const derived = computed<Derived>(() => {
  const day = form.day
  const none = { holder: null, location: null }
  if (!day) return { source: 'none', text: '', ...none }
  const loan = props.loans.find(
    (l) => dayOf(l.loaned_at, zone.value) <= day && (!l.returned_at || dayOf(l.returned_at, zone.value) >= day),
  )
  const date = formatDay(day)
  if (loan) {
    return {
      source: 'loan',
      text: t('asset-record-form-snapshot-loan', {
        name: person(loan.borrower_user_uuid) ?? t('asset-timeline-someone'),
        date,
      }),
      holder: loan.borrower_user_uuid,
      location: null,
    }
  }
  if (daysAgo.value <= 1) {
    const uuid = props.asset.primary_user_uuid ?? null
    const holder = person(uuid)
    const location = props.asset.location ?? null
    const current = { source: 'current' as const, holder: uuid, location }
    if (holder && location) {
      return { ...current, text: t('asset-record-form-snapshot-current-both', { name: holder, location }) }
    }
    if (holder) return { ...current, text: t('asset-record-form-snapshot-current-holder', { name: holder }) }
    if (location) return { ...current, text: t('asset-record-form-snapshot-current-location', { location }) }
  }
  return { source: 'none', text: t('asset-record-form-snapshot-none', { date }), ...none }
})

// --- Options ----------------------------------------------------------

/** People a picker offers: the first page by name (the server's largest), with the one already
 *  chosen kept even when it falls outside that page. */
const PEOPLE_PAGE = 100
const techniciansQuery = useWorkspaceQuery({
  key: ['asset-record-form', 'people', 'assignee'],
  query: () => fetchEligibleUsers('assignee', '', PEOPLE_PAGE),
  enabled: () => props.show && extra.technician,
})
const holdersQuery = useWorkspaceQuery({
  key: ['asset-record-form', 'people', 'requester'],
  query: () => fetchEligibleUsers('requester', '', PEOPLE_PAGE),
  enabled: () => props.show && extra.holder,
})
function peopleOptions(list: { uuid: string; name: string }[] | undefined, chosen: string) {
  const people = (list ?? []).map((u) => ({ value: u.uuid, label: u.name }))
  if (chosen && !people.some((p) => p.value === chosen)) {
    people.unshift({ value: chosen, label: person(chosen) ?? t('asset-timeline-someone') })
  }
  return [{ value: '', label: t('asset-record-form-none') }, ...people]
}
const technicianOptions = computed(() => peopleOptions(techniciansQuery.data.value, form.technician))
const holderOptions = computed(() => peopleOptions(holdersQuery.data.value, form.holder))

const ticketOptions = computed(() => {
  const linked = props.tickets.map((ticket) => ({
    value: String(ticket.id),
    label: `#${ticket.number ?? ticket.id} ${ticket.title ?? ''}`.trim(),
  }))
  // A record's ticket that is no longer linked stays offered.
  if (form.ticket && !linked.some((o) => o.value === form.ticket)) {
    linked.unshift({ value: form.ticket, label: `#${form.ticket}` })
  }
  return [{ value: '', label: t('asset-record-form-none') }, ...linked]
})

/** Assets a replacement can point at: the first page by name, this one
 *  left out. */
const ASSETS_PAGE = 100
const replacementsQuery = useWorkspaceQuery({
  key: () => ['asset-record-form', 'replacements', props.asset.id],
  query: () => getPaginatedAssetsExcluding({ page: 1, pageSize: ASSETS_PAGE, excludeIds: [props.asset.id] }),
  enabled: () => props.show && outcomeIsReplaced.value,
})
const assetOptions = computed(() => {
  const assets = (replacementsQuery.data.value?.data ?? []).map((a) => ({
    value: String(a.id),
    label: a.asset_tag ? `${a.asset_tag} ${a.name}` : a.name,
  }))
  if (form.replacement && !assets.some((a) => a.value === form.replacement)) {
    assets.unshift({ value: form.replacement, label: `#${form.replacement}` })
  }
  return [{ value: '', label: t('asset-record-form-replacement-none') }, ...assets]
})

const typeOptions = computed(() => [
  ...offeredTypes.value.map((x) => ({ value: x.slug, label: typeName(t, x) })),
  // An archived type stays shown on the record that has it.
  ...(type.value && !offeredTypes.value.includes(type.value)
    ? [{ value: type.value.slug, label: typeName(t, type.value) }]
    : []),
  ...catalogue.value
    .filter((c) => !c.added)
    .map((c) => ({
      value: `catalogue:${c.slug}`,
      label: t(`asset-record-type-${c.slug}`),
      group: t('asset-record-form-type-more-hint'),
    })),
])
function setType(value: string) {
  if (value.startsWith('catalogue:')) void addFromCatalogue(value.slice(10))
  else form.type = value
}

// How it ended is one choice: an outcome of the type, or still open.
const OPEN = '__open'
const endOptions = computed(() => [
  {
    value: '',
    label:
      (type.value?.outcomes.length ?? 0) > 0 && shows('outcome')
        ? t('asset-record-form-outcome-none')
        : t('asset-record-form-finished'),
  },
  ...(shows('outcome') ? (type.value?.outcomes ?? []) : []).map((o) => ({
    value: o.key,
    label: outcomeName(t, type.value, o.key),
  })),
  { value: OPEN, label: t('asset-record-form-in-progress') },
])
const endValue = computed(() => (isOpen.value ? OPEN : (form.outcome ?? '')))
function setEnd(value: string) {
  form.finished = value === OPEN ? 'open' : 'done'
  form.outcome = value === OPEN || value === '' ? null : value
}

function catalogueOutcomes(entry: { slug: string; outcomes: { key: string; label?: string | null }[] }): string {
  return entry.outcomes.map((o) => o.label ?? t(`asset-record-outcome-${entry.slug}-${o.key}`)).join(', ')
}

/** The record's own currency on an edit, else the workspace's. */
const effectiveCurrency = computed(
  () => props.record?.cost?.currency ?? currency.value ?? suggestedCurrency.value ?? '',
)
const costHint = computed(() => {
  if (!effectiveCurrency.value) return undefined
  return currency.value
    ? currencyName(effectiveCurrency.value)
    : t('asset-record-form-currency-unset', { code: effectiveCurrency.value })
})

/** The cost as typed, read in the reader's locale: "1,250" is a thousand
 *  and more in en-US, and refused as two readings in nl-NL. */
const costInput = computed(() => {
  const text = form.cost.trim()
  return text ? readAmountInput(text) : null
})
const COST_ERRORS: Record<AmountInputError, string> = {
  invalid: 'asset-lifecycle-record-cost-invalid',
  ambiguous: 'asset-lifecycle-record-cost-ambiguous',
  negative: 'asset-lifecycle-record-cost-negative',
  too_many_decimals: 'asset-lifecycle-record-cost-invalid',
}

// --- Files ------------------------------------------------------------

function pickFiles(event: Event) {
  const input = event.target as HTMLInputElement
  files.value = [...files.value, ...Array.from(input.files ?? [])]
  input.value = ''
}

// --- Save -------------------------------------------------------------

const missing = computed(() => {
  const out: Partial<Record<'type' | 'day' | AssetRecordCoreField, string>> = {}
  if (!form.type) out.type = t('asset-record-form-required-type')
  if (!form.day) out.day = t('asset-record-form-required-day')
  if (needs('summary') && !form.summary.trim()) out.summary = t('asset-record-form-required')
  if (needs('vendor') && !form.vendor.trim() && !form.technician) out.vendor = t('asset-record-form-required')
  if (needs('reference') && !form.reference.trim()) out.reference = t('asset-record-form-required')
  if (needs('cost') && !form.cost.trim()) out.cost = t('asset-record-form-required')
  else if (costInput.value && !costInput.value.ok && form.cost !== seededCost.value) {
    out.cost = t(COST_ERRORS[costInput.value.error])
  }
  if (needs('outcome') && !isOpen.value && !form.outcome) out.outcome = t('asset-record-form-required')
  if (needs('ticket') && !form.ticket) out.ticket = t('asset-record-form-required')
  if (needs('holder') && !form.holder) out.holder = t('asset-record-form-required')
  if (needs('location') && !form.location.trim()) out.location = t('asset-record-form-required')
  return out
})
const error = (key: keyof typeof missing.value) => (triedSubmit.value ? missing.value[key] : undefined)

/** The cost to send: none when the field is empty, in the record's own
 *  currency on an edit. */
function costBody(): CreateAssetRecordBody['cost'] {
  const input = costInput.value
  if (!input?.ok) return null
  return { amount: input.amount, ...(props.record?.cost ? { currency: props.record.cost.currency } : {}) }
}

function body(): CreateAssetRecordBody {
  const timed = form.withTime && form.time
  const out: CreateAssetRecordBody = {
    record_type: form.type as string,
    summary: form.summary.trim() || null,
    notes: form.notes.trim() || null,
    ...(timed ? { occurred_at: momentOf(form.day, form.time, zone.value) } : { occurred_on: form.day }),
    completed_on: isOpen.value ? null : form.completedDay || form.day,
    outcome: isOpen.value ? null : form.outcome,
    vendor: form.vendor.trim() || null,
    performed_by_uuid: form.technician || null,
    reference: form.reference.trim() || null,
    ticket_id: form.ticket ? Number(form.ticket) : null,
    // Until the types load, a record's replacement stands as it is.
    replacement_asset_id:
      form.replacement && (outcomeIsReplaced.value || !type.value) ? Number(form.replacement) : null,
    attributes: form.attributes,
  }
  const start = snapshotStart.value
  if (extra.holder && start && (form.holder !== start.holder || form.location.trim() !== start.location)) {
    out.holder_uuid = form.holder || null
    out.location = form.location.trim() || null
  }
  // A field the type hides isn't the form's to write.
  const hidden: [AssetRecordCoreField, keyof CreateAssetRecordBody][] = [
    ['summary', 'summary'],
    ['vendor', 'vendor'],
    ['reference', 'reference'],
    ['outcome', 'outcome'],
    ['ticket', 'ticket_id'],
    ['holder', 'holder_uuid'],
    ['location', 'location'],
  ]
  for (const [field, key] of hidden) if (!shows(field)) delete out[key]
  return out
}

/** An edit: the fields that differ from what the form started with. */
function changes(record: AssetRecord): UpdateAssetRecordBody {
  const next: Record<string, unknown> = { ...body() }
  const before = baseline.value ?? {}
  const out: Record<string, unknown> = { expected_version: record.version }
  for (const [key, value] of Object.entries(next)) {
    if (JSON.stringify(value) !== JSON.stringify(before[key])) out[key] = value
  }
  if (form.cost !== seededCost.value && shows('cost')) out.cost = costBody()
  return out as unknown as UpdateAssetRecordBody
}

async function submit() {
  triedSubmit.value = true
  if (Object.keys(missing.value).length > 0) return
  submitting.value = true
  errorMessage.value = ''
  try {
    let write: AssetRecordWrite
    if (props.record) {
      write = await assetRecordService.update(props.asset.id, props.record.id, changes(props.record))
    } else {
      write = await assetRecordService.create(props.asset.id, {
        ...body(),
        ...(shows('cost') ? { cost: costBody() } : {}),
      })
    }
    const another = form.addAnother && !props.record
    const saved = { write, files: files.value, another }
    if (newlyReplaced(write)) await openCarry(saved)
    else finish(saved)
  } catch (e) {
    const status = (e as { response?: { status?: number } }).response?.status
    errorMessage.value =
      status === 409 ? t('asset-record-form-conflict') : extractErrorMessage(e, t('asset-record-form-error'))
    // Someone else's change: reopening the record reads it fresh.
    if (status === 409) void invalidateTimeline(props.asset.id)
  } finally {
    submitting.value = false
  }
}

/** Hand the saved record to the parent, which closes the form or, when
 *  adding another, gets it back empty. */
function finish(saved: Saved) {
  carry.saved = null
  emit('saved', saved.write, saved.files, saved.another)
  if (saved.another) {
    // Entering a stack of past work: keep the type, vendor and date.
    reset({ type: form.type, day: form.day, vendor: form.vendor })
  }
}

// --- Carry-overs to the replacement -------------------------------------

interface Saved {
  write: AssetRecordWrite
  files: File[]
  another: boolean
}

/** The save named a replacement the record didn't name before. */
function newlyReplaced(write: AssetRecordWrite): boolean {
  const next = write.record.replacement_asset_id
  return next !== null && next !== (props.record?.replacement_asset_id ?? null)
}

const carry = reactive({
  /** The save waiting on this step; the step shows while it is set. */
  saved: null as Saved | null,
  /** The new asset, once read. */
  replacement: null as Asset | null,
  replacementId: 0,
  tickets: false,
  assignment: false,
  move: false,
  moveTo: 'retired' as ReplacedAssetStatus,
  disposal: emptyDisposal() as DisposalDraft,
  running: false,
  error: '',
})
const carrying = computed(() => carry.saved !== null)

/** The old asset's open tickets, as the timeline lists them. */
const openTickets = computed(() =>
  props.tickets.filter((ticket) => !ticket.state || coarseStatusBucket(ticket.state.category) !== 'closed'),
)

const replacementLabel = computed(() => {
  const r = carry.replacement
  return r ? (r.asset_tag ?? r.name) : `#${carry.replacementId}`
})

/** What the old asset has to copy, said as the new asset would take it. A
 *  synced asset's holder and location are the sync's. */
const copyable = computed(() => {
  const old = props.asset
  const synced = !!carry.replacement?.external_sync_source
  const items: string[] = []
  const holder = synced ? null : person(old.primary_user_uuid)
  if (holder) items.push(holder)
  const manager = person(old.managed_by_user_uuid)
  if (manager) items.push(t('asset-record-carry-managed-by', { name: manager }))
  const location = synced ? '' : (old.location ?? '').trim()
  if (location) items.push(location)
  const groups = old.asset_groups ?? []
  if (groups.length === 1) items.push(groups[0].name)
  else if (groups.length > 1) items.push(t('asset-record-carry-groups', { count: groups.length }))
  return items
})

/** The new asset already has some of what would copy. */
const replacementAssigned = computed(() => {
  const r = carry.replacement
  return (
    !!r &&
    (!!r.primary_user_uuid || !!r.managed_by_user_uuid || !!r.location?.trim() || (r.asset_groups?.length ?? 0) > 0)
  )
})

const moveOptions = computed(() =>
  REPLACED_ASSET_STATUSES.filter((s) => s !== props.asset.status).map((s) => ({
    value: s,
    label: assetStatusLabel(t, s),
  })),
)

async function openCarry(saved: Saved) {
  const id = saved.write.record.replacement_asset_id as number
  Object.assign(carry, {
    saved,
    replacement: null,
    replacementId: id,
    tickets: openTickets.value.length > 0,
    assignment: false,
    move: false,
    moveTo: moveOptions.value[0]?.value ?? 'retired',
    disposal: emptyDisposal(),
    running: false,
    error: '',
  })
  try {
    carry.replacement = await getAssetById(id)
  } catch {
    // Without the new asset, nothing says it has none of these: leave the
    // copy unticked.
    return
  }
  // Ticked only when the new asset has none of what would copy.
  carry.assignment = copyable.value.length > 0 && !replacementAssigned.value
}

const carryBody = computed<ReplacementCarryOverBody>(() => {
  const out: ReplacementCarryOverBody = {}
  if (carry.tickets && openTickets.value.length > 0) out.link_open_tickets = true
  if (carry.assignment && copyable.value.length > 0) out.copy_assignment = true
  if (carry.move) {
    out.move_old_to = carry.moveTo
    if (carry.moveTo === 'disposed') out.disposal = disposalBody(carry.disposal)
  }
  return out
})
const carriesAnything = computed(() => Object.keys(carryBody.value).length > 0)

async function applyCarry() {
  const saved = carry.saved
  if (!saved || !carriesAnything.value) return
  carry.running = true
  carry.error = ''
  try {
    await assetRecordService.carryOver(props.asset.id, saved.write.record.id, carryBody.value)
    // The sync rows refresh both timelines; this makes it certain.
    void invalidateTimeline(props.asset.id)
    void invalidateTimeline(carry.replacementId)
    emit('carried', carry.replacement ?? { id: carry.replacementId, name: replacementLabel.value, asset_tag: null })
    finish(saved)
  } catch (e) {
    carry.error = extractErrorMessage(e, t('asset-record-carry-error'))
  } finally {
    carry.running = false
  }
}

function skipCarry() {
  if (carry.saved) finish(carry.saved)
}

/** Closing the step keeps the record and moves nothing. */
function close() {
  skipCarry()
  emit('close')
}

// Fill the form each time it opens. Last, so everything `body()` reads
// is set up.
watch(
  () => [props.show, props.record] as const,
  ([open]) => {
    if (!open) {
      // Closed from outside mid-step: the record is saved all the same.
      if (carry.saved) finish(carry.saved)
      return
    }
    if (props.record) fill(props.record)
    else reset()
  },
  { immediate: true },
)
// The workspace zone arrives with the types: an edit opened before then
// reads its day and time again in it.
watch(zone, () => {
  if (props.show && props.record) fill(props.record)
})

// A phone on its side: under 500px of height the footer joins the form.
const short = ref(false)
let shortQuery: MediaQueryList | null = null
const onShort = (e: MediaQueryListEvent) => (short.value = e.matches)
onMounted(() => {
  if (typeof window.matchMedia !== 'function') return
  shortQuery = window.matchMedia('(max-height: 500px)')
  short.value = shortQuery.matches
  shortQuery.addEventListener('change', onShort)
})
onBeforeUnmount(() => shortQuery?.removeEventListener('change', onShort))

const assetLabel = computed(() => props.asset.asset_tag ?? props.asset.name)
const statusName = computed(() => assetStatusLabel(t, props.asset.status))
const title = computed(() => {
  if (carrying.value) return t('asset-record-carry-title', { asset: replacementLabel.value })
  if (props.completing && type.value) {
    return t('asset-record-form-title-complete', { type: typeName(t, type.value) })
  }
  if (editing.value) return t('asset-record-form-title-edit')
  return t('asset-record-form-title-new', { asset: assetLabel.value })
})
const reassurance = computed(() =>
  carrying.value
    ? t('asset-record-carry-description', { old: assetLabel.value, asset: replacementLabel.value })
    : t('asset-record-form-status-unchanged', { asset: assetLabel.value, status: statusName.value }),
)
</script>

<template>
  <Modal
    :show="show"
    :title="title"
    :description="reassurance"
    size="sm"
    content-class="record-form-panel"
    @close="close"
  >
    <!-- After the save: what follows the equipment to its replacement. One
         row per carry-over, its checkbox, what it does and to what. -->
    <div v-if="carrying" class="grid grid-cols-2 gap-x-3 gap-y-3" data-testid="carry-step">
      <div class="col-span-2 flex items-start gap-2">
        <Checkbox
          id="carry-tickets"
          v-model="carry.tickets"
          size="sm"
          class="mt-0.5"
          :disabled="openTickets.length === 0"
          :aria-label="t('asset-record-carry-tickets')"
        />
        <label for="carry-tickets" class="min-w-0 flex flex-col" :class="openTickets.length === 0 ? 'opacity-50' : 'cursor-pointer'">
          <span class="text-sm leading-5 text-primary">{{ t('asset-record-carry-tickets') }}</span>
          <span class="text-xs leading-5 text-secondary" data-testid="carry-tickets-detail">
            {{ t('asset-record-carry-tickets-detail', { count: openTickets.length, old: assetLabel, asset: replacementLabel }) }}
          </span>
        </label>
      </div>

      <div class="col-span-2 flex items-start gap-2">
        <Checkbox
          id="carry-assignment"
          v-model="carry.assignment"
          size="sm"
          class="mt-0.5"
          :disabled="copyable.length === 0"
          :aria-label="t('asset-record-carry-assignment')"
        />
        <label for="carry-assignment" class="min-w-0 flex flex-col" :class="copyable.length === 0 ? 'opacity-50' : 'cursor-pointer'">
          <span class="text-sm leading-5 text-primary">{{ t('asset-record-carry-assignment') }}</span>
          <span class="text-xs leading-5 text-secondary [overflow-wrap:anywhere]" data-testid="carry-assignment-detail">
            {{ copyable.length > 0 ? copyable.join(', ') : t('asset-record-carry-assignment-none', { old: assetLabel }) }}
          </span>
          <span v-if="copyable.length > 0 && replacementAssigned" class="text-xs leading-5 text-tertiary">
            {{ t('asset-record-carry-assignment-replaces', { asset: replacementLabel }) }}
          </span>
          <span v-if="carry.replacement?.external_sync_source" class="text-xs leading-5 text-tertiary">
            {{ t('asset-record-carry-assignment-synced', { asset: replacementLabel }) }}
          </span>
        </label>
      </div>

      <div class="col-span-2 flex items-start gap-2">
        <Checkbox
          id="carry-move"
          v-model="carry.move"
          size="sm"
          class="mt-0.5"
          :disabled="moveOptions.length === 0"
          :aria-label="t('asset-record-carry-move', { old: assetLabel })"
        />
        <label for="carry-move" class="min-w-0 flex flex-col cursor-pointer">
          <span class="text-sm leading-5 text-primary">{{ t('asset-record-carry-move', { old: assetLabel }) }}</span>
          <span class="text-xs leading-5 text-secondary">{{ t('asset-record-carry-move-detail', { status: statusName }) }}</span>
        </label>
      </div>
      <template v-if="carry.move">
        <BaseDropdown
          :model-value="carry.moveTo"
          :options="moveOptions"
          size="sm"
          :label="t('asset-record-carry-move-to')"
          @update:model-value="carry.moveTo = String($event) as ReplacedAssetStatus"
        />
        <AssetDisposalFields
          v-if="carry.moveTo === 'disposed'"
          v-model="carry.disposal"
          size="sm"
          wide-class="col-span-2"
        />
      </template>

      <p v-if="carry.error" class="col-span-2 text-xs leading-5 text-status-error" role="alert">{{ carry.error }}</p>
      <div v-if="short" class="col-span-2 border-t border-subtle pt-3 flex items-center justify-end gap-2">
        <Button variant="secondary" size="sm" @click="skipCarry">{{ t('asset-record-carry-skip') }}</Button>
        <Button size="sm" :loading="carry.running" :disabled="!carriesAnything" @click="applyCarry">
          {{ t('asset-record-carry-apply') }}
        </Button>
      </div>
    </div>

    <form v-else class="grid grid-cols-2 gap-x-3 gap-y-3" novalidate @submit.prevent="submit">
      <!-- The first record in a workspace: the built-in types, in place.
           One tap adds the type and picks it. -->
      <div v-if="typesLoaded && offeredTypes.length === 0" class="col-span-2 flex flex-col gap-2">
        <p class="text-sm leading-5 text-secondary">{{ t('asset-record-form-type-first-run') }}</p>
        <ul class="flex flex-col divide-y divide-subtle border-y border-subtle">
          <li v-for="entry in catalogue.filter((c) => !c.added)" :key="entry.slug">
            <button
              type="button"
              class="w-full flex items-baseline gap-3 py-2 text-left hover:bg-surface-hover/50 transition-colors pointer-coarse:min-h-11"
              :disabled="!!adding"
              @click="addFromCatalogue(entry.slug)"
            >
              <span class="min-w-0 flex-1 flex flex-col">
                <span class="text-sm leading-5 font-medium text-primary">{{ t(`asset-record-type-${entry.slug}`) }}</span>
                <span class="text-xs leading-5 text-secondary">{{ catalogueOutcomes(entry) }}</span>
              </span>
              <Spinner v-if="adding === entry.slug" size="sm" />
              <span v-else class="text-xs font-medium text-accent">{{ t('asset-record-form-type-use') }}</span>
            </button>
          </li>
        </ul>
        <p class="text-xs leading-5 text-tertiary">{{ t('asset-record-form-type-first-run-hint') }}</p>
      </div>

      <template v-else>
        <BaseDropdown
          :model-value="form.type ?? ''"
          :options="typeOptions"
          size="sm"
          :class="form.type ? '' : 'col-span-2'"
          :label="t('asset-record-form-type-short')"
          :placeholder="t('asset-record-form-type')"
          :error="error('type')"
          :disabled="editing && !!record && record.source === 'transition'"
          @update:model-value="setType(String($event))"
        />
        <BaseDropdown
          v-if="form.type"
          :model-value="endValue"
          :options="endOptions"
          size="sm"
          :label="t('admin-asset-record-types-core-outcome')"
          :error="error('outcome')"
          @update:model-value="setEnd(String($event))"
        />
      </template>

      <template v-if="form.type">
        <DatePicker
          v-model="form.day"
          :label="t('asset-record-form-when-short')"
          block
          :max="latestDay"
          min="1900-01-01"
          :error="!!error('day')"
        />
        <DatePicker
          :model-value="isOpen ? '' : form.completedDay"
          :label="t('asset-record-form-completed')"
          block
          :disabled="isOpen"
          :min="form.day || undefined"
          :max="latestDay"
          @update:model-value="(v: string) => { form.completedDay = v; form.completedTouched = true }"
        />
        <!-- What the date means for this record, in a line. -->
        <p v-if="error('day')" class="col-span-2 -mt-1.5 text-xs leading-5 text-status-error">{{ error('day') }}</p>
        <p v-else-if="beforePurchase" class="col-span-2 -mt-1.5 text-xs leading-5 text-status-warning">{{ beforePurchase }}</p>
        <p v-else-if="backdatedNote" class="col-span-2 -mt-1.5 text-xs leading-5 text-tertiary">{{ backdatedNote }}</p>
        <p v-if="derived.text && (extra.holder || derived.source === 'loan')" class="col-span-2 -mt-1.5 text-xs leading-5 text-tertiary">{{ derived.text }}</p>
        <TimePicker
          v-if="form.withTime"
          v-model="form.time"
          :label="t('asset-record-form-time')"
        />

        <SearchableDropdown
          v-if="outcomeIsReplaced"
          class="col-span-2"
          :model-value="form.replacement"
          :options="assetOptions"
          size="sm"
          :label="t('asset-record-form-replacement')"
          :description="t('asset-record-form-replacement-hint')"
          @update:model-value="form.replacement = String($event)"
        />

        <FormInput
          v-if="shows('summary')"
          v-model="form.summary"
          class="col-span-2"
          size="sm"
          :label="t('admin-asset-record-types-core-summary')"
          :placeholder="t('asset-record-form-summary-placeholder')"
          :required="needs('summary')"
          :error="error('summary')"
          maxlength="255"
        />
        <Autocomplete
          v-if="shows('vendor')"
          v-model="form.vendor"
          class="col-span-2"
          size="sm"
          :options="vendors"
          :label="t('admin-asset-record-types-core-vendor')"
          :required="needs('vendor')"
          :error="error('vendor')"
        />
        <FormInput
          v-if="shows('reference')"
          v-model="form.reference"
          size="sm"
          :label="t('admin-asset-record-types-core-reference')"
          :required="needs('reference')"
          :error="error('reference')"
        />
        <div v-if="shows('cost')" class="relative">
          <FormInput
            v-model="form.cost"
            size="sm"
            :label="t('admin-asset-record-types-core-cost')"
            placeholder="0.00"
            inputmode="decimal"
            :required="needs('cost')"
            :error="error('cost')"
            class="[&_input]:pr-12 [&_input]:tabular-nums"
          />
          <!-- The code sits in the field, so the currency is never a guess. -->
          <span class="absolute right-2.5 top-[1.375rem] h-[2.125rem] flex items-center text-2xs font-medium text-tertiary pointer-events-none">
            {{ effectiveCurrency }}
          </span>
        </div>
        <p v-if="shows('cost') && !currency" class="col-span-2 -mt-1.5 text-xs leading-5 text-tertiary">{{ costHint }}</p>

        <AssetRecordTypeFields v-if="type" v-model="form.attributes" :type="type" />

        <SearchableDropdown
          v-if="extra.technician"
          :class="extra.ticket ? '' : 'col-span-2'"
          :model-value="form.technician"
          :options="technicianOptions"
          size="sm"
          :label="t('asset-record-form-technician')"
          @update:model-value="form.technician = String($event)"
        />
        <BaseDropdown
          v-if="extra.ticket"
          :class="extra.technician ? '' : 'col-span-2'"
          :model-value="form.ticket"
          :options="ticketOptions"
          size="sm"
          :label="t('admin-asset-record-types-core-ticket')"
          :error="error('ticket')"
          @update:model-value="form.ticket = String($event)"
        />
        <template v-if="extra.holder">
          <SearchableDropdown
            v-if="shows('holder')"
            :model-value="form.holder"
            :options="holderOptions"
            size="sm"
            :label="t('asset-record-detail-holder')"
            :error="error('holder')"
            @update:model-value="form.holder = String($event)"
          />
          <FormInput
            v-if="shows('location')"
            v-model="form.location"
            size="sm"
            :label="t('asset-record-detail-location')"
            :error="error('location')"
          />
        </template>
        <FormTextarea
          v-if="extra.notes"
          v-model="form.notes"
          class="col-span-2"
          :label="t('asset-record-form-notes')"
          :rows="2"
          :max-rows="10"
        />

        <ul v-if="files.length > 0 || (record && record.files.length > 0)" class="col-span-2 flex flex-col text-xs-plus leading-5">
          <li v-for="f in record?.files ?? []" :key="`kept-${f.id}`" class="flex items-baseline gap-2">
            <span class="min-w-0 flex-1 truncate text-primary">{{ f.name }}</span>
            <span class="text-tertiary tabular-nums">{{ fileSizeText(f.file_size) }}</span>
          </li>
          <li v-for="(f, i) in files" :key="`new-${i}`" class="flex items-baseline gap-2">
            <span class="min-w-0 flex-1 truncate text-primary">{{ f.name }}</span>
            <span class="text-tertiary tabular-nums">{{ fileSizeText(f.size) }}</span>
            <button type="button" :class="LINK" :aria-label="t('asset-record-form-files-remove', { name: f.name })" @click="files.splice(i, 1)">
              {{ t('asset-record-form-remove') }}
            </button>
          </li>
        </ul>
        <input ref="fileInput" type="file" multiple class="sr-only" tabindex="-1" @change="pickFiles" />

        <!-- The fields most records leave empty: one line of links. -->
        <p class="col-span-2 text-xs leading-6 text-tertiary">
          {{ t('asset-record-form-add') }}
          <button type="button" :class="LINK" @click="fileInput?.click()">{{ t('asset-record-form-add-files') }}</button>
          <template v-if="!extra.notes">
            <span aria-hidden="true"> · </span>
            <button type="button" :class="LINK" @click="extra.notes = true">{{ t('asset-record-form-add-notes') }}</button>
          </template>
          <template v-if="!extra.ticket && shows('ticket')">
            <span aria-hidden="true"> · </span>
            <button type="button" :class="LINK" @click="extra.ticket = true">{{ t('asset-record-form-add-ticket') }}</button>
          </template>
          <template v-if="!extra.technician && shows('vendor')">
            <span aria-hidden="true"> · </span>
            <button type="button" :class="LINK" @click="extra.technician = true">{{ t('asset-record-form-add-technician') }}</button>
          </template>
          <template v-if="!extra.holder && (shows('holder') || shows('location'))">
            <span aria-hidden="true"> · </span>
            <button type="button" :class="LINK" @click="openHolder">{{ t('asset-record-form-add-holder') }}</button>
          </template>
          <template v-if="!form.withTime">
            <span aria-hidden="true"> · </span>
            <button type="button" :class="LINK" @click="form.withTime = true">{{ t('asset-record-form-add-time') }}</button>
          </template>
        </p>
      </template>

      <p v-if="errorMessage" class="col-span-2 text-xs leading-5 text-status-error" role="alert">{{ errorMessage }}</p>

      <div v-if="short" class="col-span-2 border-t border-subtle pt-3">
        <div class="flex flex-wrap items-center gap-x-3 gap-y-2">
          <div class="flex-1 min-w-0">
            <Checkbox
              v-if="!editing && form.type"
              v-model="form.addAnother"
              size="sm"
              :label="t('asset-record-form-add-another')"
            />
          </div>
          <Button size="sm" :loading="submitting" :disabled="!form.type" @click="submit">
            {{ editing ? t('asset-record-form-save') : t('asset-record-form-save-new') }}
          </Button>
        </div>
      </div>
    </form>

    <!-- One row, on a phone too: the sheet has its own close and swipe, so
         Cancel shows from the dialog width up. On a short screen (a phone
         on its side) the row joins the end of the form instead. -->
    <template v-if="!short" #footer>
      <div v-if="carrying" class="flex items-center justify-end gap-2">
        <Button variant="secondary" size="sm" @click="skipCarry">{{ t('asset-record-carry-skip') }}</Button>
        <Button size="sm" :loading="carry.running" :disabled="!carriesAnything" @click="applyCarry">
          {{ t('asset-record-carry-apply') }}
        </Button>
      </div>
      <div v-else class="flex flex-wrap items-center gap-x-2 gap-y-2">
        <div class="flex-1 min-w-0">
          <Checkbox
            v-if="!editing && form.type"
            v-model="form.addAnother"
            size="sm"
            :label="t('asset-record-form-add-another')"
          />
        </div>
        <div class="hidden md:block">
          <Button variant="secondary" size="sm" @click="emit('close')">{{ t('asset-record-form-cancel') }}</Button>
        </div>
        <Button size="sm" :loading="submitting" :disabled="!form.type" @click="submit">
          {{ editing ? t('asset-record-form-save') : t('asset-record-form-save-new') }}
        </Button>
      </div>
    </template>
  </Modal>
</template>

<style>
/* Portalled, so not scoped. On a short screen the dialog may use nearly
   all of the height; the default 85vh leaves a phone on its side with
   three lines of form. */
@media (max-height: 500px) {
  .modal-panel.record-form-panel {
    max-height: calc(100dvh - 0.5rem) !important;
  }
}
</style>
