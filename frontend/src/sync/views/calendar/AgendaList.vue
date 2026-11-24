<script setup lang="ts">
/**
 * The calendar as an agenda: the days that have something on them, one
 * after another, each a heading and that day's jobs in the rows a day's
 * list uses everywhere else. Days with nothing on them are left out;
 * today is always there. A day holding only another date (a warranty
 * running out) is listed too.
 *
 * One column, at most 720px wide, on the view's left edge, as My day is.
 * The time gutter is the list's and not the day's here, so every title
 * down the page starts on one line.
 *
 * The list opens with today at the top and the days just before it
 * already above, so scrolling up simply works. Each end brings in more as
 * it nears: earlier days arrive above without moving what is on screen,
 * later days below. The tickets are already held, so nothing waits and
 * there is nothing to press; the only marks are at the true ends, where
 * the list says there is nothing before its first day or after its last.
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import type { Day } from '@nosdesk/core/utils/ticketDates'
import type { CardData } from '@nosdesk/core/sync/views/types'
import { loadHours, loadMinutes } from '@nosdesk/core/utils/ticketPlan'
import {
  agendaDays,
  agendaWindow,
  dayTitle,
  dayTitleParts,
  jobsOn,
  windowToReveal,
  type DayJobs,
  type WallClock,
} from '@nosdesk/core/sync/views/calendar'
import type { CalendarOverlay } from './calendarTypes'
import DayList from './DayList.vue'

const props = defineProps<{
  byDay: ReadonlyMap<Day | null, DayJobs>
  overlaysByDay: ReadonlyMap<Day, CalendarOverlay[]>
  clock: WallClock
  locale: string
  /** Rows show their time and can be carried: off when anchored on a
   *  moment. */
  timed: boolean
  /** The day a carried job is over, and the job. */
  dropDay: Day | null
  draggedId: number | null
}>()

const emit = defineEmits<{
  (e: 'open', id: number): void
  (e: 'patch', id: number, patch: Partial<CardData>): void
  (e: 'row-down', id: number, event: PointerEvent): void
  /** The day at the top of what is on screen, and whether today is. */
  (e: 'position', top: Day, todayInView: boolean): void
}>()

const fluent = useFluent()

// --- Which days --------------------------------------------------------

/** Days brought in per step, counted in days that have something. */
const STEP = 7

const today = computed(() => props.clock.day)
const days = computed(() => agendaDays(props.byDay.keys(), props.overlaysByDay.keys(), today.value))

const before = ref(STEP)
const after = ref(STEP)
const view = computed(() => agendaWindow(days.value, today.value, before.value, after.value))
const firstDay = computed(() => days.value[0])
const lastDay = computed(() => days.value[days.value.length - 1])

function load(jobs: readonly CardData[]): string {
  const minutes = loadMinutes(jobs)
  return minutes > 0
    ? fluent.$t('plan-day-load-sized', { count: jobs.length, hours: loadHours(minutes) })
    : fluent.$t('plan-day-load', { count: jobs.length })
}

// --- The two ends ------------------------------------------------------

const scroller = ref<HTMLElement | null>(null)
const startMark = ref<HTMLElement | null>(null)
const endMark = ref<HTMLElement | null>(null)

/** Bring in more days before, keeping what is on screen where it is. */
async function growBefore(to: number): Promise<void> {
  const el = scroller.value
  if (!el || to <= before.value) return
  const height = el.scrollHeight
  const top = el.scrollTop
  before.value = to
  await nextTick()
  el.scrollTop = top + (el.scrollHeight - height)
  report()
}

let observer: IntersectionObserver | null = null

function observe(): void {
  observer?.disconnect()
  if (!scroller.value || typeof IntersectionObserver === 'undefined') return
  observer = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue
        if (entry.target === endMark.value && view.value.moreAhead) after.value += STEP
        if (entry.target === startMark.value && view.value.moreEarlier) void growBefore(before.value + STEP)
      }
    },
    { root: scroller.value, rootMargin: '240px 0px 240px 0px' },
  )
  if (startMark.value) observer.observe(startMark.value)
  if (endMark.value) observer.observe(endMark.value)
}

// --- Where the reader is -----------------------------------------------

let frame = 0
function report(): void {
  cancelAnimationFrame(frame)
  frame = requestAnimationFrame(() => {
    const el = scroller.value
    if (!el) return
    const box = el.getBoundingClientRect()
    let top: Day | null = null
    let todayInView = false
    for (const section of el.querySelectorAll<HTMLElement>('section[data-day]')) {
      const rect = section.getBoundingClientRect()
      if (rect.bottom <= box.top + 1) continue
      if (rect.top >= box.bottom) break
      top ??= section.dataset.day as Day
      if (section.dataset.day === today.value) todayInView = true
    }
    emit('position', top ?? today.value, top == null || todayInView)
  })
}

/** Room under the list so today can reach the top however short the
 *  days after it are. */
const tail = ref(0)

function sectionOf(day: Day): HTMLElement | null {
  return scroller.value?.querySelector<HTMLElement>(`section[data-day="${day}"]`) ?? null
}

function goToday(smooth = true): void {
  const el = scroller.value
  const section = sectionOf(today.value)
  if (!el || !section) return
  el.scrollTo({ top: section.offsetTop, behavior: smooth ? 'smooth' : 'instant' })
}

/** Bring a job's day on screen, scroll its row into view under the
 *  day's sticky heading, and put the focus on it. */
async function reveal(day: Day, id: number): Promise<void> {
  const next = windowToReveal(days.value, today.value, day, { before: before.value, after: after.value }, STEP)
  after.value = next.after
  await growBefore(next.before)
  await nextTick()
  const el = scroller.value
  const row = el?.querySelector<HTMLElement>(`[data-row="${id}"]`)
  const heading = sectionOf(day)?.querySelector<HTMLElement>('[data-day-heading]')
  if (el && row) {
    const top = el.scrollTop + row.getBoundingClientRect().top - el.getBoundingClientRect().top
    el.scrollTo({ top: Math.max(0, top - (heading?.offsetHeight ?? 0)), behavior: 'instant' })
    row.querySelector<HTMLElement>('[data-row-open]')?.focus({ preventScroll: true })
  }
  report()
}

defineExpose({ goToday, reveal })

onMounted(async () => {
  const el = scroller.value
  const list = el?.firstElementChild as HTMLElement | null
  const section = sectionOf(today.value)
  if (el && list && section) {
    tail.value = Math.max(0, section.offsetTop + el.clientHeight - list.offsetHeight)
    await nextTick()
    goToday(false)
  }
  observe()
  report()
})
watch(view, () => void nextTick(observe))
onBeforeUnmount(() => {
  observer?.disconnect()
  cancelAnimationFrame(frame)
})
</script>

<template>
  <div ref="scroller" class="relative min-h-0 flex-1 overflow-y-auto overscroll-contain" data-agenda @scroll.passive="report">
    <div class="flex w-full max-w-[45rem] flex-col pb-8" :style="{ marginBottom: `${tail}px` }">
      <!-- Where the list starts. -->
      <div ref="startMark" data-agenda-start>
        <p v-if="!view.moreEarlier && firstDay && firstDay < today" class="px-4 pt-4 text-xs leading-5 text-tertiary">
          {{ $t('calendar-agenda-start', { day: dayTitle(firstDay, locale) }) }}
        </p>
      </div>

      <section
        v-for="day in view.shown"
        :key="day"
        :data-day="day"
        :data-drop-day="timed ? day : undefined"
        :aria-labelledby="`agenda-${day}`"
        class="relative flex flex-col"
        :class="dropDay === day && 'after:pointer-events-none after:absolute after:inset-0 after:z-20 after:border-2 after:border-accent after:bg-accent/5'"
      >
        <div data-day-heading class="sticky top-0 z-10 flex flex-wrap items-baseline gap-x-3 bg-surface px-4 pt-4 pb-1.5">
          <!-- Today is marked on its number alone, as in the month: the
               accent fill under the on-accent ink. -->
          <h3 :id="`agenda-${day}`" class="text-sm leading-5 font-semibold text-primary">
            <template v-if="day === today">
              <template v-for="(part, i) in dayTitleParts(day, locale)" :key="i">
                <span
                  v-if="part.number"
                  class="inline-flex h-5 min-w-5 items-center justify-center rounded-full bg-accent px-1 text-on-accent tabular-nums"
                  data-today-number
                >{{ part.text }}</span>
                <template v-else>{{ part.text }}</template>
              </template>
              <span class="sr-only">, {{ $t('calendar-today') }}</span>
            </template>
            <template v-else>{{ dayTitle(day, locale) }}</template>
          </h3>
          <p v-if="jobsOn(byDay, day).jobs.length" class="text-xs leading-5 text-secondary tabular-nums">
            {{ load(jobsOn(byDay, day).jobs) }}
          </p>
        </div>
        <DayList
          :day="jobsOn(byDay, day)"
          :overlays="overlaysByDay.get(day)"
          :clock="clock"
          :locale="locale"
          :gutter="timed"
          :dragged-id="draggedId"
          :holdable="timed"
          :empty="$t('plan-day-empty')"
          @open="(id) => emit('open', id)"
          @patch="(id, patch) => emit('patch', id, patch)"
          @row-down="(id, event) => emit('row-down', id, event)"
        />
      </section>

      <!-- Where the list ends. A list that is only today has said so. -->
      <div ref="endMark" data-agenda-end>
        <p v-if="!view.moreAhead && lastDay && lastDay !== today" class="px-4 pt-5 text-xs leading-5 text-tertiary">
          {{ $t('calendar-agenda-end', { day: dayTitle(lastDay, locale) }) }}
        </p>
      </div>

      <slot name="after" />
    </div>
  </div>
</template>
