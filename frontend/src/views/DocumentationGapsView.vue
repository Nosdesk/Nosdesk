<!--
  DocumentationGapsView: queue of open knowledge gaps.

  Two-pane layout: left list of gaps ranked by impact_score, right
  detail panel for the currently-selected gap. Clicking a gap row
  routes to /documentation/gaps/:id (props mode) so the URL stays
  shareable; the detail panel reads the route param and fetches
  via useKnowledgeGap.

  Phase 2a only renders manual_flag signals; the layout supports
  any signal_type so 2b/2c/2d can plug in without view changes.
-->
<script setup lang="ts">
import { computed, watch, ref } from 'vue'
import { useRoute, useRouter, RouterLink } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { useTitleManager } from '@/composables/useTitleManager'
import Icon from '@/components/common/Icon.vue'
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils'
import {
  useKnowledgeGaps,
  useKnowledgeGap,
  useDismissGapMutation,
  useDetectClustersMutation,
} from '@/composables/useKnowledgeGaps'
import type { KnowledgeGapSignal } from '@nosdesk/core/services/knowledgeGapsService'

defineOptions({ name: 'DocumentationGapsView' })

const props = defineProps<{
  /** Route param when the URL is /documentation/gaps/:id. */
  id?: string
}>()

const route = useRoute()
const router = useRouter()
const titleManager = useTitleManager()
const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

titleManager.setCustomTitle(t('docs-gaps-title'))

// List
const { gaps, isLoading: listLoading, refetch: refetchList } = useKnowledgeGaps()

// Detail — driven by the route param.
const selectedId = computed<number | null>(() => {
  const raw = props.id ?? (route.params.id as string | undefined)
  if (!raw) return null
  const n = Number(raw)
  return Number.isFinite(n) ? n : null
})

const { gap: selectedGap, isLoading: detailLoading } = useKnowledgeGap(selectedId)

// On large viewports, jump to the top gap so the side-by-side
// detail pane isn't blank on first paint. On small viewports we
// stay on the list (the detail pane is hidden anyway, and an
// auto-jump would feel like a hijacked navigation).
watch([gaps, selectedId], ([list, current]) => {
  if (current !== null) return
  if (list.length === 0) return
  if (typeof window === 'undefined') return
  if (window.matchMedia('(min-width: 1024px)').matches) {
    router.replace({ name: 'documentation-gap-detail', params: { id: list[0].id } })
  }
})

const dismissMutation = useDismissGapMutation()
const isDismissing = ref(false)

const detectMutation = useDetectClustersMutation()
const detectMessage = ref<string | null>(null)

async function runDetection() {
  detectMessage.value = null
  const result = await detectMutation.mutateAsync(undefined)
  if (result) {
    if (result.gaps_created === 0 && result.gaps_updated === 0) {
      detectMessage.value = t('docs-gaps-detect-no-results')
    } else {
      const parts: string[] = []
      if (result.gaps_created > 0) parts.push(t('docs-gaps-detect-created', { count: result.gaps_created }))
      if (result.gaps_updated > 0) parts.push(t('docs-gaps-detect-updated', { count: result.gaps_updated }))
      detectMessage.value = parts.join(', ')
    }
    await refetchList()
  }
}

interface ClusterPayload {
  ticket_ids?: number[]
  sample_titles?: string[]
  category_name?: string | null
  device_model?: string | null
  channel_label?: string | null
  member_count?: number
}

function clusterPayload(signal: KnowledgeGapSignal): ClusterPayload {
  return (signal.payload ?? {}) as ClusterPayload
}

interface FailedSearchPayload {
  query_sample?: string
  count?: number
  first_seen?: string
  last_seen?: string
}

function failedSearchPayload(signal: KnowledgeGapSignal): FailedSearchPayload {
  return (signal.payload ?? {}) as FailedSearchPayload
}

interface StaleDocPayload {
  page_uuid?: string
  page_title?: string
  page_slug?: string
  verified_at?: string
  verify_interval_days?: number
  days_stale?: number
  recent_ticket_ids?: number[]
}

function staleDocPayload(signal: KnowledgeGapSignal): StaleDocPayload {
  return (signal.payload ?? {}) as StaleDocPayload
}

/** Pick the best label for an impact_score badge based on the
 *  gap's signal mix. We don't have the full signal list in the
 *  queue summary (only the count), so we infer from the gap
 *  title pattern. */
function impactLabel(gapTitle: string): string {
  if (gapTitle.startsWith('Customers searched:')) return t('docs-gaps-impact-searches')
  if (gapTitle.startsWith('Doc may be stale:')) return t('docs-gaps-impact-recent-tickets')
  return t('docs-gaps-impact-tickets')
}

async function dismissCurrent() {
  if (!selectedGap.value) return
  isDismissing.value = true
  try {
    await dismissMutation.mutateAsync({ gapId: selectedGap.value.id })
    await refetchList()
    // Move selection to the next gap, or clear.
    const remaining = gaps.value.filter((g) => g.id !== selectedGap.value?.id)
    if (remaining.length > 0) {
      router.replace({ name: 'documentation-gap-detail', params: { id: remaining[0].id } })
    } else {
      router.replace({ name: 'documentation-gaps' })
    }
  } finally {
    isDismissing.value = false
  }
}

/** UI label for a signal type. Keeps copy decisions out of the
 *  template and makes 2b/2c/2d additions one line. */
function signalLabel(signal: KnowledgeGapSignal): string {
  switch (signal.signal_type) {
    case 'manual_flag':
      return t('docs-gaps-signal-manual-flag')
    case 'ticket_cluster':
      return t('docs-gaps-signal-ticket-cluster')
    case 'failed_search':
      return t('docs-gaps-signal-failed-search')
    case 'stale_doc':
      return t('docs-gaps-signal-stale-doc')
    case 'ai_suggested':
      return t('docs-gaps-signal-ai-suggested')
    default:
      return signal.signal_type
  }
}
</script>

<template>
  <!--
    Two-pane layout follows the AdminLayout convention: the list
    sits in a left sidebar at lg+ and detail fills the rest. Below
    lg, only one pane is visible at a time, route-driven (list
    when no `:id`, detail when `:id` is set), with a Back link in
    the detail header so phone users can return to the list.
  -->
  <div class="bg-app flex flex-col lg:flex-row h-full">
    <!-- Left: list pane.
         lg+: always visible as a 320px sidebar.
         Below lg: visible only when no gap is selected (the URL
         carries no :id) — when one is selected, the detail pane
         takes the full width. -->
    <aside
      class="flex flex-col flex-shrink-0 border-default overflow-hidden bg-app"
      :class="[
        'lg:w-80 lg:border-r lg:flex',
        selectedId !== null ? 'hidden' : 'flex flex-1',
      ]"
    >
      <!-- List header: title + count, plus a "Back to docs"
           affordance only on the list view. -->
      <div class="border-b border-default px-4 py-3 flex flex-col gap-2 flex-shrink-0">
        <div class="flex items-center justify-between gap-3">
          <div class="flex items-center gap-2 min-w-0">
            <Icon name="warning" class="text-amber-500 flex-shrink-0" />
            <h2 class="text-base font-semibold text-primary truncate">{{ $t('docs-gaps-heading') }}</h2>
            <span v-if="!listLoading" class="text-xs text-tertiary bg-surface-alt px-2 py-0.5 rounded-full flex-shrink-0">
              {{ gaps.length }}
            </span>
          </div>
          <RouterLink
            to="/documentation"
            class="text-xs text-secondary hover:text-primary transition-colors flex items-center gap-1 flex-shrink-0"
          >
            <Icon name="chevronRight" size="xs" class="rotate-180" />
            {{ $t('docs-gaps-back-docs') }}
          </RouterLink>
        </div>
        <div class="flex items-center justify-between gap-2">
          <button
            type="button"
            :disabled="detectMutation.asyncStatus.value === 'loading'"
            class="text-[11px] px-2 py-1 rounded-md bg-surface-alt hover:bg-surface-hover text-secondary hover:text-primary transition-colors disabled:opacity-50 flex items-center gap-1.5"
            @click="runDetection"
          >
            <Icon name="search" size="xs" />
            <span v-if="detectMutation.asyncStatus.value === 'loading'">{{ $t('docs-gaps-refreshing') }}&hellip;</span>
            <span v-else>{{ $t('docs-gaps-refresh') }}</span>
          </button>
          <span v-if="detectMessage" class="text-[11px] text-tertiary truncate">
            {{ detectMessage }}
          </span>
        </div>
      </div>

      <!-- Scrollable list -->
      <div class="flex-1 overflow-y-auto">
        <div v-if="listLoading" class="p-4 text-sm text-tertiary">{{ $t('docs-gaps-loading') }}&hellip;</div>
        <div v-else-if="gaps.length === 0" class="p-6 text-center text-sm text-tertiary">
          {{ $t('docs-gaps-empty') }}
        </div>
        <ul v-else class="divide-y divide-subtle">
          <li v-for="gap in gaps" :key="gap.id">
            <RouterLink
              :to="{ name: 'documentation-gap-detail', params: { id: gap.id } }"
              class="block px-4 py-3 hover:bg-surface-hover transition-colors"
              :class="{ 'bg-surface-alt lg:bg-surface-alt': selectedId === gap.id }"
            >
              <div class="flex items-start justify-between gap-2 mb-1">
                <p class="flex-1 min-w-0 text-sm text-primary font-medium truncate">
                  {{ gap.title }}
                </p>
                <span
                  class="flex-shrink-0 text-[10px] px-1.5 py-0.5 rounded bg-surface text-tertiary"
                  :title="$t('docs-gaps-impact-tooltip', { count: gap.impact_score, label: impactLabel(gap.title) })"
                >
                  {{ gap.impact_score }}&nbsp;{{ impactLabel(gap.title) }}
                </span>
              </div>
              <div class="flex items-center justify-between gap-2 text-[11px] text-tertiary">
                <span>{{ $t('docs-gaps-signal-count', { count: gap.evidence_count }) }}</span>
                <span v-if="gap.last_evidence_at">
                  {{ formatRelativeTime(gap.last_evidence_at) }}
                </span>
              </div>
            </RouterLink>
          </li>
        </ul>
      </div>
    </aside>

    <!-- Right: detail pane.
         lg+: always visible, takes remaining space.
         Below lg: visible only when a gap is selected. -->
    <section
      class="flex-1 min-w-0 overflow-y-auto"
      :class="[selectedId === null ? 'hidden lg:block' : 'block']"
    >
        <!-- Mobile back-to-list bar; hidden at lg+ where the
             sidebar makes the back affordance unnecessary. -->
        <div
          v-if="selectedId !== null"
          class="lg:hidden border-b border-default bg-app px-4 py-2.5"
        >
          <RouterLink
            :to="{ name: 'documentation-gaps' }"
            class="flex items-center gap-1.5 text-sm text-secondary hover:text-primary transition-colors"
          >
            <Icon name="chevronRight" size="xs" class="rotate-180" />
            {{ $t('docs-gaps-back-list') }}
          </RouterLink>
        </div>

        <div v-if="!selectedId" class="p-8 text-center text-sm text-tertiary">
          {{ $t('docs-gaps-select-prompt') }}
        </div>
        <div v-else-if="detailLoading" class="p-8 text-sm text-tertiary">{{ $t('docs-gaps-loading') }}&hellip;</div>
        <article v-else-if="selectedGap" class="max-w-3xl mx-auto p-4 sm:p-6 flex flex-col gap-6">
          <!-- Title + actions -->
          <header class="flex items-start justify-between gap-4 pb-4 border-b border-subtle">
            <div class="flex-1 min-w-0">
              <h1 class="text-xl font-semibold text-primary">{{ selectedGap.title }}</h1>
              <p v-if="selectedGap.description" class="text-sm text-secondary mt-2">
                {{ selectedGap.description }}
              </p>
              <div class="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-tertiary">
                <span>{{ $t('docs-gaps-status-label') }} <span class="text-secondary">{{ selectedGap.status }}</span></span>
                <span>
                  <span class="text-secondary">{{ selectedGap.impact_score }}</span>
                  {{ impactLabel(selectedGap.title) }}
                </span>
                <span v-if="selectedGap.last_evidence_at">
                  {{ $t('docs-gaps-last-evidence', { time: formatRelativeTime(selectedGap.last_evidence_at) }) }}
                </span>
              </div>
            </div>
            <button
              type="button"
              :disabled="isDismissing"
              class="flex-shrink-0 text-xs px-3 py-1.5 rounded-md text-secondary hover:text-status-error hover:bg-status-error-muted transition-colors disabled:opacity-50"
              @click="dismissCurrent"
            >
              {{ $t('docs-gaps-dismiss') }}
            </button>
          </header>

          <!-- Evidence -->
          <section class="flex flex-col gap-3">
            <h2 class="text-xs font-semibold uppercase tracking-wide text-tertiary">
              {{ $t('docs-gaps-evidence-heading') }}
            </h2>
            <ul v-if="selectedGap.signals && selectedGap.signals.length > 0" class="flex flex-col gap-2">
              <li
                v-for="signal in selectedGap.signals"
                :key="signal.id"
                class="rounded-lg border border-default bg-surface-alt px-3 py-2 flex items-start gap-3"
              >
                <Icon name="ticket" class="flex-shrink-0 mt-0.5 text-tertiary" />
                <div class="flex-1 min-w-0">
                  <div class="flex items-center justify-between gap-2 mb-1">
                    <span class="text-[10px] uppercase tracking-wide text-tertiary">
                      {{ signalLabel(signal) }}
                    </span>
                    <span class="text-[10px] text-tertiary">
                      {{ formatRelativeTime(signal.detected_at) }}
                    </span>
                  </div>

                  <!-- Cluster signal: the source is a fingerprint,
                       not a single ticket. Render the member ticket
                       list (sample titles + count remainder) and
                       the discriminating facets. -->
                  <template v-if="signal.signal_type === 'ticket_cluster'">
                    <p class="text-sm text-primary">
                      <template v-if="clusterPayload(signal).category_name">
                        {{ clusterPayload(signal).category_name }}
                      </template>
                      <template v-else>{{ $t('docs-gaps-cluster-fallback') }}</template>
                      <span
                        v-if="clusterPayload(signal).device_model || clusterPayload(signal).channel_label"
                        class="text-secondary"
                      >
                        <template v-if="clusterPayload(signal).device_model">
                          &middot; {{ clusterPayload(signal).device_model }}
                        </template>
                        <template v-if="clusterPayload(signal).channel_label">
                          &middot; {{ $t('docs-gaps-cluster-via', { channel: clusterPayload(signal).channel_label ?? '' }) }}
                        </template>
                      </span>
                    </p>
                    <ul class="mt-1.5 flex flex-col gap-0.5">
                      <li
                        v-for="(title, idx) in clusterPayload(signal).sample_titles ?? []"
                        :key="idx"
                        class="text-xs text-secondary truncate"
                      >
                        <RouterLink
                          v-if="clusterPayload(signal).ticket_ids?.[idx]"
                          :to="`/tickets/${clusterPayload(signal).ticket_ids![idx]}`"
                          class="hover:text-accent transition-colors"
                        >
                          <span class="text-tertiary">#{{ clusterPayload(signal).ticket_ids![idx] }}</span>
                          {{ title }}
                        </RouterLink>
                      </li>
                    </ul>
                    <p
                      v-if="(clusterPayload(signal).member_count ?? 0) > (clusterPayload(signal).sample_titles?.length ?? 0)"
                      class="text-[11px] text-tertiary mt-1"
                    >
                      &hellip; {{ $t('docs-gaps-cluster-more', { count: (clusterPayload(signal).member_count ?? 0) - (clusterPayload(signal).sample_titles?.length ?? 0) }) }}
                    </p>
                  </template>

                  <!-- Stale-doc signal: a verified-but-now-stale
                       page that 'resolves' recently-closed tickets.
                       Editorial action is to re-verify or update
                       the doc, which auto-dismisses the gap. -->
                  <template v-else-if="signal.signal_type === 'stale_doc'">
                    <RouterLink
                      v-if="staleDocPayload(signal).page_slug"
                      :to="`/documentation/${staleDocPayload(signal).page_slug}`"
                      class="text-sm text-primary hover:text-accent transition-colors"
                    >
                      📄 {{ staleDocPayload(signal).page_title ?? $t('docs-gaps-stale-untitled') }}
                    </RouterLink>
                    <p class="text-[11px] text-tertiary mt-1">
                      <template v-if="staleDocPayload(signal).verified_at">
                        <span class="text-secondary">
                          {{ $t('docs-gaps-stale-verified', { time: formatRelativeTime(staleDocPayload(signal).verified_at!) }) }}
                        </span>
                      </template>
                      <template v-else>
                        {{ $t('docs-gaps-stale-verified-no-time') }}
                      </template>
                      <template v-if="(staleDocPayload(signal).days_stale ?? 0) > 0">
                        &middot;
                        <span class="text-amber-700 dark:text-amber-300 font-medium">
                          {{ $t('docs-gaps-stale-days-past-due', { count: staleDocPayload(signal).days_stale ?? 0 }) }}
                        </span>
                      </template>
                    </p>
                    <p
                      v-if="(staleDocPayload(signal).recent_ticket_ids?.length ?? 0) > 0"
                      class="text-[11px] text-tertiary mt-1"
                    >
                      <span class="text-secondary">{{ staleDocPayload(signal).recent_ticket_ids!.length }}</span>
                      {{ $t('docs-gaps-stale-recent-tickets', { count: staleDocPayload(signal).recent_ticket_ids!.length }) }}
                      <span class="text-tertiary">
                        <template v-for="(tid, i) in staleDocPayload(signal).recent_ticket_ids!.slice(0, 5)" :key="tid">
                          <RouterLink
                            :to="`/tickets/${tid}`"
                            class="hover:text-accent transition-colors"
                          >#{{ tid }}</RouterLink><span v-if="i < Math.min(4, staleDocPayload(signal).recent_ticket_ids!.length - 1)">, </span>
                        </template>
                        <template v-if="(staleDocPayload(signal).recent_ticket_ids?.length ?? 0) > 5">
                          {{ $t('docs-gaps-stale-plus-more', { count: staleDocPayload(signal).recent_ticket_ids!.length - 5 }) }}
                        </template>
                      </span>
                    </p>
                    <p class="text-[11px] text-tertiary mt-1 italic">
                      {{ $t('docs-gaps-stale-auto-dismiss') }}
                    </p>
                  </template>

                  <!-- Failed-search signal: a recurring zero-result
                       query. Shows the query text, occurrence
                       count, and first/last seen times. -->
                  <template v-else-if="signal.signal_type === 'failed_search'">
                    <p class="text-sm text-primary">
                      "{{ failedSearchPayload(signal).query_sample ?? signal.source_ref }}"
                    </p>
                    <p class="text-[11px] text-tertiary mt-1">
                      {{ $t('docs-gaps-failed-search-count', { count: failedSearchPayload(signal).count ?? 0 }) }}
                      <template v-if="failedSearchPayload(signal).first_seen && failedSearchPayload(signal).last_seen">
                        &middot; {{ $t('docs-gaps-failed-search-range', { first: formatRelativeTime(failedSearchPayload(signal).first_seen!), last: formatRelativeTime(failedSearchPayload(signal).last_seen!) }) }}
                      </template>
                    </p>
                  </template>

                  <!-- Manual flag (and other ticket-typed signals):
                       single ticket as the source. -->
                  <template v-else-if="signal.source_kind === 'ticket'">
                    <RouterLink
                      :to="`/tickets/${signal.source_ref}`"
                      class="text-sm text-primary hover:text-accent transition-colors"
                    >
                      #{{ signal.source_ref }}
                      <span v-if="signal.ticket_title" class="text-secondary">
                        &middot; {{ signal.ticket_title }}
                      </span>
                    </RouterLink>
                  </template>

                  <p v-else class="text-sm text-secondary">
                    {{ signal.source_ref }}
                  </p>

                  <!-- Detector attribution: who flagged it
                       (manual_flag) or which auto-detector emitted
                       it. Renders below the source so it reads as
                       provenance, not as the headline. -->
                  <p
                    v-if="signal.detected_by_user"
                    class="text-[11px] text-tertiary mt-1"
                  >
                    {{ $t('docs-gaps-flagged-by', { name: signal.detected_by_user.name }) }}
                  </p>
                  <p
                    v-if="signal.payload?.reason"
                    class="text-xs text-tertiary mt-1 italic"
                  >
                    "{{ signal.payload.reason }}"
                  </p>
                </div>
              </li>
            </ul>
            <p v-else class="text-sm text-tertiary">{{ $t('docs-gaps-evidence-empty') }}</p>
          </section>

          <!-- Resolve action: punts to "create a doc and link". The
               actual flow uses Phase 1's existing 'Save as doc'
               action; an explicit resolve UI lands in 2b once
               clusters give us multi-ticket gaps. For now, the
               agent navigates to a ticket and uses Save-as-doc. -->
          <section
            v-if="selectedGap.status === 'open' || selectedGap.status === 'drafting'"
            class="rounded-lg border border-dashed border-default p-4 text-sm text-secondary"
          >
            <p class="font-medium text-primary mb-1">{{ $t('docs-gaps-resolve-heading') }}</p>
            <p v-safe-html="$t('docs-gaps-resolve-body', { action: `<span class=&quot;font-medium text-primary&quot;>${$t('docs-gaps-resolve-action')}</span>` })"></p>
          </section>
        </article>
    </section>
  </div>
</template>
