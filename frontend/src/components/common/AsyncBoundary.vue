<script setup lang="ts">
/**
 * State-machine slot renderer for asynchronous content.
 *
 * Implements the "UI renders first, subscribes to content"
 * principle: the default slot always represents what the user
 * should see when data is available. The boundary shows
 * `#pending` or `#error` only during a true first-load (no data
 * yet). Once data exists, refetches happen quietly in the
 * background and any indicator is a global concern (use
 * `<RouteProgress>` for that).
 *
 * The `pendingDelay` prop is the load-bearing UX choice. Below
 * the threshold (default 300ms), the pending slot never renders.
 * Fast operations show no indicator at all. This kills
 * "skeleton flash" by construction.
 *
 * State machine:
 *   - !hasData && op.isPending && delayed  →  #pending
 *   - !hasData && op.isError                →  #error
 *   - otherwise                             →  default slot
 *
 * Errors after the first successful load do NOT replace content.
 * That would punish users for a transient network blip on a
 * background refetch. Consumers surface those errors via toasts
 * or banners above the content.
 */
import { computed } from 'vue'
import { useDelayedFlag } from '@/composables/useDelayedFlag'

/**
 * Minimal operation contract this boundary subscribes to. Any
 * object that exposes these three booleans / values works:
 * Pinia Colada's `useQuery` and `useInfiniteQuery` both project
 * cleanly into this shape (see consumers for the standard
 * `computed(() => ({ isPending: list.asyncStatus.value ===
 * 'loading', isError: ..., error: ... }))` recipe).
 */
export interface AsyncBoundaryOp {
  isPending: boolean
  isError: boolean
  error: unknown
}

interface Props {
  /** Operation to subscribe to. Use the projection recipe in
   *  the doc comment above to derive this from a Pinia Colada
   *  query result. */
  op: AsyncBoundaryOp
  /** Whether the consumer's default slot has meaningful data
   *  to show. When true, pending and error chrome are
   *  suppressed (data wins over status). */
  hasData?: boolean
  /** Milliseconds the operation must remain pending before the
   *  pending slot renders. Default 300, fast enough that the
   *  indicator appears for genuinely slow loads, slow enough
   *  that quick loads complete invisibly. */
  pendingDelay?: number
}

const props = withDefaults(defineProps<Props>(), {
  hasData: false,
  pendingDelay: 300,
})

const showDelayedPending = useDelayedFlag(
  () => props.op.isPending && !props.hasData,
  props.pendingDelay,
)

const slotKind = computed<'pending' | 'error' | 'default'>(() => {
  if (props.hasData) return 'default'
  if (props.op.isError) return 'error'
  if (showDelayedPending.value) return 'pending'
  return 'default'
})
</script>

<template>
  <slot v-if="slotKind === 'default'" />
  <slot
    v-else-if="slotKind === 'pending'"
    name="pending"
  />
  <slot
    v-else
    name="error"
    :error="op.error"
  />
</template>
