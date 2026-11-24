<script setup lang="ts">
/**
 * One entry of an asset's timeline, whatever its source.
 *
 * Every entry sits on the card's lines: the date at the left edge, a
 * narrow gutter, the text, and a figure or action at the right edge. It
 * has a main line and, when there is more to say, one line of facts
 * joined by middle dots.
 *
 * The gutter holds one kind of mark: the lifecycle state the asset moved
 * to, in the icon and colour that state has everywhere else in the app.
 * It appears on the entry that tells of the move, whatever its source (a
 * status change, the loan that put it On loan, the repair record that
 * moved it to In repair), and nowhere else. Read down the gutter and you
 * have the asset's status history; the rows between are the work.
 *
 * Words carry the kind ("Lent to Sam Okafor", "Moved to In service"), and
 * weight carries the hierarchy: a record's main line is medium and
 * primary, everything else regular and secondary.
 *
 * The row is not a button: the main line is, stretched over the row, so
 * the links and the Complete action inside it stay real controls.
 */
import { computed } from 'vue'
import { RouterLink } from 'vue-router'
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'
import { useUsersDirectory } from '@/composables/useUsersDirectory'
import { assetStatusLabel } from '@/utils/assetStatusMeta'
import { formatMoney } from '@nosdesk/core/utils/money'
import { ticketRoute, ticketRouteById } from '@nosdesk/core/utils/ticketRoutes'
import { numberForTicketId } from '@/composables/useTicketNumberLookup'
import type { Asset, AssetRecordType, AssetTimelineEntry, AssetTimelineTicket } from '@nosdesk/core/types/asset'
import {
  dayOf,
  formatClock,
  formatDay,
  outcomeCategory,
  outcomeName,
  readings,
  recordedLater,
  statusMark,
  typeName,
} from './assetTimeline'

const props = defineProps<{
  entry: AssetTimelineEntry
  /** The entry's record type, for a record or replacement. */
  type?: AssetRecordType
  /** The tag of the asset the entry is on, when it is a predecessor's. */
  predecessorTag?: string | null
  /** The asset that replaced this one, on the record that says so. */
  replacedBy?: Pick<Asset, 'id' | 'name' | 'asset_tag'> | null
  /** Staff may complete an open record from its row. */
  canEdit?: boolean
  /** Briefly marked after a save. */
  highlighted?: boolean
  /** The list is filtered to one record type, so rows needn't repeat it. */
  hideType?: boolean
  /** The record's linked ticket, said on the record instead of on a row
   *  of its own. */
  ticket?: AssetTimelineTicket | null
  /** The lifecycle state the asset moved to with this entry, if it moved:
   *  the entry's mark. A record or a loan carries it when the status
   *  change is folded into it. */
  movedTo?: string | null
  /** On the "Added to Nosdesk" marker: the state an import created the
   *  asset in, when the arrival was an import. */
  importedTo?: string | null
}>()

const emit = defineEmits<{
  open: [recordId: number]
  complete: [recordId: number]
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const { getUserHandle } = useUsersDirectory()

function person(uuid: string | null | undefined): string | null {
  if (!uuid) return null
  return getUserHandle(uuid).user.value?.name ?? null
}

const record = computed(() => (props.entry.source === 'record' ? props.entry.data : null))
const isOpen = computed(() => record.value !== null && record.value.completed_on === null)

/** The id of the record a click on this entry opens, if any. */
const opensRecord = computed<number | null>(() => {
  const e = props.entry
  if (e.source === 'record') return e.data.id
  if (e.source === 'replacement') return e.data.record_id
  return null
})

const title = computed(() => {
  const e = props.entry
  switch (e.source) {
    case 'record':
      return e.data.summary ?? typeName(t, props.type, e.data.record_type)
    case 'status':
      // The event an import writes for each asset it creates.
      if (!e.data.from_status && typeof e.data.metadata?.import_job_id === 'string') {
        return t('asset-lifecycle-timeline-imported', { to: assetStatusLabel(t, e.data.to_status) })
      }
      return t('asset-timeline-status-moved', { status: assetStatusLabel(t, e.data.to_status) })
    case 'loan': {
      const name = person(e.data.borrower_user_uuid) ?? t('asset-timeline-someone')
      if (e.kind === 'returned') return t('asset-timeline-loan-returned', { name })
      return e.data.due_back
        ? t('asset-timeline-loan-lent-due', { name, date: formatDay(e.data.due_back, 'monthDay') })
        : t('asset-timeline-loan-lent', { name })
    }
    case 'ticket':
      return e.data.title ?? t('asset-timeline-ticket-untitled')
    case 'replacement': {
      const other = e.data.other_asset
      const name = other ? (other.asset_tag ?? other.name) : t('asset-timeline-replacement-gone')
      return e.kind === 'replaces'
        ? t('asset-timeline-replaces', { name })
        : t('asset-timeline-replaced-by', { name })
    }
    default:
      if (e.kind === 'purchased') return t('asset-timeline-marker-purchased')
      return props.importedTo
        ? t('asset-lifecycle-timeline-imported', { to: assetStatusLabel(t, props.importedTo) })
        : t('asset-timeline-marker-added')
  }
})

interface Fact {
  key: string
  text: string
  /** `state` for work in progress, `failed` for an outcome that failed,
   *  `quiet` for a note about the record itself. */
  tone?: 'state' | 'failed' | 'quiet'
  /** A route the fact links to. */
  to?: string
  /** Hover text, for a link whose label is a number or a tag. */
  hint?: string
}

/** The line under the main one: short facts, in a fixed order. */
const facts = computed<Fact[]>(() => {
  const e = props.entry
  const out: Fact[] = []
  if (props.predecessorTag) out.push({ key: 'asset', text: props.predecessorTag })

  if (e.source === 'record') {
    const r = e.data
    if (r.summary !== null && !props.hideType) {
      out.push({ key: 'type', text: typeName(t, props.type, r.record_type) })
    }
    if (isOpen.value) {
      out.push({ key: 'state', text: t('asset-timeline-in-progress'), tone: 'state' })
      const back = r.attributes.expected_return
      if (typeof back === 'string' && back) {
        out.push({ key: 'back', text: t('asset-timeline-expected-back', { date: formatDay(back, 'monthDay') }) })
      }
    } else if (r.outcome) {
      out.push({
        key: 'outcome',
        text: outcomeName(t, props.type, r.outcome),
        tone: outcomeCategory(props.type, r.outcome) === 'not_done' ? 'failed' : undefined,
      })
    }
    const doneBy = r.vendor ?? person(r.performed_by_uuid)
    if (doneBy) out.push({ key: 'by', text: doneBy })
    if (r.reference) out.push({ key: 'reference', text: r.reference })
    if (r.cost && isOpen.value && props.canEdit) {
      out.push({ key: 'cost', text: formatMoney(r.cost.amount, r.cost.currency) })
    }
    for (const reading of readings(props.type, r)) out.push({ key: `reading-${reading}`, text: reading })
    const ticket = props.ticket
    if (ticket?.number !== undefined) {
      out.push({ key: 'ticket', text: `#${ticket.number}`, to: ticketRoute(ticket.number), hint: ticket.title })
    }
    if (props.movedTo) {
      out.push({ key: 'moved', text: t('asset-timeline-moved-to', { status: assetStatusLabel(t, props.movedTo) }) })
    }
    if (r.files.length) out.push({ key: 'files', text: t('asset-timeline-files', { count: r.files.length }) })
    if (props.replacedBy) {
      out.push({
        key: 'replaced',
        text: t('asset-timeline-replaced-by-fact', { name: props.replacedBy.asset_tag ?? props.replacedBy.name }),
        to: `/assets/${props.replacedBy.id}`,
        hint: props.replacedBy.name,
      })
    }
    if (recordedLater(e)) {
      const date = formatDay(dayOf(e.recorded_at))
      out.push({
        key: 'recorded',
        text:
          r.source === 'import'
            ? t('asset-timeline-imported-fact', { date })
            : t('asset-timeline-recorded-fact', { date }),
        tone: 'quiet',
      })
    }
  } else if (e.source === 'status') {
    if (e.data.reason) out.push({ key: 'reason', text: e.data.reason })
    // Who moved it and the ticket it was moved for: staff only.
    if (props.canEdit) {
      const actor = person(e.data.actor_uuid)
      if (actor) out.push({ key: 'actor', text: actor })
      const ticketId = e.data.ticket_id
      if (ticketId) {
        const number = numberForTicketId(ticketId)
        out.push({
          key: 'ticket',
          text: `#${number ?? ticketId}`,
          to: number !== undefined ? ticketRoute(number) : ticketRouteById(ticketId),
        })
      }
    }
    // The disposal written with the move, said on it.
    const disposal = e.data.disposal
    if (disposal) {
      out.push({
        key: 'method',
        text: t('asset-timeline-fact', {
          label: t('asset-disposal-method'),
          value: t(`asset-disposal-method-${disposal.sanitization_method}`),
        }),
      })
      if (disposal.data_bearing) out.push({ key: 'bearing', text: t('asset-disposal-data-bearing') })
      if (disposal.itad_vendor) out.push({ key: 'itad', text: disposal.itad_vendor })
      if (disposal.notes) out.push({ key: 'notes', text: disposal.notes, tone: 'quiet' })
    }
  } else if (e.source === 'ticket') {
    if (e.data.closed_at) {
      out.push({ key: 'closed', text: t('asset-timeline-ticket-closed', { date: formatDay(dayOf(e.data.closed_at)) }) })
    } else if (e.data.state) {
      out.push({ key: 'state', text: e.data.state.name })
    }
  } else if (e.source === 'replacement') {
    if (e.data.other_asset?.asset_tag) out.push({ key: 'name', text: e.data.other_asset.name })
  }
  return out
})

const cost = computed(() =>
  record.value?.cost ? formatMoney(record.value.cost.amount, record.value.cost.currency) : null,
)

/** The state this entry moved the asset to, as its gutter mark. */
const mark = computed(() => {
  if (!props.movedTo) return null
  const meta = statusMark(props.movedTo)
  return { icon: meta.icon, colorClass: meta.colorClass, label: t(meta.labelKey) }
})

/** Open work a viewer can complete: Complete takes the right edge, and a
 *  cost moves to the facts. */
const completable = computed(() => isOpen.value && !!props.canEdit)
const hasAside = computed(() => cost.value !== null || completable.value)

const dateText = computed(() => formatDay(props.entry.day, 'monthDay'))
/** The full date, and the time when it is known, for the date's tooltip. */
const whenTitle = computed(() => {
  const day = formatDay(props.entry.day)
  return props.entry.at ? `${day}, ${formatClock(props.entry.at)}` : day
})

/** Where the main line goes when it is a link rather than a record. */
const titleRoute = computed(() => {
  const e = props.entry
  if (e.source === 'ticket' && e.data.number !== undefined) return ticketRoute(e.data.number)
  if (e.source === 'replacement' && e.kind === 'replaces' && e.data.other_asset) {
    return `/assets/${e.data.other_asset.id}`
  }
  return null
})

const TONE: Record<NonNullable<Fact['tone']>, string> = {
  // Accent is kept for what can be clicked; a state is said in weight.
  state: 'font-medium text-primary',
  failed: 'text-status-error',
  quiet: 'text-tertiary',
}
</script>

<template>
  <li
    :data-entry="entry.key"
    class="relative transition-colors duration-700"
    :class="[opensRecord !== null && !titleRoute ? 'hover:bg-surface-hover/50' : '', highlighted ? 'bg-accent-muted' : '']"
  >
    <!-- The card's one grid: date, text, figure. The facts run under
         the figure too, so they get the row's full width. -->
    <div class="timeline-grid py-1.5">
      <time class="text-xs leading-5 text-tertiary tabular-nums whitespace-nowrap" :title="whenTitle">{{ dateText }}</time>

      <!-- 14px, the size of the text beside it, in the state's colour. -->
      <span
        v-if="mark"
        class="h-5 flex items-center"
        :class="mark.colorClass"
        role="img"
        :aria-label="mark.label"
        :title="mark.label"
      >
        <span class="w-3.5 h-3.5 flex"><Icon :name="mark.icon" class="w-full h-full" /></span>
      </span>
      <span v-else aria-hidden="true" />

      <p
        class="min-w-0 text-sm leading-5 [overflow-wrap:anywhere]"
        :class="[record ? 'font-medium text-primary' : 'text-secondary', hasAside ? '' : 'tl-text']"
      >
        <!-- Two lines at most; the record view has the rest. -->
        <button
          v-if="opensRecord !== null && !titleRoute"
          type="button"
          class="text-left line-clamp-2 after:absolute after:inset-0 focus:outline-none focus-visible:underline"
          @click="emit('open', opensRecord)"
        >{{ title }}</button>
        <RouterLink v-else-if="titleRoute" :to="titleRoute" class="hover:text-primary hover:underline">
          <template v-if="entry.source === 'ticket'"><span class="tabular-nums">#{{ entry.data.number }}</span>&nbsp;</template>{{ title }}
        </RouterLink>
        <template v-else>{{ title }}</template>
      </p>

      <!-- The right edge: what it cost, or the one thing to do. -->
      <div v-if="hasAside" class="text-sm leading-5 whitespace-nowrap text-right">
        <span v-if="!completable" class="tabular-nums text-primary">{{ cost }}</span>
        <button
          v-else-if="record"
          type="button"
          class="relative z-10 font-medium text-accent hover:underline pointer-coarse:after:absolute pointer-coarse:after:-inset-3"
          @click="emit('complete', record.id)"
        >{{ t('asset-timeline-complete') }}</button>
      </div>

      <p v-if="facts.length > 0" class="tl-text min-w-0 text-xs leading-5 text-secondary [overflow-wrap:anywhere]">
        <template v-for="(fact, i) in facts" :key="fact.key">
          <span v-if="i > 0" class="text-tertiary" aria-hidden="true"> · </span>
          <RouterLink
            v-if="fact.to"
            :to="fact.to"
            :title="fact.hint"
            class="relative z-10 underline decoration-strong underline-offset-2 hover:text-primary hover:decoration-current pointer-coarse:after:absolute pointer-coarse:after:-inset-2.5"
          >{{ fact.text }}</RouterLink>
          <span v-else :class="fact.tone ? TONE[fact.tone] : undefined">{{ fact.text }}</span>
        </template>
      </p>
    </div>
  </li>
</template>
