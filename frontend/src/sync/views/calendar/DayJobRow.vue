<script setup lang="ts">
/**
 * One job in a day's list: My day's row (`agenda/AgendaRow.vue`), on the
 * same alignment lines and the same wrap, where the calendar has nothing
 * to arrange inside a day. The time is read, not set; there is no grip,
 * since the day's order is My day's to change; and the title carries the
 * calendar's one color, a job past its deadline, while a finished job
 * steps back.
 *
 * The gutter is the list's, not the row's: a list with no timed job has
 * no gutter at all.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import SizeChip from '@/components/planning/SizeChip.vue'
import WorkflowStateGlyph from '@/components/views/WorkflowStateGlyph.vue'
import { useUsersDirectory } from '@/composables/useUsersDirectory'
import { priorityForBadge, priorityLabel } from '@/utils/priorityHelpers'
import { dueChipLabel, formatDay, type Day } from '@nosdesk/core/utils/ticketDates'
import { formatWallTime, planTime } from '@nosdesk/core/utils/ticketPlan'
import { separateDueDay } from '@nosdesk/core/sync/views/planOrder'
import { isClosed, isLate } from '@nosdesk/core/sync/views/calendar'
import type { CardData } from '@nosdesk/core/sync/views/types'

const props = defineProps<{
  card: CardData
  today: Day
  locale: string
  /** The list has a time gutter. */
  gutter: boolean
  /** Being carried to another day: it stays in place, dimmed. */
  lifted?: boolean
  /** Can be carried to another day, so a hold picks it up. */
  holdable?: boolean
}>()

const emit = defineEmits<{
  (e: 'open'): void
  (e: 'row-down', event: PointerEvent): void
  (e: 'size', minutes: number | null): void
}>()

const fluent = useFluent()
const { getUserHandle } = useUsersDirectory()

const time = computed(() => (props.gutter ? planTime(props.card) : null))
const late = computed(() => isLate(props.card, props.today))
const closed = computed(() => isClosed(props.card))
const requester = computed(() =>
  props.card.requester_uuid ? (getUserHandle(props.card.requester_uuid).user.value?.name ?? '') : '',
)

/** "due Fri", only when the deadline is another day than the plan day. */
const due = computed(() => {
  const day: Day | null = separateDueDay(props.card)
  if (!day) return null
  return {
    label: fluent.$t('plan-row-due', {
      day: dueChipLabel(day, props.today, {
        today: fluent.$t('tickets-due-today').toLowerCase(),
        tomorrow: fluent.$t('tickets-due-tomorrow').toLowerCase(),
      }),
    }),
    title: fluent.$t('ticket-detail-scheduling-due-prefix', { date: formatDay(day) }),
  }
})
</script>

<template>
  <li
    :data-row="card.id"
    class="plan-row flex scroll-mt-12 cursor-pointer items-start gap-3 border-b border-subtle/40 px-4 py-2.5 transition-colors hover:bg-surface-hover has-[[data-row-open]:focus-visible]:bg-surface-hover"
    :class="[holdable && 'plan-row-hold', lifted && 'opacity-40']"
    @click="emit('open')"
    @pointerdown="emit('row-down', $event)"
  >
    <span v-if="gutter" class="flex h-5 w-14 shrink-0 items-center text-xs whitespace-nowrap text-secondary tabular-nums">
      {{ time ? formatWallTime(time, locale) : '' }}
    </span>

    <span class="flex h-5 shrink-0 items-center">
      <WorkflowStateGlyph
        :category="card.workflow_state.category"
        :color="card.workflow_state.color"
        :name="card.workflow_state.name"
      />
    </span>

    <div class="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-4 gap-y-0.5">
      <!-- The title is the row's open control. "Overdue" is said before
           it and kept out of the truncated line: a hidden span at the end
           of a clipped title sits past the edge and widens the page. -->
      <button
        type="button"
        data-row-open
        class="min-w-0 flex-1 basis-64 truncate text-left text-sm leading-5 focus:outline-none"
        :class="late ? 'font-medium text-overdue' : closed ? 'text-tertiary' : 'font-medium text-primary'"
        :title="card.title"
        @click.stop="emit('open')"
      ><span v-if="late" class="sr-only">{{ $t('calendar-job-overdue') }}: </span>{{ card.title }}</button>
      <span class="flex min-w-0 shrink grow-0 basis-56 items-baseline gap-3 text-xs leading-5 text-tertiary">
        <span class="w-11 shrink-0 font-mono text-2xs tabular-nums">#{{ card.number }}</span>
        <span class="min-w-0 flex-1 truncate">{{ requester }}</span>
        <span
          v-if="due"
          class="shrink-0"
          :class="late && 'text-overdue'"
          :title="due.title"
        >{{ due.label }}</span>
      </span>
    </div>

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
