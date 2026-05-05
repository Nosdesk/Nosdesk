<script setup lang="ts">
/**
 * Dense data table for the tickets list.
 *
 * Presentational. The parent owns sort state, column layout,
 * density, grouping, and the cards array. This component
 * renders the `<table>`, the per-column cell types, and the
 * group header rows when grouping is active.
 *
 * Visual hierarchy choices (informed by Linear / Plain / Height):
 *
 * - Each row has a 3px leading "tone strip" that encodes SLA
 *   urgency (red breached, amber at-risk). Reads as a peripheral
 *   urgency indicator without burning a column.
 * - Title is `font-medium text-primary`; metadata cells are
 *   `text-tertiary` and a step smaller. Strong title weight is
 *   what lets a tech scan a queue of 30+ tickets.
 * - Title cell carries a leading workflow-state colour dot, so
 *   the row's status is visible at a glance even when the
 *   Status column is hidden.
 * - Priority shows a coloured dot + label in the title row when
 *   not 'none', so urgent / high are visible without needing
 *   the Priority column to be enabled.
 *
 * SSE-optimised: each `<tr>` is wrapped in v-memo against the
 * subset of CardData fields the row actually renders, so an SSE
 * burst that touches N tickets re-renders N rows, not the whole
 * visible list.
 *
 * `table-layout: fixed` makes the inline column widths
 * authoritative — cells respect them even when content overflows,
 * keeping the resize / reorder interactions predictable.
 */
import { computed } from 'vue'
import Icon from '@/components/common/Icon.vue'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import UserCell from '@/components/views/UserCell.vue'
import { paletteForColor } from '@/utils/workflowColors'
import { rowMemoKey, type ListColumn } from '@/sync/views/ticketColumns'
import type { useColumnLayout } from '@/composables/useColumnLayout'
import type { CardData } from '@/sync/views/types'
import type { GroupBucket } from '@/composables/useTicketsGrouping'

const props = defineProps<{
  cards: CardData[]
  visibleColumns: ListColumn[]
  rowClass: string
  cellPadding: string
  sortField: string
  sortDir: 'asc' | 'desc'
  layout: ReturnType<typeof useColumnLayout>
  colStyle: (col: ListColumn) => Record<string, string>
  /** When non-empty, the body renders bucket headers + cards
   * grouped under each. When empty, the body is a flat list. */
  buckets: GroupBucket[]
  isCollapsed: (key: string) => boolean
  /** Currently-selected ticket id (split-view mode). Drives the
   * "selected row" visual treatment — distinct from hover so
   * users can see the row that's powering the preview pane. */
  selectedId?: number | null
}>()

const emit = defineEmits<{
  (e: 'open', id: number): void
  (e: 'select', id: number): void
  (e: 'toggle-sort', field: string): void
  (e: 'toggle-bucket', key: string): void
}>()

/** When split-view is on, single-click selects (preview).
 * When off, single-click opens (full route navigation).
 * The shell decides which by binding `selectedId` —
 * presence of the prop signals split-view mode. */
const isSplitMode = computed<boolean>(
  () => props.selectedId !== undefined,
)

function onRowClick(id: number): void {
  if (isSplitMode.value) emit('select', id)
  else emit('open', id)
}

const grouped = computed<boolean>(() => props.buckets.length > 0)

function priorityForBadge(p: CardData['priority']): 'low' | 'medium' | 'high' | null {
  if (p === 'urgent') return 'high'
  if (p === 'low' || p === 'medium' || p === 'high') return p
  return null
}

function inlinePriorityClass(p: CardData['priority']): string | null {
  if (p === 'urgent') return 'text-rose-500'
  if (p === 'high') return 'text-orange-500'
  return null
}

function relativeTime(iso: string): string {
  const then = new Date(iso).getTime()
  const seconds = Math.max(1, Math.round((Date.now() - then) / 1000))
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return `${hours}h`
  const days = Math.round(hours / 24)
  if (days < 30) return `${days}d`
  return new Date(iso).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

function shortDate(iso: string | null | undefined): string {
  if (!iso) return '—'
  return new Date(iso).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

function recurrenceLabel(rule: string): string {
  const freq = rule
    .split(';')
    .map((p) => p.trim())
    .find((p) => p.toUpperCase().startsWith('FREQ='))
    ?.split('=')[1]
    ?.toUpperCase()
  if (freq === 'DAILY') return 'D'
  if (freq === 'WEEKLY') return 'W'
  if (freq === 'MONTHLY') return 'M'
  if (freq === 'YEARLY') return 'Y'
  return '↻'
}

function slaToneClass(card: CardData): string {
  const sla = card.sla
  if (!sla) return 'text-tertiary'
  if (sla.breached) return 'text-rose-600 dark:text-rose-400'
  if (sla.pill_color === 'amber') return 'text-amber-600 dark:text-amber-400'
  if (sla.pill_color === 'green') return 'text-emerald-600 dark:text-emerald-400'
  return 'text-tertiary'
}

function slaLabel(card: CardData): string {
  const sla = card.sla
  if (!sla) return '—'
  if (sla.breached) return 'Breached'
  if (sla.paused) return 'Paused'
  const remaining = sla.seconds_remaining ?? 0
  if (remaining < 3600) return `${Math.ceil(remaining / 60)}m`
  if (remaining < 86_400) return `${Math.ceil(remaining / 3600)}h`
  return `${Math.ceil(remaining / 86_400)}d`
}

/** Leading 3px row stripe encoding SLA urgency. Returns the bg
 * class for the strip cell — empty string when the row has no
 * SLA tone to communicate. */
function rowToneClass(card: CardData): string {
  const sla = card.sla
  if (!sla) return ''
  if (sla.breached) return 'bg-rose-500'
  if (sla.pill_color === 'amber') return 'bg-amber-500'
  return ''
}
</script>

<template>
  <div class="flex-1 min-h-0 overflow-auto">
    <table class="w-full text-sm border-separate border-spacing-0 table-fixed">
      <thead>
        <tr class="sticky top-0 z-10 bg-surface">
          <th
            v-for="col in visibleColumns"
            :key="col.id"
            :draggable="layout.isReorderable(col.id)"
            class="relative text-left text-[10px] font-semibold text-tertiary uppercase tracking-wider border-b border-subtle bg-surface select-none"
            :class="[
              cellPadding,
              col.align === 'center' && 'text-center',
              col.align === 'right' && 'text-right',
              layout.isReorderable(col.id) && 'cursor-grab',
              layout.dragSourceId.value === col.id && 'opacity-50',
              layout.dragTargetId.value === col.id && 'bg-accent/10',
            ]"
            :style="colStyle(col)"
            @dragstart="layout.onDragStart(col.id, $event)"
            @dragover="layout.onDragOver(col.id, $event)"
            @dragleave="layout.onDragLeave(col.id)"
            @drop="layout.onDrop(col.id, $event)"
            @dragend="layout.onDragEnd"
          >
            <button
              v-if="col.sortKey"
              type="button"
              class="inline-flex items-center gap-1 hover:text-primary transition-colors"
              @click="emit('toggle-sort', col.sortKey!)"
            >
              {{ col.label }}
              <span v-if="sortField === col.sortKey" class="text-[10px] leading-none">
                {{ sortDir === 'asc' ? '↑' : '↓' }}
              </span>
            </button>
            <span v-else>{{ col.label }}</span>
            <span
              class="absolute top-0 right-0 h-full w-1.5 cursor-col-resize touch-none group/resize"
              :class="layout.resizingId.value === col.id && 'bg-accent/50'"
              @pointerdown="layout.beginResize(col.id, $event)"
              @click.stop
            >
              <span
                class="absolute inset-y-1 right-0.5 w-px bg-transparent group-hover/resize:bg-accent/40 transition-colors"
              />
            </span>
          </th>
        </tr>
      </thead>
      <tbody>
        <!-- Flat (ungrouped) list -->
        <template v-if="!grouped">
          <tr
            v-for="card in cards"
            :key="card.id"
            v-memo="[...rowMemoKey(card), visibleColumns.length, cellPadding, rowClass, selectedId === card.id]"
            class="cursor-pointer transition-colors group relative"
            :class="[
              rowClass,
              selectedId === card.id
                ? 'bg-accent/10 hover:bg-accent/15'
                : 'hover:bg-surface-hover',
            ]"
            @click="onRowClick(card.id)"
          >
            <td
              v-for="(col, ci) in visibleColumns"
              :key="col.id"
              class="border-b border-subtle/40 align-middle relative"
              :class="[
                cellPadding,
                col.align === 'center' && 'text-center',
                col.align === 'right' && 'text-right',
              ]"
              :style="colStyle(col)"
            >
              <!-- SLA tone strip on the leftmost cell only.
                   Absolute-positioned 3px bar; empty when the
                   row has no urgency signal so we don't spend
                   visual budget on a transparent strip. -->
              <span
                v-if="ci === 0 && rowToneClass(card)"
                class="absolute left-0 top-0 bottom-0 w-[3px]"
                :class="rowToneClass(card)"
                aria-hidden="true"
              />

              <template v-if="col.id === 'id'">
                <span class="text-tertiary font-mono text-[11px] tabular-nums">#{{ card.id }}</span>
              </template>

              <template v-else-if="col.id === 'title'">
                <div class="flex items-center gap-2 min-w-0">
                  <span
                    class="inline-block w-2 h-2 rounded-full shrink-0"
                    :class="paletteForColor(card.workflow_state.color).solid"
                    :title="card.workflow_state.name"
                    aria-hidden="true"
                  />
                  <span
                    v-if="inlinePriorityClass(card.priority)"
                    class="text-[11px] leading-none font-bold shrink-0"
                    :class="inlinePriorityClass(card.priority)!"
                    :title="`Priority: ${card.priority}`"
                  >!</span>
                  <span
                    v-if="card.recurrence_rule"
                    class="text-tertiary text-xs leading-none shrink-0"
                    title="Recurring ticket"
                  >↻</span>
                  <span
                    class="truncate font-medium text-primary"
                    :title="card.title"
                  >{{ card.title }}</span>
                  <span
                    v-if="card.sla?.breached"
                    class="text-[10px] font-semibold uppercase tracking-wide text-rose-600 dark:text-rose-400 shrink-0"
                    title="SLA breached"
                  >SLA</span>
                </div>
              </template>

              <template v-else-if="col.id === 'workflow_state'">
                <span class="inline-flex items-center gap-1.5 text-xs text-secondary">
                  <span
                    class="inline-block w-2 h-2 rounded-full shrink-0"
                    :class="paletteForColor(card.workflow_state.color).solid"
                    aria-hidden="true"
                  />
                  <span class="truncate">{{ card.workflow_state.name }}</span>
                </span>
              </template>

              <template v-else-if="col.id === 'priority'">
                <PriorityIndicator
                  v-if="priorityForBadge(card.priority)"
                  :priority="priorityForBadge(card.priority)!"
                  size="xs"
                />
                <span v-else class="text-xs text-tertiary">—</span>
              </template>

              <template v-else-if="col.id === 'assignee'">
                <UserCell :uuid="card.assignee_uuid" />
              </template>

              <template v-else-if="col.id === 'requester'">
                <UserCell :uuid="card.requester_uuid" />
              </template>

              <template v-else-if="col.id === 'category'">
                <span
                  v-if="card.category_id != null"
                  class="text-[11px] text-secondary bg-surface-hover rounded px-1.5 py-0.5"
                >#{{ card.category_id }}</span>
                <span v-else class="text-xs text-tertiary">—</span>
              </template>

              <template v-else-if="col.id === 'cycle'">
                <span
                  v-if="card.cycle_id != null"
                  class="text-[11px] text-accent bg-accent/10 rounded px-1.5 py-0.5"
                  title="Belongs to a cycle"
                >cycle #{{ card.cycle_id }}</span>
                <span v-else class="text-xs text-tertiary">—</span>
              </template>

              <template v-else-if="col.id === 'due_date'">
                <span
                  class="text-[11px] tabular-nums"
                  :class="card.due_date ? 'text-secondary' : 'text-tertiary'"
                  :title="card.due_date ? new Date(card.due_date).toLocaleString() : 'No due date'"
                >{{ shortDate(card.due_date) }}</span>
              </template>

              <template v-else-if="col.id === 'last_activity'">
                <span
                  class="text-[11px] text-tertiary tabular-nums"
                  :title="new Date(card.last_activity_at).toLocaleString()"
                >{{ relativeTime(card.last_activity_at) }}</span>
              </template>

              <template v-else-if="col.id === 'created_at'">
                <span
                  class="text-[11px] text-tertiary tabular-nums"
                  :title="new Date(card.created_at).toLocaleString()"
                >{{ relativeTime(card.created_at) }}</span>
              </template>

              <template v-else-if="col.id === 'sla'">
                <span
                  v-if="card.sla"
                  class="inline-flex items-center gap-1 text-[11px] tabular-nums"
                  :class="slaToneClass(card)"
                  :title="card.sla.breached ? 'Breached' : (card.sla.paused ? 'Paused' : 'On track')"
                >
                  <Icon name="clock" class="w-3 h-3" />
                  {{ slaLabel(card) }}
                </span>
                <span v-else class="text-xs text-tertiary">—</span>
              </template>

              <template v-else-if="col.id === 'kb_gap'">
                <span
                  v-if="card.kb_gap_signal && card.kb_gap_signal !== 'none'"
                  class="text-[10px] font-semibold uppercase tracking-wide rounded px-1.5 py-0.5"
                  :class="card.kb_gap_signal === 'strong'
                    ? 'bg-amber-500/20 text-amber-700 dark:text-amber-300'
                    : 'bg-surface-hover text-secondary'"
                  :title="`${card.kb_gap_signal} knowledge gap signal`"
                >KB</span>
                <span v-else class="text-xs text-tertiary">—</span>
              </template>

              <template v-else-if="col.id === 'devices'">
                <span
                  v-if="card.affected_devices && card.affected_devices.count > 0"
                  class="text-[11px] text-secondary tabular-nums inline-flex items-center gap-1"
                  :title="card.affected_devices.first?.name ?? `${card.affected_devices.count} device(s)`"
                >
                  <Icon name="device" class="w-3 h-3" />
                  {{ card.affected_devices.count }}
                </span>
                <span v-else class="text-xs text-tertiary">—</span>
              </template>

              <template v-else-if="col.id === 'recurrence'">
                <span
                  v-if="card.recurrence_rule"
                  class="text-[10px] font-medium rounded px-1.5 py-0.5 bg-violet-500/15 text-violet-700 dark:text-violet-300"
                  :title="card.recurrence_rule"
                >{{ recurrenceLabel(card.recurrence_rule) }}</span>
                <span v-else class="text-xs text-tertiary">—</span>
              </template>
            </td>
          </tr>
        </template>

        <!-- Grouped list. Each bucket renders a sticky header row
             with chevron toggle + label + count, then the bucket's
             cards (or nothing when collapsed). -->
        <template v-else>
          <template v-for="bucket in buckets" :key="bucket.key">
            <tr
              class="bg-surface/50 border-b border-subtle cursor-pointer select-none hover:bg-surface-hover transition-colors"
              @click="emit('toggle-bucket', bucket.key)"
            >
              <td
                :colspan="visibleColumns.length"
                class="px-3 py-1.5"
              >
                <div class="flex items-center gap-2">
                  <Icon
                    name="chevronDown"
                    class="w-3 h-3 text-tertiary transition-transform"
                    :class="isCollapsed(bucket.key) && '-rotate-90'"
                  />
                  <span class="text-xs font-semibold text-primary">{{ bucket.label }}</span>
                  <span class="text-[11px] text-tertiary tabular-nums">{{ bucket.cards.length }}</span>
                </div>
              </td>
            </tr>
            <template v-if="!isCollapsed(bucket.key)">
              <tr
                v-for="card in bucket.cards"
                :key="`${bucket.key}:${card.id}`"
                v-memo="[...rowMemoKey(card), visibleColumns.length, cellPadding, rowClass, selectedId === card.id]"
                class="cursor-pointer transition-colors group"
                :class="[
                  rowClass,
                  selectedId === card.id
                    ? 'bg-accent/10 hover:bg-accent/15'
                    : 'hover:bg-surface-hover',
                ]"
                @click="onRowClick(card.id)"
              >
                <td
                  v-for="(col, ci) in visibleColumns"
                  :key="col.id"
                  class="border-b border-subtle/40 align-middle relative"
                  :class="[
                    cellPadding,
                    col.align === 'center' && 'text-center',
                    col.align === 'right' && 'text-right',
                  ]"
                  :style="colStyle(col)"
                >
                  <span
                    v-if="ci === 0 && rowToneClass(card)"
                    class="absolute left-0 top-0 bottom-0 w-[3px]"
                    :class="rowToneClass(card)"
                    aria-hidden="true"
                  />

                  <template v-if="col.id === 'id'">
                    <span class="text-tertiary font-mono text-[11px] tabular-nums">#{{ card.id }}</span>
                  </template>

                  <template v-else-if="col.id === 'title'">
                    <div class="flex items-center gap-2 min-w-0">
                      <span
                        class="inline-block w-2 h-2 rounded-full shrink-0"
                        :class="paletteForColor(card.workflow_state.color).solid"
                        :title="card.workflow_state.name"
                        aria-hidden="true"
                      />
                      <span
                        v-if="inlinePriorityClass(card.priority)"
                        class="text-[11px] leading-none font-bold shrink-0"
                        :class="inlinePriorityClass(card.priority)!"
                        :title="`Priority: ${card.priority}`"
                      >!</span>
                      <span
                        v-if="card.recurrence_rule"
                        class="text-tertiary text-xs leading-none shrink-0"
                        title="Recurring ticket"
                      >↻</span>
                      <span
                        class="truncate font-medium text-primary"
                        :title="card.title"
                      >{{ card.title }}</span>
                      <span
                        v-if="card.sla?.breached"
                        class="text-[10px] font-semibold uppercase tracking-wide text-rose-600 dark:text-rose-400 shrink-0"
                        title="SLA breached"
                      >SLA</span>
                    </div>
                  </template>

                  <template v-else-if="col.id === 'workflow_state'">
                    <span class="inline-flex items-center gap-1.5 text-xs text-secondary">
                      <span
                        class="inline-block w-2 h-2 rounded-full shrink-0"
                        :class="paletteForColor(card.workflow_state.color).solid"
                        aria-hidden="true"
                      />
                      <span class="truncate">{{ card.workflow_state.name }}</span>
                    </span>
                  </template>

                  <template v-else-if="col.id === 'priority'">
                    <PriorityIndicator
                      v-if="priorityForBadge(card.priority)"
                      :priority="priorityForBadge(card.priority)!"
                      size="xs"
                    />
                    <span v-else class="text-xs text-tertiary">—</span>
                  </template>

                  <template v-else-if="col.id === 'assignee'">
                    <UserCell :uuid="card.assignee_uuid" />
                  </template>

                  <template v-else-if="col.id === 'requester'">
                    <UserCell :uuid="card.requester_uuid" />
                  </template>

                  <template v-else-if="col.id === 'category'">
                    <span
                      v-if="card.category_id != null"
                      class="text-[11px] text-secondary bg-surface-hover rounded px-1.5 py-0.5"
                    >#{{ card.category_id }}</span>
                    <span v-else class="text-xs text-tertiary">—</span>
                  </template>

                  <template v-else-if="col.id === 'cycle'">
                    <span
                      v-if="card.cycle_id != null"
                      class="text-[11px] text-accent bg-accent/10 rounded px-1.5 py-0.5"
                      title="Belongs to a cycle"
                    >cycle #{{ card.cycle_id }}</span>
                    <span v-else class="text-xs text-tertiary">—</span>
                  </template>

                  <template v-else-if="col.id === 'due_date'">
                    <span
                      class="text-[11px] tabular-nums"
                      :class="card.due_date ? 'text-secondary' : 'text-tertiary'"
                      :title="card.due_date ? new Date(card.due_date).toLocaleString() : 'No due date'"
                    >{{ shortDate(card.due_date) }}</span>
                  </template>

                  <template v-else-if="col.id === 'last_activity'">
                    <span
                      class="text-[11px] text-tertiary tabular-nums"
                      :title="new Date(card.last_activity_at).toLocaleString()"
                    >{{ relativeTime(card.last_activity_at) }}</span>
                  </template>

                  <template v-else-if="col.id === 'created_at'">
                    <span
                      class="text-[11px] text-tertiary tabular-nums"
                      :title="new Date(card.created_at).toLocaleString()"
                    >{{ relativeTime(card.created_at) }}</span>
                  </template>

                  <template v-else-if="col.id === 'sla'">
                    <span
                      v-if="card.sla"
                      class="inline-flex items-center gap-1 text-[11px] tabular-nums"
                      :class="slaToneClass(card)"
                      :title="card.sla.breached ? 'Breached' : (card.sla.paused ? 'Paused' : 'On track')"
                    >
                      <Icon name="clock" class="w-3 h-3" />
                      {{ slaLabel(card) }}
                    </span>
                    <span v-else class="text-xs text-tertiary">—</span>
                  </template>

                  <template v-else-if="col.id === 'kb_gap'">
                    <span
                      v-if="card.kb_gap_signal && card.kb_gap_signal !== 'none'"
                      class="text-[10px] font-semibold uppercase tracking-wide rounded px-1.5 py-0.5"
                      :class="card.kb_gap_signal === 'strong'
                        ? 'bg-amber-500/20 text-amber-700 dark:text-amber-300'
                        : 'bg-surface-hover text-secondary'"
                      :title="`${card.kb_gap_signal} knowledge gap signal`"
                    >KB</span>
                    <span v-else class="text-xs text-tertiary">—</span>
                  </template>

                  <template v-else-if="col.id === 'devices'">
                    <span
                      v-if="card.affected_devices && card.affected_devices.count > 0"
                      class="text-[11px] text-secondary tabular-nums inline-flex items-center gap-1"
                      :title="card.affected_devices.first?.name ?? `${card.affected_devices.count} device(s)`"
                    >
                      <Icon name="device" class="w-3 h-3" />
                      {{ card.affected_devices.count }}
                    </span>
                    <span v-else class="text-xs text-tertiary">—</span>
                  </template>

                  <template v-else-if="col.id === 'recurrence'">
                    <span
                      v-if="card.recurrence_rule"
                      class="text-[10px] font-medium rounded px-1.5 py-0.5 bg-violet-500/15 text-violet-700 dark:text-violet-300"
                      :title="card.recurrence_rule"
                    >{{ recurrenceLabel(card.recurrence_rule) }}</span>
                    <span v-else class="text-xs text-tertiary">—</span>
                  </template>
                </td>
              </tr>
            </template>
          </template>
        </template>
      </tbody>
    </table>
  </div>
</template>
