<script setup lang="ts">
/**
 * One job in My day. A single row for every width, on four alignment
 * lines: the gutter (the time, or the grip of a job with none), the
 * state glyph, the title, and the size column on the right edge.
 *
 * The reference details (number, requester, a deadline on another day)
 * follow the title. Where the row is wide enough they sit beside it in
 * their own columns; where it is not they wrap under it, on the title's
 * line. That is the row's only response to width, and it is the browser's
 * wrap, not a breakpoint.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import SizeChip from '@/components/planning/SizeChip.vue'
import WorkflowStateGlyph from '@/components/views/WorkflowStateGlyph.vue'
import { useUsersDirectory } from '@/composables/useUsersDirectory'
import { priorityForBadge, priorityLabel } from '@/utils/priorityHelpers'
import { dueChipLabel, dueDay, formatDay, type Day } from '@nosdesk/core/utils/ticketDates'
import { formatWallTime, planTime, toHhmm } from '@nosdesk/core/utils/ticketPlan'
import { endMinutes, separateDueDay } from '@nosdesk/core/sync/views/planOrder'
import type { CardData } from '@nosdesk/core/sync/views/types'

const props = defineProps<{
  card: CardData
  today: Day
  /** The booked job this one starts before the end of. */
  overlap?: CardData
  /** The row the preview pane is showing. */
  selected?: boolean
  /** Being dragged: it stays in place, dimmed, until the drop. */
  lifted?: boolean
  timeOpen?: boolean
  /** A job outside the day's order (Overdue, No date): no grip, and its
   *  deadline shows even when it is the plan day. */
  fixed?: boolean
}>()

const emit = defineEmits<{
  (e: 'row-down', event: PointerEvent): void
  (e: 'grip-down', event: PointerEvent): void
  (e: 'open'): void
  (e: 'open-time', anchor: HTMLElement): void
  (e: 'size', minutes: number | null): void
  (e: 'menu', event: MouseEvent): void
  (e: 'move', by: -1 | 1): void
}>()

const fluent = useFluent()
const { getUserHandle } = useUsersDirectory()

const time = computed(() => planTime(props.card))
const timeLabel = computed(() => (time.value ? formatWallTime(time.value) : ''))
const requester = computed(() =>
  props.card.requester_uuid ? (getUserHandle(props.card.requester_uuid).user.value?.name ?? '') : '',
)

/** "due Fri", only when the deadline is another day than the plan day
 *  (any day, on a fixed row); in the danger color once that day has
 *  passed. */
const due = computed(() => {
  const day = props.fixed ? dueDay(props.card) : separateDueDay(props.card)
  if (!day) return null
  return {
    label: fluent.$t('plan-row-due', {
      day: dueChipLabel(day, props.today, {
        today: fluent.$t('tickets-due-today').toLowerCase(),
        tomorrow: fluent.$t('tickets-due-tomorrow').toLowerCase(),
      }),
    }),
    title: fluent.$t('ticket-detail-scheduling-due-prefix', { date: formatDay(day) }),
    late: day < props.today,
  }
})

const overlapTitle = computed(() =>
  props.overlap
    ? fluent.$t('plan-row-overlap', {
        number: String(props.overlap.number),
        time: formatWallTime(toHhmm(endMinutes(props.overlap))),
      })
    : '',
)

function onKeydown(event: KeyboardEvent): void {
  if (event.target !== event.currentTarget) return
  if (event.key === 'Enter') {
    // The row's own: the list's keys stand down on a handled event.
    event.preventDefault()
    emit('open')
  }
  if (event.altKey && event.shiftKey && (event.key === 'ArrowUp' || event.key === 'ArrowDown')) {
    event.preventDefault()
    emit('move', event.key === 'ArrowUp' ? -1 : 1)
  }
}
</script>

<template>
  <li
    :data-row="card.id"
    tabindex="0"
    class="plan-row plan-row-hold group relative flex items-start gap-3 border-b border-subtle/40 px-4 py-2.5 cursor-pointer transition-colors focus:outline-none focus-visible:bg-surface-hover"
    :class="[
      selected ? 'bg-accent/5 hover:bg-accent/10' : 'hover:bg-surface-hover',
      lifted && 'opacity-40',
    ]"
    @pointerdown="!fixed && emit('row-down', $event)"
    @click="fixed && emit('open')"
    @contextmenu.prevent="emit('menu', $event)"
    @keydown="onKeydown"
  >
    <!-- Gutter: the time of a booked job, the grip of any other. -->
    <div class="flex h-5 w-14 shrink-0 items-center">
      <button
        v-if="time"
        type="button"
        class="-mx-1 rounded px-1 text-xs tabular-nums whitespace-nowrap transition-colors hover:bg-surface-alt focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        :class="overlap ? 'text-status-warning' : 'text-secondary hover:text-primary'"
        aria-haspopup="dialog"
        :aria-expanded="!!timeOpen"
        :aria-label="$t('plan-row-time-aria', { number: String(card.number), time: timeLabel })"
        :title="overlapTitle || undefined"
        @pointerdown.stop
        @click.stop="emit('open-time', $event.currentTarget as HTMLElement)"
      >
        {{ timeLabel }}<span v-if="overlap" class="sr-only">. {{ overlapTitle }}</span>
      </button>
      <span
        v-else-if="!fixed"
        data-no-drag
        class="plan-grip -mx-1 inline-flex h-5 w-5 cursor-grab items-center justify-center rounded text-tertiary transition-opacity hover:text-secondary active:cursor-grabbing"
        role="img"
        :aria-label="$t('plan-row-move-aria', { number: String(card.number) })"
        @pointerdown.stop="emit('grip-down', $event)"
      >
        <Icon name="grip" size="sm" />
      </span>
    </div>

    <span class="flex h-5 shrink-0 items-center">
      <WorkflowStateGlyph
        :category="card.workflow_state.category"
        :color="card.workflow_state.color"
        :name="card.workflow_state.name"
      />
    </span>

    <!-- Title, then the reference details beside or under it. The
         details are one fixed-width group, so beside the title they form
         columns down the list; under it they start on the title's line. -->
    <div class="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-4 gap-y-0.5">
      <span
        class="min-w-0 flex-1 basis-64 truncate text-sm leading-5 font-medium text-primary"
        :title="card.title"
      >{{ card.title }}</span>
      <span class="flex min-w-0 shrink grow-0 basis-56 items-baseline gap-3 text-xs leading-5 text-tertiary">
        <span class="w-11 shrink-0 font-mono text-2xs tabular-nums">#{{ card.number }}</span>
        <span class="min-w-0 flex-1 truncate">{{ requester }}</span>
        <span
          v-if="due"
          class="shrink-0"
          :class="due.late && 'text-rose-600 dark:text-rose-400'"
          :title="due.title"
        >{{ due.label }}</span>
      </span>
    </div>

    <!-- The right edge: priority, then size. -->
    <span class="flex h-5 shrink-0 items-center gap-2">
      <span class="inline-flex w-4 justify-center" :title="priorityLabel(card.priority)">
        <PriorityIndicator
          v-if="priorityForBadge(card.priority)"
          :priority="priorityForBadge(card.priority)!"
          size="xs"
        />
      </span>
      <SizeChip
        :minutes="card.estimate_minutes"
        :number="card.number"
        @update="(minutes) => emit('size', minutes)"
      />
    </span>
  </li>
</template>
