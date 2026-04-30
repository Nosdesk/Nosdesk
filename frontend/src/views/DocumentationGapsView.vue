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
import { useTitleManager } from '@/composables/useTitleManager'
import Icon from '@/components/common/Icon.vue'
import { formatRelativeTime } from '@/utils/dateUtils'
import {
  useKnowledgeGaps,
  useKnowledgeGap,
  useDismissGapMutation,
} from '@/composables/useKnowledgeGaps'
import type { KnowledgeGapSignal } from '@/services/knowledgeGapsService'

defineOptions({ name: 'DocumentationGapsView' })

const props = defineProps<{
  /** Route param when the URL is /documentation/gaps/:id. */
  id?: string
}>()

const route = useRoute()
const router = useRouter()
const titleManager = useTitleManager()

titleManager.setCustomTitle('Knowledge Gaps')

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
      return 'Flagged by agent'
    case 'ticket_cluster':
      return 'Ticket cluster'
    case 'failed_search':
      return 'Failed search'
    case 'stale_doc':
      return 'Stale documentation'
    case 'ai_suggested':
      return 'AI suggestion'
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
      <div class="border-b border-default px-4 py-3 flex items-center justify-between gap-3 flex-shrink-0">
        <div class="flex items-center gap-2 min-w-0">
          <Icon name="warning" class="text-amber-500 flex-shrink-0" />
          <h2 class="text-base font-semibold text-primary truncate">Knowledge Gaps</h2>
          <span v-if="!listLoading" class="text-xs text-tertiary bg-surface-alt px-2 py-0.5 rounded-full flex-shrink-0">
            {{ gaps.length }}
          </span>
        </div>
        <RouterLink
          to="/documentation"
          class="text-xs text-secondary hover:text-primary transition-colors flex items-center gap-1 flex-shrink-0"
        >
          <Icon name="chevronRight" size="xs" class="rotate-180" />
          Docs
        </RouterLink>
      </div>

      <!-- Scrollable list -->
      <div class="flex-1 overflow-y-auto">
        <div v-if="listLoading" class="p-4 text-sm text-tertiary">Loading&hellip;</div>
        <div v-else-if="gaps.length === 0" class="p-6 text-center text-sm text-tertiary">
          No open knowledge gaps. Flag a ticket from its sidebar to add one.
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
                  :title="`Impact score ${gap.impact_score}`"
                >
                  {{ gap.impact_score }}
                </span>
              </div>
              <div class="flex items-center justify-between gap-2 text-[11px] text-tertiary">
                <span>{{ gap.evidence_count }} signal{{ gap.evidence_count === 1 ? '' : 's' }}</span>
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
            Knowledge Gaps
          </RouterLink>
        </div>

        <div v-if="!selectedId" class="p-8 text-center text-sm text-tertiary">
          Select a gap from the list to see its evidence.
        </div>
        <div v-else-if="detailLoading" class="p-8 text-sm text-tertiary">Loading&hellip;</div>
        <article v-else-if="selectedGap" class="max-w-3xl mx-auto p-4 sm:p-6 flex flex-col gap-6">
          <!-- Title + actions -->
          <header class="flex items-start justify-between gap-4 pb-4 border-b border-subtle">
            <div class="flex-1 min-w-0">
              <h1 class="text-xl font-semibold text-primary">{{ selectedGap.title }}</h1>
              <p v-if="selectedGap.description" class="text-sm text-secondary mt-2">
                {{ selectedGap.description }}
              </p>
              <div class="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-tertiary">
                <span>Status: <span class="text-secondary">{{ selectedGap.status }}</span></span>
                <span>Impact: <span class="text-secondary">{{ selectedGap.impact_score }}</span></span>
                <span v-if="selectedGap.last_evidence_at">
                  Last evidence: {{ formatRelativeTime(selectedGap.last_evidence_at) }}
                </span>
              </div>
            </div>
            <button
              type="button"
              :disabled="isDismissing"
              class="flex-shrink-0 text-xs px-3 py-1.5 rounded-md text-secondary hover:text-status-error hover:bg-status-error-muted transition-colors disabled:opacity-50"
              @click="dismissCurrent"
            >
              Dismiss
            </button>
          </header>

          <!-- Evidence -->
          <section class="flex flex-col gap-3">
            <h2 class="text-xs font-semibold uppercase tracking-wide text-tertiary">
              Evidence
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
                  <RouterLink
                    v-if="signal.source_kind === 'ticket'"
                    :to="`/tickets/${signal.source_ref}`"
                    class="text-sm text-primary hover:text-accent transition-colors"
                  >
                    #{{ signal.source_ref }}
                    <span v-if="signal.ticket_title" class="text-secondary">
                      &middot; {{ signal.ticket_title }}
                    </span>
                  </RouterLink>
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
                    Flagged by <span class="text-secondary">{{ signal.detected_by_user.name }}</span>
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
            <p v-else class="text-sm text-tertiary">No evidence rows.</p>
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
            <p class="font-medium text-primary mb-1">Resolve this gap</p>
            <p>
              Open one of the tickets above and use
              <span class="font-medium text-primary">Save as doc</span> from its sidebar.
              The new doc will auto-link as 'resolves' on every flagged ticket.
            </p>
          </section>
        </article>
    </section>
  </div>
</template>
