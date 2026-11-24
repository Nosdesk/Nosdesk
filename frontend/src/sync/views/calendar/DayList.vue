<script setup lang="ts">
/**
 * A day, in full: its jobs in reading order (timed first, then the day's
 * order), then the dates from elsewhere that fall on it. One component
 * for every place a day is read whole: the agenda, the list a month cell
 * opens, the list under the month on a narrow screen, and the jobs with
 * no date.
 */
import { computed } from 'vue'
import Icon from '@/components/common/Icon.vue'
import type { DayJobs, WallClock } from '@nosdesk/core/sync/views/calendar'
import type { CardData } from '@nosdesk/core/sync/views/types'
import type { CalendarOverlay } from './calendarTypes'
import DayJobRow from './DayJobRow.vue'

const props = withDefaults(
  defineProps<{
    day: DayJobs
    overlays?: readonly CalendarOverlay[]
    clock: WallClock
    locale: string
    /** Words for a day with nothing on it. */
    empty: string
    /** Keep the time gutter whether or not this day has a timed job, so
     *  a run of days shares one title line. Left out, the day decides. */
    gutter?: boolean
    /** A job being carried: it stays in place, dimmed, until the drop. */
    draggedId?: number | null
    /** Rows can be carried to another day. */
    holdable?: boolean
  }>(),
  { overlays: () => [], gutter: undefined, draggedId: null, holdable: false },
)

const emit = defineEmits<{
  (e: 'open', id: number): void
  (e: 'patch', id: number, patch: Partial<CardData>): void
  (e: 'row-down', id: number, event: PointerEvent): void
}>()

const hasGutter = computed(() => props.gutter ?? props.day.timed > 0)
</script>

<template>
  <div class="flex flex-col">
    <ul v-if="day.jobs.length" class="flex flex-col border-t border-subtle/40">
      <DayJobRow
        v-for="card in day.jobs"
        :key="card.id"
        :card="card"
        :clock="clock"
        :locale="locale"
        :gutter="hasGutter"
        :lifted="draggedId === card.id"
        :holdable="holdable"
        @row-down="(event) => emit('row-down', card.id, event)"
        @open="emit('open', card.id)"
        @size="(minutes) => emit('patch', card.id, { estimate_minutes: minutes })"
      />
    </ul>

    <!-- Dates that are not jobs, on the rows' own lines: the mark where
         the state glyph sits, the words where the title does. -->
    <ul v-if="overlays.length" class="flex flex-col py-1.5" data-overlays>
      <li v-for="overlay in overlays" :key="overlay.id" class="flex items-start gap-3 px-4 py-1">
        <span v-if="hasGutter" class="w-14 shrink-0" />
        <span class="flex h-5 w-3.5 shrink-0 items-center justify-center text-tertiary">
          <Icon :name="overlay.kind === 'sla_breach' ? 'clock' : 'device'" size="xs" />
        </span>
        <RouterLink
          v-if="overlay.href"
          :to="overlay.href"
          class="min-w-0 text-xs leading-5 text-secondary hover:text-primary hover:underline"
        >{{ overlay.label }}</RouterLink>
        <span v-else class="min-w-0 text-xs leading-5 text-secondary">{{ overlay.label }}</span>
      </li>
    </ul>

    <p v-if="empty && !day.jobs.length && !overlays.length" class="px-4 py-3 text-sm text-tertiary">{{ empty }}</p>
  </div>
</template>
