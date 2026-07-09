<script setup lang="ts">
/**
 * Presentational disc for pull-to-refresh. Purely visual — gesture
 * state comes in as props, announcements live in `PullToRefresh.vue`.
 *
 * One continuous stroke lives through the whole gesture:
 *
 *   pulling    → the arc draws with the finger (0–300° at threshold)
 *                and keeps rotating with it past the arm point
 *   armed      → the disc pops once, in sync with the haptic tick
 *   refreshing → the same arc spins, starting from the exact angle
 *                the finger left it (no swap, no snap)
 *   done       → the ring closes to 360° and a check strokes in:
 *                open arc = working, closed ring = done
 *
 * Under `prefers-reduced-motion`: no spin, no pop, no draw-on — a
 * static arc and an instant check; the aria-live region carries state.
 */
import { computed, ref, watch } from 'vue'
import type { PullToRefreshState } from '@/composables/usePullToRefresh'

const props = defineProps<{
  state: PullToRefreshState
  /** 0..1 pull progress toward the arm threshold. */
  progress: number
  /** Damped pull distance in px — keeps the rotation responding to
   * the finger after `progress` caps at 1. */
  pullDistance: number
}>()

const ARC_RADIUS = 8
const CIRCUMFERENCE = 2 * Math.PI * ARC_RADIUS
/** A full pull draws 300° — the ring only closes on success. */
const MAX_ARC = 0.833
/** Degrees of arc rotation per damped pull px. */
const SPIN_PER_PX = 2.5

const isPulling = computed(() => props.state === 'pulling' || props.state === 'armed')

// Success = the settle that follows a refresh, not a released-early pull.
const succeeded = ref(false)
// The spin animation starts from wherever the pull rotation ended.
const spinFrom = ref(0)
watch(
  () => props.state,
  (now, prev) => {
    if (now === 'refreshing') spinFrom.value = (props.pullDistance * SPIN_PER_PX) % 360
    // Success survives 'idle' — the glue keeps the indicator mounted
    // through the linger; only a new pull clears the cue.
    if (now === 'settling') succeeded.value = prev === 'refreshing'
    else if (now === 'pulling') succeeded.value = false
  },
)

const pullAngle = computed(() => props.pullDistance * SPIN_PER_PX)
const arcOffset = computed(() => {
  if (succeeded.value) return 0 // ring closes: done
  if (isPulling.value) return CIRCUMFERENCE * (1 - MAX_ARC * props.progress)
  return CIRCUMFERENCE * (1 - MAX_ARC)
})
const discOpacity = computed(() =>
  isPulling.value ? Math.min(1, props.progress * 1.4) : 1,
)
</script>

<template>
  <div
    class="relative flex h-8 w-8 items-center justify-center rounded-full border border-default bg-surface shadow-sm"
    :class="{ 'ptr-armed': state === 'armed' }"
    :style="{ opacity: discOpacity }"
  >
    <svg
      class="h-5 w-5 text-accent"
      :class="{ 'ptr-spin': !isPulling && state !== 'idle' }"
      viewBox="0 0 20 20"
      :style="{
        '--ptr-angle': `${spinFrom}deg`,
        transform: isPulling ? `rotate(${pullAngle}deg)` : undefined,
      }"
      aria-hidden="true"
    >
      <circle
        class="ptr-arc"
        :class="{ 'ptr-arc-eased': !isPulling }"
        cx="10"
        cy="10"
        :r="ARC_RADIUS"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        :stroke-dasharray="CIRCUMFERENCE"
        :stroke-dashoffset="arcOffset"
      />
    </svg>
    <!-- The check sits outside the spinning svg so it draws upright. -->
    <svg
      class="absolute h-5 w-5 text-accent"
      viewBox="0 0 20 20"
      aria-hidden="true"
    >
      <path
        class="ptr-check"
        :class="{ 'ptr-check-in': succeeded }"
        d="M6.5 10.5l2.5 2.5l4.5 -5.5"
        pathLength="1"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </svg>
  </div>
</template>

<style scoped>
/* Spin picks up from the pull's final angle so release → spin is
   one continuous motion. */
.ptr-spin {
  animation: ptr-spin 0.9s linear infinite;
}
@keyframes ptr-spin {
  from {
    transform: rotate(var(--ptr-angle, 0deg));
  }
  to {
    transform: rotate(calc(var(--ptr-angle, 0deg) + 360deg));
  }
}

/* Ring-close (success) eases; during a live pull the arc must track
   the finger with zero lag, so no transition while pulling. */
.ptr-arc-eased {
  transition: stroke-dashoffset 200ms ease-out;
}

/* One small pop when the pull arms, in sync with the haptic. */
.ptr-armed {
  animation: ptr-pop 200ms ease-out;
}
@keyframes ptr-pop {
  50% {
    transform: scale(1.12);
  }
}

/* Check strokes itself in once the ring has (mostly) closed. */
.ptr-check {
  stroke-dasharray: 1;
  stroke-dashoffset: 1;
  opacity: 0;
}
.ptr-check-in {
  stroke-dashoffset: 0;
  opacity: 1;
  transition:
    stroke-dashoffset 180ms ease-out 140ms,
    opacity 0s linear 140ms;
}

@media (prefers-reduced-motion: reduce) {
  .ptr-spin,
  .ptr-armed {
    animation: none;
  }
  .ptr-arc-eased,
  .ptr-check-in {
    transition: none;
  }
}
</style>
