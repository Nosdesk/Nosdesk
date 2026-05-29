<script setup lang="ts">
/**
 * Visual 24h timeline of working hours for one day. Replaces the
 * prior text-input chip representation in WeekScheduleEditor.
 *
 * Two layers of edit:
 *   - **Drag for speed**: drag the left/right edge of a range to
 *     resize, drag the body to shift, click empty space to create.
 *     All drag movement snaps to 15-minute increments — enough for
 *     the common case of "shift this range by a chunk."
 *   - **Click for precision**: click a bar body (without dragging)
 *     to open a small popover with two TimePickers. The picker
 *     accepts any HH:MM via free-text entry, which is the path for
 *     workplaces that use off-quarter intervals (6-min education
 *     periods, 5-min retail blocks, custom shift patterns).
 *
 * Keyboard accessibility: each range bar is focusable. Arrow keys
 * shift the whole bar by 15 minutes; Shift+Arrow shifts only the
 * close edge; Alt+Arrow shifts only the open edge. Enter opens the
 * precision popover. Delete or Backspace removes the range.
 *
 * Invalid ranges (close <= open) still render but in the error
 * tone so the parent's `update:valid` watcher can disable Save.
 * The editor never silently drops bad data.
 *
 * Pointer events only (mouse + pen). Mobile touch handling is
 * deferred until the admin surface ships to mobile.
 */
import { computed, onBeforeUnmount, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'
import Popover from '@/components/common/Popover.vue'
import TimePicker from '@/components/common/TimePicker.vue'

interface Props {
  ranges: [string, string][]
  ariaLabel?: string
}

const props = defineProps<Props>()
const emit = defineEmits<{
  'update:ranges': [ranges: [string, string][]]
}>()

const fluent = useFluent()
const t = (k: string) => fluent.$t(k)

const SNAP_MINUTES = 15
const MIN_RANGE_MINUTES = 15
const DEFAULT_NEW_RANGE_MINUTES = 60
const KEY_NUDGE_MINUTES = 15
const MINUTES_PER_DAY = 24 * 60

const trackRef = ref<HTMLElement | null>(null)

function timeToMinutes(time: string): number {
  const m = /^(\d{1,2}):(\d{2})$/.exec(time)
  if (!m) return 0
  return Number(m[1]) * 60 + Number(m[2])
}

function minutesToTime(minutes: number): string {
  const clamped = Math.max(0, Math.min(MINUTES_PER_DAY, Math.round(minutes)))
  const h = Math.floor(clamped / 60)
  const mm = clamped % 60
  return `${String(h).padStart(2, '0')}:${String(mm).padStart(2, '0')}`
}

function snap(minutes: number): number {
  return Math.round(minutes / SNAP_MINUTES) * SNAP_MINUTES
}

function pctFromMinutes(minutes: number): number {
  return (minutes / MINUTES_PER_DAY) * 100
}

interface BarPos {
  index: number
  open: string
  close: string
  leftPct: number
  widthPct: number
  isValid: boolean
}

const bars = computed<BarPos[]>(() =>
  props.ranges.map((r, i) => {
    const start = timeToMinutes(r[0])
    const end = timeToMinutes(r[1])
    return {
      index: i,
      open: r[0],
      close: r[1],
      leftPct: pctFromMinutes(start),
      widthPct: Math.max(0, pctFromMinutes(end - start)),
      isValid: start < end,
    }
  }),
)

// ---------------- Drag ----------------

type DragMode = 'left' | 'right' | 'move'

interface DragState {
  index: number
  mode: DragMode
  startClientX: number
  startOpen: number
  startClose: number
  trackWidthPx: number
  moved: boolean
}

const drag = ref<DragState | null>(null)

function startDrag(index: number, mode: DragMode, event: PointerEvent): void {
  if (event.button !== 0) return
  event.preventDefault()
  event.stopPropagation()
  const range = props.ranges[index]
  if (!range || !trackRef.value) return
  const rect = trackRef.value.getBoundingClientRect()
  drag.value = {
    index,
    mode,
    startClientX: event.clientX,
    startOpen: timeToMinutes(range[0]),
    startClose: timeToMinutes(range[1]),
    trackWidthPx: rect.width,
    moved: false,
  }
  window.addEventListener('pointermove', onPointerMove)
  window.addEventListener('pointerup', endDrag)
  window.addEventListener('pointercancel', endDrag)
}

function onPointerMove(event: PointerEvent): void {
  const d = drag.value
  if (!d) return
  if (d.trackWidthPx <= 0) return
  const pxPerMinute = d.trackWidthPx / MINUTES_PER_DAY
  const deltaMinutes = snap((event.clientX - d.startClientX) / pxPerMinute)

  let newOpen = d.startOpen
  let newClose = d.startClose
  if (d.mode === 'left') {
    newOpen = Math.max(0, Math.min(d.startClose - MIN_RANGE_MINUTES, d.startOpen + deltaMinutes))
  } else if (d.mode === 'right') {
    newClose = Math.min(
      MINUTES_PER_DAY,
      Math.max(d.startOpen + MIN_RANGE_MINUTES, d.startClose + deltaMinutes),
    )
  } else {
    const width = d.startClose - d.startOpen
    newOpen = Math.max(0, Math.min(MINUTES_PER_DAY - width, d.startOpen + deltaMinutes))
    newClose = newOpen + width
  }

  if (newOpen === d.startOpen && newClose === d.startClose) return

  d.moved = true
  const next = props.ranges.map((r, i) =>
    i === d.index ? ([minutesToTime(newOpen), minutesToTime(newClose)] as [string, string]) : r,
  )
  emit('update:ranges', next)
}

function endDrag(): void {
  const d = drag.value
  // A body-mode "drag" that never produced any movement is really a
  // click — open the precision popover so the admin can type a
  // value that doesn't land on the 15-min snap grid.
  if (d && d.mode === 'move' && !d.moved) {
    openPopoverForIndex.value = d.index
  }
  drag.value = null
  window.removeEventListener('pointermove', onPointerMove)
  window.removeEventListener('pointerup', endDrag)
  window.removeEventListener('pointercancel', endDrag)
}

onBeforeUnmount(endDrag)

// ---------------- Precision popover ----------------

const openPopoverForIndex = ref<number | null>(null)
const barRefs = ref<Array<HTMLElement | null>>([])

function setBarRef(index: number, el: Element | unknown): void {
  if (el instanceof HTMLElement) {
    barRefs.value[index] = el
  }
}

const popoverAnchorEl = computed<HTMLElement | null>(() => {
  const idx = openPopoverForIndex.value
  if (idx === null) return null
  return barRefs.value[idx] ?? null
})

const activeRange = computed<[string, string] | null>(() => {
  const idx = openPopoverForIndex.value
  if (idx === null) return null
  return props.ranges[idx] ?? null
})

function setActiveRangePart(which: 0 | 1, value: string): void {
  const idx = openPopoverForIndex.value
  if (idx === null) return
  const range = props.ranges[idx]
  if (!range) return
  const next = props.ranges.map((r, i) =>
    i === idx ? ([which === 0 ? value : r[0], which === 1 ? value : r[1]] as [string, string]) : r,
  )
  emit('update:ranges', next)
}

// ---------------- Track click (create) ----------------

function handleTrackPointerDown(event: PointerEvent): void {
  if (event.button !== 0) return
  // If the pointerdown lands on a bar, the bar's own handler takes
  // over (and stopPropagates); this only fires for empty-track hits.
  if ((event.target as HTMLElement).closest('.day-timeline__bar')) return
  if (!trackRef.value) return
  const rect = trackRef.value.getBoundingClientRect()
  const clickMinutes = snap(((event.clientX - rect.left) / rect.width) * MINUTES_PER_DAY)
  const open = Math.max(0, Math.min(MINUTES_PER_DAY - DEFAULT_NEW_RANGE_MINUTES, clickMinutes))
  const close = open + DEFAULT_NEW_RANGE_MINUTES
  emit('update:ranges', [
    ...props.ranges,
    [minutesToTime(open), minutesToTime(close)] as [string, string],
  ])
}

// ---------------- Keyboard ----------------

function handleKeydown(index: number, event: KeyboardEvent): void {
  const range = props.ranges[index]
  if (!range) return
  let open = timeToMinutes(range[0])
  let close = timeToMinutes(range[1])
  const nudge = event.key === 'ArrowLeft' ? -KEY_NUDGE_MINUTES : KEY_NUDGE_MINUTES

  switch (event.key) {
    case 'ArrowLeft':
    case 'ArrowRight':
      event.preventDefault()
      if (event.shiftKey) {
        // Resize close edge.
        close = Math.min(
          MINUTES_PER_DAY,
          Math.max(open + MIN_RANGE_MINUTES, close + nudge),
        )
      } else if (event.altKey) {
        // Resize open edge.
        open = Math.max(0, Math.min(close - MIN_RANGE_MINUTES, open + nudge))
      } else {
        // Shift the whole range.
        const width = close - open
        open = Math.max(0, Math.min(MINUTES_PER_DAY - width, open + nudge))
        close = open + width
      }
      emit('update:ranges',
        props.ranges.map((r, i) =>
          i === index ? ([minutesToTime(open), minutesToTime(close)] as [string, string]) : r,
        ),
      )
      break
    case 'Enter':
    case ' ':
      // Open the precision popover from the keyboard. Space is the
      // ARIA convention for activating a focusable role=group child;
      // Enter is the natural pairing.
      event.preventDefault()
      openPopoverForIndex.value = index
      break
    case 'Delete':
    case 'Backspace':
      event.preventDefault()
      removeRange(index)
      break
  }
}

function removeRange(index: number): void {
  emit('update:ranges', props.ranges.filter((_, i) => i !== index))
}

// Hour markers at 0, 6, 12, 18 — enough to anchor the eye without
// stripey visual clutter. A subtle midnight tick at 24 doubles as
// the right edge of the track via the border.
const HOUR_MARKERS = [0, 6, 12, 18] as const
</script>

<template>
  <div
    ref="trackRef"
    class="day-timeline"
    role="group"
    :aria-label="ariaLabel"
    @pointerdown="handleTrackPointerDown"
  >
    <div class="day-timeline__ticks" aria-hidden="true">
      <div
        v-for="h in HOUR_MARKERS"
        :key="h"
        class="day-timeline__tick"
        :style="{ left: `${(h / 24) * 100}%` }"
      >
        <span class="day-timeline__tick-label">{{ h }}</span>
      </div>
    </div>

    <div
      v-for="bar in bars"
      :key="bar.index"
      :ref="(el) => setBarRef(bar.index, el)"
      class="day-timeline__bar"
      :class="{ 'day-timeline__bar--invalid': !bar.isValid }"
      :style="{ left: `${bar.leftPct}%`, width: `${bar.widthPct}%` }"
      :title="`${bar.open} – ${bar.close}`"
      :aria-label="`${bar.open} to ${bar.close}`"
      tabindex="0"
      @pointerdown="startDrag(bar.index, 'move', $event)"
      @keydown="handleKeydown(bar.index, $event)"
    >
      <button
        type="button"
        class="day-timeline__handle day-timeline__handle--left"
        :aria-label="t('admin-sla-schedule-resize-open-aria')"
        tabindex="-1"
        @pointerdown="startDrag(bar.index, 'left', $event)"
      />
      <span class="day-timeline__label">{{ bar.open }}–{{ bar.close }}</span>
      <button
        type="button"
        class="day-timeline__handle day-timeline__handle--right"
        :aria-label="t('admin-sla-schedule-resize-close-aria')"
        tabindex="-1"
        @pointerdown="startDrag(bar.index, 'right', $event)"
      />
      <button
        type="button"
        class="day-timeline__remove"
        :aria-label="t('admin-sla-schedule-remove-range-aria')"
        tabindex="-1"
        @click.stop="removeRange(bar.index)"
        @pointerdown.stop
      >
        <Icon name="close" class="w-2.5 h-2.5" />
      </button>
    </div>

    <Popover
      :open="openPopoverForIndex !== null"
      :anchor="{ type: 'element', element: () => popoverAnchorEl }"
      placement="bottom"
      role="dialog"
      :offset="6"
      :auto-focus="false"
      :aria-label="t('admin-sla-schedule-edit-range-aria')"
      popover-class="day-timeline__edit"
      @close="openPopoverForIndex = null"
    >
      <div
        v-if="activeRange"
        class="flex items-center gap-2"
        @pointerdown.stop
      >
        <TimePicker
          :model-value="activeRange[0]"
          :aria-label="t('admin-sla-schedule-resize-open-aria')"
          size="sm"
          @update:model-value="(v: string) => setActiveRangePart(0, v)"
        />
        <span class="text-tertiary text-xs" aria-hidden="true">→</span>
        <TimePicker
          :model-value="activeRange[1]"
          :aria-label="t('admin-sla-schedule-resize-close-aria')"
          size="sm"
          @update:model-value="(v: string) => setActiveRangePart(1, v)"
        />
      </div>
    </Popover>
  </div>
</template>

<style scoped>
.day-timeline {
  position: relative;
  height: 28px;
  background-color: var(--color-surface-alt);
  border: 1px solid var(--color-subtle);
  border-radius: 0.375rem;
  cursor: copy;
  user-select: none;
  touch-action: none;
}

.day-timeline:hover {
  border-color: var(--color-default);
}

.day-timeline__ticks {
  position: absolute;
  inset: 0;
  pointer-events: none;
}

.day-timeline__tick {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 1px;
  background-color: var(--color-default);
  opacity: 0.4;
}

/* Hide the leading 0-hour tick line — it overlaps the left border. */
.day-timeline__tick:first-child {
  background-color: transparent;
}

.day-timeline__tick-label {
  position: absolute;
  top: 50%;
  left: 4px;
  transform: translateY(-50%);
  font-size: 9px;
  font-variant-numeric: tabular-nums;
  color: var(--color-tertiary);
  opacity: 0.6;
  pointer-events: none;
}

.day-timeline__bar {
  position: absolute;
  top: 3px;
  bottom: 3px;
  background-color: var(--color-accent);
  border-radius: 0.25rem;
  cursor: grab;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  color: var(--color-on-accent);
  overflow: hidden;
  outline: none;
  transition: box-shadow 100ms ease, background-color 100ms ease;
  /* Make sure the bar paints above the ticks. */
  z-index: 1;
}

.day-timeline__bar:active {
  cursor: grabbing;
}

.day-timeline__bar:focus-visible {
  box-shadow: 0 0 0 2px var(--color-surface), 0 0 0 4px var(--color-accent);
}

.day-timeline__bar--invalid {
  background-color: var(--color-status-error);
}

.day-timeline__label {
  pointer-events: none;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: clip;
  padding: 0 8px;
}

.day-timeline__handle {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 6px;
  cursor: ew-resize;
  background-color: transparent;
  border: none;
  padding: 0;
}

.day-timeline__handle::before {
  content: '';
  position: absolute;
  top: 25%;
  bottom: 25%;
  left: 50%;
  width: 2px;
  transform: translateX(-50%);
  background-color: var(--color-on-accent);
  opacity: 0;
  border-radius: 1px;
  transition: opacity 100ms ease;
}

.day-timeline__bar:hover .day-timeline__handle::before,
.day-timeline__bar:focus-visible .day-timeline__handle::before {
  opacity: 0.7;
}

.day-timeline__handle--left {
  left: 0;
}

.day-timeline__handle--right {
  right: 0;
}

.day-timeline__remove {
  position: absolute;
  top: -6px;
  right: -6px;
  width: 14px;
  height: 14px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background-color: var(--color-surface);
  color: var(--color-tertiary);
  border: 1px solid var(--color-default);
  border-radius: 50%;
  cursor: pointer;
  opacity: 0;
  transition: opacity 100ms ease, color 100ms ease, border-color 100ms ease;
}

.day-timeline__bar:hover .day-timeline__remove,
.day-timeline__bar:focus-within .day-timeline__remove {
  opacity: 1;
}

.day-timeline__remove:hover {
  color: var(--color-status-error);
  border-color: var(--color-status-error);
}
</style>

<style>
/* Unscoped: the precision popover is teleported to <body>. Class
   prefix scopes the rule logically. Inner gap + padding only; the
   Popover primitive owns the border / shadow / background. */
.day-timeline__edit {
  background-color: var(--color-surface);
  border: 1px solid var(--color-default);
  border-radius: 0.5rem;
  box-shadow: 0 10px 25px -10px rgba(0, 0, 0, 0.2);
  padding: 0.5rem;
}
</style>
