<script setup lang="ts">
/**
 * One asset record in full, as a single two-column list: a label, then
 * what it says. Everything is a row of that list, in one order: when,
 * how it ended, who, what it cost, the type's own fields, notes, files,
 * and last the second clock (when it was written down, and by whom).
 *
 * The same two columns at every width; a narrow dialog on desktop and a
 * sheet on phones show the same thing.
 */
import { computed, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { useFluent } from 'fluent-vue'
import DownloadLink from '@/components/common/DownloadLink.vue'
import { useUsersDirectory } from '@/composables/useUsersDirectory'
import { numberForTicketId } from '@/composables/useTicketNumberLookup'
import { formatDateTime } from '@nosdesk/core/utils/dateUtils'
import { formatMoney } from '@nosdesk/core/utils/money'
import { ticketRoute } from '@nosdesk/core/utils/ticketRoutes'
import type { Asset, AssetRecord, AssetRecordType } from '@nosdesk/core/types/asset'
import {
  agoText,
  dayOf,
  daysBetweenDays,
  fieldProps,
  fieldTitle,
  fieldValueText,
  fileSizeText,
  formatClock,
  formatDay,
  outcomeCategory,
  outcomeName,
  splitUnit,
  typeName,
} from './assetTimeline'

const props = defineProps<{
  record: AssetRecord
  type?: AssetRecordType
  /** The asset that replaced this one, when the record names it. */
  replacement?: Pick<Asset, 'id' | 'name' | 'asset_tag'> | null
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const { getUserHandle } = useUsersDirectory()

function person(uuid: string | null | undefined): string | null {
  return uuid ? (getUserHandle(uuid).user.value?.name ?? null) : null
}

interface Fact {
  key: string
  label: string
  value: string
  /** A quieter remark after the value. */
  note?: string
  to?: string
  tone?: 'state' | 'failed'
}

const facts = computed<Fact[]>(() => {
  const r = props.record
  const out: Fact[] = []
  const open = r.completed_on === null

  out.push({ key: 'type', label: t('asset-record-detail-type'), value: typeName(t, props.type, r.record_type) })
  const day = formatDay(r.occurred_on)
  out.push({
    key: 'when',
    label: open ? t('asset-record-detail-started') : t('asset-record-detail-work-done'),
    value: r.occurred_at ? `${day}, ${formatClock(r.occurred_at)}` : day,
    note: agoText(t, r.occurred_on),
  })
  if (r.completed_on) {
    const days = daysBetweenDays(r.occurred_on, r.completed_on)
    if (days > 0 || r.completed_at) {
      const done = formatDay(r.completed_on)
      out.push({
        key: 'completed',
        label: t('asset-record-detail-completed'),
        value: r.completed_at ? `${done}, ${formatClock(r.completed_at)}` : done,
        note: days > 0 ? t('asset-record-detail-span-days', { count: days }) : undefined,
      })
    }
  }
  if (open) {
    out.push({ key: 'outcome', label: t('admin-asset-record-types-core-outcome'), value: t('asset-timeline-in-progress'), tone: 'state' })
  } else if (r.outcome) {
    out.push({
      key: 'outcome',
      label: t('admin-asset-record-types-core-outcome'),
      value: outcomeName(t, props.type, r.outcome),
      tone: outcomeCategory(props.type, r.outcome) === 'not_done' ? 'failed' : undefined,
    })
  }
  if (props.replacement) {
    out.push({
      key: 'replacement',
      label: t('asset-record-form-replacement'),
      value: props.replacement.asset_tag ? `${props.replacement.asset_tag} ${props.replacement.name}` : props.replacement.name,
      to: `/assets/${props.replacement.id}`,
    })
  }
  if (r.vendor) out.push({ key: 'vendor', label: t('admin-asset-record-types-core-vendor'), value: r.vendor })
  const technician = person(r.performed_by_uuid)
  if (technician) out.push({ key: 'technician', label: t('asset-record-form-technician'), value: technician })
  if (r.reference) out.push({ key: 'reference', label: t('admin-asset-record-types-core-reference'), value: r.reference })
  if (r.cost) {
    out.push({
      key: 'cost',
      label: t('admin-asset-record-types-core-cost'),
      value: formatMoney(r.cost.amount, r.cost.currency),
    })
  }
  if (r.ticket_id) {
    const number = numberForTicketId(r.ticket_id)
    out.push({
      key: 'ticket',
      label: t('admin-asset-record-types-core-ticket'),
      value: `#${number ?? r.ticket_id}`,
      to: number !== undefined ? ticketRoute(number) : undefined,
    })
  }
  for (const [key, prop] of fieldProps(props.type)) {
    const value = fieldValueText(t, prop, r.attributes[key])
    if (!value || !props.type) continue
    const { name, unit } = splitUnit(fieldTitle(t, props.type, key, prop))
    out.push({ key: `field-${key}`, label: name, value: unit ? `${value} ${unit}` : value })
  }
  const sourceNote =
    r.snapshot_source === 'loan'
      ? t('asset-record-detail-snapshot-loan')
      : r.snapshot_source === 'current'
        ? t('asset-record-detail-snapshot-current')
        : undefined
  const holder = person(r.holder_uuid)
  if (holder) out.push({ key: 'holder', label: t('asset-record-detail-holder'), value: holder, note: sourceNote })
  if (r.location) {
    out.push({ key: 'location', label: t('asset-record-detail-location'), value: r.location, note: holder ? undefined : sourceNote })
  }
  return out
})

/** Where the record came from, in a few words. */
const origin = computed(() => {
  switch (props.record.source) {
    case 'import':
      return t('asset-record-detail-source-import')
    case 'transition':
      return t('asset-record-detail-source-transition')
    case 'api':
      return t('asset-record-detail-source-api')
    case 'rule':
      return t('asset-record-detail-source-rule')
    case 'backfill':
      return t('asset-record-detail-source-backfill')
    default:
      return undefined
  }
})

const recorded = computed(() => {
  const r = props.record
  const at = formatDateTime(r.recorded_at)
  const name = person(r.recorded_by)
  return name ? t('asset-record-detail-recorded-value-by', { date: at, name }) : at
})

const edited = computed(() => {
  const r = props.record
  if (r.version <= 1) return null
  const name = person(r.updated_by)
  const date = formatDay(dayOf(r.updated_at))
  return name ? t('asset-record-detail-recorded-value-by', { date, name }) : date
})

const FILES_FOLD = 5
const allFiles = ref(false)
const shownFiles = computed(() =>
  allFiles.value ? props.record.files : props.record.files.slice(0, FILES_FOLD),
)

const TONE = { state: 'font-medium', failed: 'text-status-error' } as const
</script>

<template>
  <div class="flex flex-col gap-3">
    <!-- What was done: the one line that isn't a row. -->
    <p v-if="record.summary" class="text-sm leading-5 font-medium text-primary [overflow-wrap:anywhere]">
      {{ record.summary }}
    </p>

    <dl class="record-list">
      <template v-for="fact in facts" :key="fact.key">
        <dt>{{ fact.label }}</dt>
        <dd>
          <RouterLink v-if="fact.to" :to="fact.to" class="underline decoration-strong underline-offset-2 hover:decoration-current">{{ fact.value }}</RouterLink>
          <span v-else :class="fact.tone ? TONE[fact.tone] : undefined">{{ fact.value }}</span>
          <span v-if="fact.note" class="text-tertiary">, {{ fact.note }}</span>
        </dd>
      </template>

      <template v-if="record.notes">
        <dt>{{ t('asset-record-form-notes') }}</dt>
        <dd class="whitespace-pre-wrap">{{ record.notes }}</dd>
      </template>

      <template v-if="record.files.length > 0">
        <dt>{{ t('asset-record-form-files') }}</dt>
        <dd>
          <ul class="flex flex-col">
            <li v-for="f in shownFiles" :key="f.id">
              <DownloadLink
                :href="f.url"
                :filename="f.name"
                spinner-size="xs"
                class="relative underline decoration-strong underline-offset-2 hover:decoration-current pointer-coarse:after:absolute pointer-coarse:after:-inset-y-1.5 pointer-coarse:after:inset-x-0"
              >{{ f.name }}</DownloadLink>
              <span class="text-tertiary tabular-nums whitespace-nowrap">, {{ fileSizeText(f.file_size) }}</span>
            </li>
          </ul>
          <!-- A long run of files starts folded, so the foot of the record
               stays within a scroll. -->
          <button
            v-if="record.files.length > FILES_FOLD"
            type="button"
            class="relative font-medium text-accent hover:underline pointer-coarse:after:absolute pointer-coarse:after:-inset-2.5"
            :aria-expanded="allFiles"
            @click="allFiles = !allFiles"
          >
            {{ allFiles ? t('asset-record-detail-files-fewer') : t('asset-record-detail-files-all', { count: record.files.length }) }}
          </button>
        </dd>
      </template>

      <!-- The second clock. -->
      <dt>{{ t('asset-record-detail-recorded-label') }}</dt>
      <dd>
        {{ recorded }}<span v-if="origin" class="text-tertiary">. {{ origin }}</span>
      </dd>
      <template v-if="edited">
        <dt>{{ t('asset-record-detail-edited-label') }}</dt>
        <dd>{{ edited }}</dd>
      </template>
    </dl>
  </div>
</template>

<style>
/* Two columns, the same at every width: labels on one line, values on
   the next. Rows keep a 20px rhythm. */
.record-list {
  display: grid;
  grid-template-columns: minmax(5.5rem, max-content) minmax(0, 1fr);
  column-gap: 1rem;
  row-gap: 0.375rem;
  font-size: 0.8125rem;
  line-height: 1.25rem;
}
.record-list > dt {
  color: var(--color-tertiary);
  max-width: 9rem;
}
.record-list > dd {
  min-width: 0;
  color: var(--color-primary);
  overflow-wrap: anywhere;
}
</style>
