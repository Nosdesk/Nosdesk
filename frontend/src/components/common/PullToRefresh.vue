<script setup lang="ts">
/**
 * Pull-to-refresh glue: binds `usePullToRefresh` to a scroll
 * container and renders the indicator in the gap the pulled content
 * opens up. Active only in the Tauri app — on the web this renders
 * nothing and attaches nothing.
 *
 * The indicator Teleports to <body>: the gesture translates the
 * scroll container, and a transformed ancestor becomes the containing
 * block for `position: fixed` descendants (the BulkActionBar gotcha),
 * so the indicator must live outside the transformed subtree. It is
 * positioned from the scroller's rect captured at gesture start and
 * clipped to the revealed gap, so it can never paint over the header.
 *
 * After a successful refresh the indicator holds for a beat and fades
 * while the content settles underneath it — the closed-ring + check
 * completion cue is meant to be seen, not clipped mid-frame. A
 * released-early pull hides immediately with the collapsing gap.
 *
 * The default refresh action is correct for every current view:
 * `pullDelta()` re-syncs the object pool (tickets, projects, assets…)
 * and `invalidateQueries({ active: true })` refetches every mounted
 * Pinia Colada query (dashboard KPIs, notifications, list pages).
 * Data is already live via SSE + delta poll — the gesture is
 * reassurance and repair, so it always resolves to the success state.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQueryCache } from '@pinia/colada'
import { pullDelta } from '@/sync/lifecycle'
import { isTauriRuntime } from '@/platform'
import {
  usePullToRefresh,
  type PullToRefreshAnchor,
} from '@/composables/usePullToRefresh'
import { useReducedMotion } from '@/composables/useReducedMotion'
import PullToRefreshIndicator from './PullToRefreshIndicator.vue'

interface Props {
  /** The scroll container. Null-safe: nothing attaches until it exists. */
  target: HTMLElement | null | undefined
  /** Override the refresh work. Defaults to pool delta + active-query refetch. */
  onRefresh?: () => Promise<unknown>
  disabled?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  onRefresh: undefined,
  disabled: false,
})

/** How long the success cue lingers after the content has settled. */
const SUCCESS_LINGER_MS = 400

const fluent = useFluent()
const queryCache = useQueryCache()
const reducedMotion = useReducedMotion()

// Constant per session: the Tauri global is injected before the app boots.
const tauri = isTauriRuntime()

function defaultRefresh(): Promise<unknown> {
  return Promise.allSettled([
    pullDelta(),
    queryCache.invalidateQueries({ active: true }),
  ])
}

const { state, pullDistance, progress, isActive, anchor, holdDistance } = usePullToRefresh({
  target: () => props.target,
  onRefresh: () => (props.onRefresh ?? defaultRefresh)(),
  enabled: () => tauri && !props.disabled,
})

// The composable clears `anchor` on reset, but the success cue
// outlives the gesture — keep the last known position for the fade.
const lastAnchor = ref<PullToRefreshAnchor | null>(null)
watch(anchor, (a) => {
  if (a) lastAnchor.value = { ...a }
})

// Success = the settle that followed a refresh (vs a released-early pull).
const succeeded = ref(false)

// Visibility outlives `isActive` on the success path so the check is
// actually seen: hold at the gap, then fade out.
const shown = ref(false)
const fading = ref(false)
let hideTimer: number | null = null
watch(isActive, (active) => {
  if (active) {
    if (hideTimer !== null) {
      window.clearTimeout(hideTimer)
      hideTimer = null
    }
    shown.value = true
    fading.value = false
    return
  }
  if (succeeded.value) {
    // Linger so the completion cue lands; fade only when motion is ok.
    fading.value = !reducedMotion.value
    hideTimer = window.setTimeout(() => {
      shown.value = false
      fading.value = false
      succeeded.value = false
      hideTimer = null
    }, SUCCESS_LINGER_MS)
  } else {
    shown.value = false
    succeeded.value = false
  }
})

/** Height of the revealed gap the indicator lives in. Follows the
 * finger while pulling; holds at the refresh offset through the
 * success cue (CSS-transitioned to match the content's settle). */
const gapHeight = computed(() => {
  if (reducedMotion.value) return shown.value ? holdDistance : 0
  switch (state.value) {
    case 'pulling':
    case 'armed':
      return pullDistance.value
    case 'refreshing':
      return holdDistance
    case 'settling':
      return succeeded.value ? holdDistance : 0
    default:
      // Success linger after the gesture is over.
      return shown.value ? holdDistance : 0
  }
})

/** Transition the gap only when the content itself is animating —
 * during a live pull the gap tracks the finger directly. */
const gapAnimates = computed(
  () => !reducedMotion.value && state.value !== 'pulling' && state.value !== 'armed',
)

const wrapperStyle = computed(() => {
  const a = lastAnchor.value
  if (!a) return undefined
  return {
    top: `${a.top}px`,
    left: `${a.left}px`,
    width: `${a.width}px`,
    height: `${gapHeight.value}px`,
    zIndex: 30,
    transition: gapAnimates.value
      ? `height 200ms cubic-bezier(0.22, 1, 0.36, 1), opacity ${SUCCESS_LINGER_MS}ms ease-out`
      : undefined,
    opacity: fading.value ? 0 : 1,
  }
})

// Screen-reader announcements. The region persists across the
// indicator's mount/unmount so completion is still announced.
const announcement = ref('')
let announceTimer: number | null = null
watch(state, (now, prev) => {
  if (now === 'refreshing') {
    succeeded.value = false
    if (announceTimer !== null) window.clearTimeout(announceTimer)
    announcement.value = fluent.$t('ptr-refreshing')
  } else if (prev === 'refreshing' && now === 'settling') {
    succeeded.value = true
    announcement.value = fluent.$t('ptr-updated')
    announceTimer = window.setTimeout(() => {
      announcement.value = ''
      announceTimer = null
    }, 2000)
  }
})
</script>

<template>
  <Teleport v-if="tauri" to="body">
    <div
      v-if="shown && lastAnchor"
      class="pointer-events-none fixed flex items-end justify-center overflow-hidden"
      :style="wrapperStyle"
    >
      <div class="mb-2.5">
        <PullToRefreshIndicator
          :state="state"
          :progress="progress"
          :pull-distance="pullDistance"
        />
      </div>
    </div>
    <span class="sr-only" role="status" aria-live="polite">{{ announcement }}</span>
  </Teleport>
</template>
