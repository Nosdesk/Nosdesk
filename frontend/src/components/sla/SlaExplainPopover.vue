<script setup lang="ts">
/**
 * "Why does this ticket have this SLA?" popover.
 *
 * Backs the in-place transparency promise of the compute-on-read
 * engine: the pill is on screen, the explanation should be one
 * click away. The popover loads its data lazily the first time it
 * opens for a given ticket (cheap one-shot, no caching beyond the
 * component lifetime) and reuses the result on subsequent opens
 * until the ticket id changes.
 *
 * Rendered content stays text-only on purpose. No icons inside the
 * bullets, no nested badges; the surrounding pill already carries
 * the colour. The popover sits at `role="dialog"` so screen
 * readers announce it as a focused region rather than an inert
 * tooltip, and focus moves into the popover on open.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import Popover from '@/components/common/Popover.vue'
import LoadingSpinner from '@/components/common/LoadingSpinner.vue'
import { slaService, type SlaExplain, type SlaExplainFilter } from '@/services/slaService'

interface Props {
  /** Element to anchor against. Parent passes a template ref; Vue
   *  unwraps it through the prop boundary, and the computed
   *  descriptor below feeds a function form into Popover so it
   *  re-reads the live element each reposition. */
  anchor: HTMLElement | null
  open: boolean
  ticketId: number
}

const props = defineProps<Props>()
const emit = defineEmits<{ (e: 'close'): void }>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const explain = ref<SlaExplain | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const loadedForTicket = ref<number | null>(null)

async function load(): Promise<void> {
  if (loadedForTicket.value === props.ticketId && explain.value) return
  // Stamp the in-flight target *before* awaiting so a fast ticket
  // switch can't slip past the cache check and trigger a duplicate
  // fetch — and so a late-arriving response for a stale ticket id
  // gets discarded instead of overwriting the current one.
  const requestedTicket = props.ticketId
  loadedForTicket.value = requestedTicket
  loading.value = true
  error.value = null
  try {
    const result = await slaService.explainForTicket(requestedTicket)
    // If the ticket changed while we were waiting, drop this result.
    if (loadedForTicket.value === requestedTicket) {
      explain.value = result
    }
  } catch (e) {
    if (loadedForTicket.value === requestedTicket) {
      error.value = e instanceof Error ? e.message : t('sla-explain-error')
    }
  } finally {
    if (loadedForTicket.value === requestedTicket) {
      loading.value = false
    }
  }
}

// Lazy-load on first open per ticket. Reopens for the same ticket
// reuse the cached payload; switching tickets invalidates it.
watch(
  () => [props.open, props.ticketId] as const,
  ([isOpen, id]) => {
    if (!isOpen) return
    if (loadedForTicket.value !== id) {
      explain.value = null
    }
    load()
  },
)

function formatMinutes(minutes: number | null | undefined): string {
  if (minutes == null) return '-'
  if (minutes < 60) return t('sla-explain-fmt-minutes', { n: minutes })
  if (minutes < 24 * 60) {
    const hours = minutes / 60
    return t('sla-explain-fmt-hours', { n: hours % 1 === 0 ? hours : hours.toFixed(1) })
  }
  const days = minutes / (24 * 60)
  return t('sla-explain-fmt-days', { n: days % 1 === 0 ? days : days.toFixed(1) })
}

function filterLabel(f: SlaExplainFilter): string {
  switch (f.kind) {
    case 'priority':
      return t('sla-explain-filter-priority', { value: f.value })
    case 'category':
      return t('sla-explain-filter-category', { name: f.name })
    case 'assignee_group':
      return t('sla-explain-filter-group', { name: f.name })
    default: {
      // Exhaustiveness check: if a new filter kind lands in the
      // SlaExplainFilter union the compiler will error here until
      // it's handled.
      const _exhaustive: never = f
      return _exhaustive
    }
  }
}

const anchorDescriptor = computed(() => ({
  type: 'element' as const,
  element: () => props.anchor,
}))
</script>

<template>
  <Popover
    :open="open"
    :anchor="anchorDescriptor"
    placement="bottom-end"
    role="dialog"
    :aria-label="t('sla-explain-aria')"
    popover-class="w-80 rounded-lg border border-default bg-surface shadow-lg p-3 text-xs flex flex-col gap-3"
    :offset="6"
    @close="emit('close')"
  >
    <header class="flex items-center justify-between">
      <h3 class="text-[11px] uppercase tracking-wider font-semibold text-tertiary">
        {{ t('sla-explain-title') }}
      </h3>
    </header>

    <div v-if="loading" class="flex justify-center py-2">
      <LoadingSpinner size="sm" />
    </div>

    <p v-else-if="error" class="text-status-error">{{ error }}</p>

    <template v-else-if="explain">
      <!-- No-policy case: explicit empty state so the popover still
           pays back the click (rather than silently rendering nothing). -->
      <p v-if="!explain.policy" class="text-tertiary">
        {{ t('sla-explain-no-policy') }}
      </p>

      <template v-else>
        <div class="flex flex-col gap-1">
          <div class="flex items-baseline justify-between gap-2">
            <span class="font-semibold text-primary truncate">{{ explain.policy.name }}</span>
            <span
              v-if="explain.policy.is_default"
              class="text-[10px] uppercase tracking-wide font-semibold text-tertiary"
            >
              {{ t('sla-explain-default-badge') }}
            </span>
          </div>
          <p v-if="explain.policy.matched_filters.length === 0" class="text-tertiary">
            {{ t('sla-explain-no-filters') }}
          </p>
          <ul v-else class="flex flex-col gap-0.5 text-secondary">
            <li v-for="(f, i) in explain.policy.matched_filters" :key="i" class="flex gap-1">
              <span class="text-tertiary" aria-hidden="true">·</span>
              <span>{{ filterLabel(f) }}</span>
            </li>
          </ul>
        </div>

        <div v-if="explain.policy.calendar" class="flex flex-col gap-0.5">
          <span class="text-[10px] uppercase tracking-wider font-semibold text-tertiary">
            {{ t('sla-explain-calendar-label') }}
          </span>
          <span class="text-secondary">
            {{ explain.policy.calendar.name }}
            <span class="text-tertiary">· {{ explain.policy.calendar.timezone }}</span>
          </span>
        </div>

        <div class="flex flex-col gap-0.5">
          <span class="text-[10px] uppercase tracking-wider font-semibold text-tertiary">
            {{ t('sla-explain-targets-label') }}
          </span>
          <span class="text-secondary tabular-nums">
            {{
              t('sla-explain-targets', {
                response: formatMinutes(explain.policy.target_response_minutes),
                resolution: formatMinutes(explain.policy.target_resolution_minutes),
              })
            }}
          </span>
        </div>
      </template>

      <div class="flex flex-col gap-0.5 pt-2 border-t border-subtle">
        <span class="text-[10px] uppercase tracking-wider font-semibold text-tertiary">
          {{ t('sla-explain-state-label') }}
        </span>
        <span class="text-secondary">
          {{
            explain.state.paused
              ? t('sla-explain-state-paused', { state: explain.state.state_name })
              : t('sla-explain-state-running', { state: explain.state.state_name })
          }}
        </span>
      </div>
    </template>
  </Popover>
</template>
