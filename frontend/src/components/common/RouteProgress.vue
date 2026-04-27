<script setup lang="ts">
/**
 * Top-of-viewport progress bar driven by global async activity.
 * One indicator for the whole app, replacing per-component
 * spinners for background work.
 *
 * Behaviour:
 *   - Hidden when nothing is in flight
 *   - Animates in when the oldest pending op crosses ~150ms
 *     (matches `useDelayedFlag`'s "no flash for fast ops"
 *     principle but slightly tighter, the bar is unobtrusive
 *     enough that we can show it sooner than a per-component
 *     spinner)
 *   - Fills asymptotically toward 90% while pending, never
 *     reaches 100% during work because we don't know how long
 *     things will take
 *   - Snaps to 100% on completion, then fades out
 *
 * Visual: 2px high, accent-colored, fixed at the very top.
 * Respects `prefers-reduced-motion` by replacing the easing
 * with a discrete on/off instead of the smooth fill animation.
 */
import { computed, onScopeDispose, ref, watch } from 'vue'
import { useNetworkActivity } from '@/composables/useNetworkActivity'

const { hasPending } = useNetworkActivity()

// Three visible states: hidden, growing (pending), finishing
// (snap-to-100-then-fade).
const state = ref<'hidden' | 'growing' | 'finishing'>('hidden')
const progress = ref(0)

let growTimer: ReturnType<typeof setInterval> | null = null
let fadeTimer: ReturnType<typeof setTimeout> | null = null

function clearTimers() {
  if (growTimer !== null) {
    clearInterval(growTimer)
    growTimer = null
  }
  if (fadeTimer !== null) {
    clearTimeout(fadeTimer)
    fadeTimer = null
  }
}

function startGrowing() {
  clearTimers()
  state.value = 'growing'
  progress.value = 8
  // Asymptotic ramp toward 90%. Each tick adds a fraction of
  // the remaining distance, so the bar slows down the closer
  // it gets, the standard NProgress feel.
  growTimer = setInterval(() => {
    if (state.value !== 'growing') return
    const remaining = 90 - progress.value
    if (remaining <= 0.5) return
    progress.value += remaining * 0.08
  }, 200)
}

function finish() {
  clearTimers()
  state.value = 'finishing'
  progress.value = 100
  fadeTimer = setTimeout(() => {
    state.value = 'hidden'
    progress.value = 0
    fadeTimer = null
  }, 220)
}

watch(
  hasPending,
  (active) => {
    if (active) {
      startGrowing()
    } else if (state.value === 'growing') {
      finish()
    }
  },
  { immediate: true },
)

onScopeDispose(clearTimers)

const visible = computed(() => state.value !== 'hidden')
const opacity = computed(() => (state.value === 'finishing' ? 0 : 1))
</script>

<template>
  <Transition
    enter-active-class="transition-opacity duration-150"
    enter-from-class="opacity-0"
    enter-to-class="opacity-100"
    leave-active-class="transition-opacity duration-200"
    leave-from-class="opacity-100"
    leave-to-class="opacity-0"
  >
    <div
      v-if="visible"
      class="route-progress"
      :style="{ width: `${progress}%`, opacity }"
      role="progressbar"
      aria-label="Loading"
      aria-hidden="true"
    />
  </Transition>
</template>

<style scoped>
.route-progress {
  position: fixed;
  top: 0;
  left: 0;
  height: 2px;
  background: var(--color-accent);
  z-index: var(--z-overlay, 300);
  transition: width 220ms cubic-bezier(0.16, 1, 0.3, 1);
  pointer-events: none;
}

@media (prefers-reduced-motion: reduce) {
  .route-progress {
    /* Skip the smooth fill, show as a static bar instead so the
       animation doesn't violate user motion preferences. */
    transition: none;
  }
}
</style>
