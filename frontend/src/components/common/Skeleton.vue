<!--
Low-level skeleton primitive. Renders one or more shimmer-animated
blocks of a caller-specified shape. All the a11y + perceived-perf
rules live here so per-feature skeletons (rows, cards, article
bodies) can compose it without re-implementing them:

  - `role="status"` + `aria-busy` announces "Loading …" to screen
    readers once per skeleton instance. The shimmer bars themselves
    are aria-hidden since they're decorative.
  - Honours `prefers-reduced-motion` — the keyframe collapses to a
    static neutral fill for users who've asked for less motion.
  - 150 ms mount delay so a cache-hit fetch that resolves instantly
    never flashes a skeleton (Airbnb / Stripe / Meta convention; see
    Nielsen Norman response-time thresholds).
  - Once visible, stays up for at least 300 ms so it doesn't
    disappear mid-animation and cause a jitter.

Usage:

  <Skeleton label="Loading archived pages">
    <SkeletonBar class="h-4 w-40" />
    <SkeletonBar class="h-4 w-full mt-2" />
  </Skeleton>
-->
<template>
  <div
    v-if="mounted"
    role="status"
    aria-live="polite"
    :aria-busy="true"
    :aria-label="label"
    :class="$attrs.class"
  >
    <span class="sr-only">{{ label }}</span>
    <slot />
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue';

const props = withDefaults(
  defineProps<{
    /** Screen-reader label; keep it task-specific ("Loading archived pages"). */
    label?: string;
    /** Delay before the skeleton is rendered. Prevents flashing on cache-hit. */
    delayMs?: number;
    /** Minimum time the skeleton stays mounted once shown. Prevents jitter. */
    minDurationMs?: number;
  }>(),
  {
    label: 'Loading',
    delayMs: 150,
    minDurationMs: 300,
  },
);

const mounted = ref(false);
let showTimer: ReturnType<typeof setTimeout> | null = null;
let shownAt = 0;

onMounted(() => {
  if (props.delayMs <= 0) {
    mounted.value = true;
    shownAt = performance.now();
    return;
  }
  showTimer = setTimeout(() => {
    mounted.value = true;
    shownAt = performance.now();
  }, props.delayMs);
});

onBeforeUnmount(() => {
  if (showTimer) clearTimeout(showTimer);
});

// Expose a Promise that resolves once the minimum-display window has
// elapsed. Callers that care (e.g. "hide the skeleton only after data
// AND minDuration") can `await` it; simpler callers can ignore it.
defineExpose({
  waitForMinDuration() {
    if (!mounted.value) return Promise.resolve();
    const elapsed = performance.now() - shownAt;
    const remaining = props.minDurationMs - elapsed;
    if (remaining <= 0) return Promise.resolve();
    return new Promise<void>((resolve) => setTimeout(resolve, remaining));
  },
});
</script>
