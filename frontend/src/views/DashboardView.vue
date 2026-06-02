<script setup lang="ts">
import { computed, onMounted, provide, ref, watch, type ComponentPublicInstance } from 'vue'
import { onBeforeRouteLeave } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { useAuthStore } from '@/stores/auth'
import { useDashboardGreeting } from '@/composables/useDashboardGreeting'
import { useDashboardLayoutStore } from '@/stores/dashboardLayout'
import {
  DASHBOARD_STATS_KEY,
  useDashboardStats,
} from '@/composables/useDashboardStats'
import { useCreateTicketAction } from '@/composables/useCreateTicketAction'
import DashboardGrid from './dashboard/DashboardGrid.vue'
import DashboardEditBar from './dashboard/DashboardEditBar.vue'
import AnchorRail from './dashboard/AnchorRail.vue'
import TimeRangeChipCluster from './dashboard/chrome/TimeRangeChipCluster.vue'
import CompareToggle from './dashboard/chrome/CompareToggle.vue'
import AnnotationsToggle from './dashboard/chrome/AnnotationsToggle.vue'
import RefreshButton from './dashboard/chrome/RefreshButton.vue'
import Icon from '@/components/common/Icon.vue'
import { SECTIONS } from './dashboard/sections'
import { useAnchorScroll } from '@/composables/useAnchorScroll'
import { useDashboardKeybindings } from '@/composables/useDashboardKeybindings'

const authStore = useAuthStore()
const dashboardLayout = useDashboardLayoutStore()
const fluent = useFluent()

// Single shared stats query for all stat-bearing widgets. The
// coordinator collects `dataNeeds` from active widgets and fires
// one /api/dashboard/stats request that serves them all. Widgets
// inject the handle via `useInjectedDashboardStats()`.
const dashboardStats = useDashboardStats()
provide(DASHBOARD_STATS_KEY, dashboardStats)

// Refresh timestamp: when the page last successfully re-fetched
// its non-SSE data. Updates when the user clicks the refresh
// button or presses R (registered by useDashboardKeybindings in
// Wave 6+). Wave 1 stamps it on mount and on click; Wave 6 wires
// up the per-widget refresh-on-event handling.
const refreshedAt = ref<string | null>(null)
function refreshPage(): void {
  // Wave 1: the regular dashboard widgets re-fetch via
  // useDashboardStats; bump the underlying query's refetch and
  // stamp the timestamp so the RefreshButton's "Updated X ago"
  // resets. Wave 4+ widgets that subscribe to chart endpoints
  // each register their own refetch handler against this same
  // event in later waves.
  dashboardStats.refetch?.()
  refreshedAt.value = new Date().toISOString()
}

// First name only; the greeting template substitutes `{0}`.
// "Guest" fallback fires on the rare race where the dashboard
// mounts before /auth/me settles — falls through to the active
// locale's word for guest.
const username = computed(() => {
  if (!authStore.user?.name) return fluent.$t('dashboard-guest-fallback')
  return authStore.user.name.split(' ')[0]
})

const { currentTheme, formattedGreeting, subtitle } = useDashboardGreeting(username)

// The dashboard layout depends on the user's role (technician/admin
// vs end-user widget sets differ entirely). The store is created at
// module-load time with `auth.user === null`, so its initial layout
// is the end-user default. If we render the grid against that and
// auth then resolves to an admin, every admin-only widget mounts
// fresh in a second render — which is what caused the
// Unassigned-queue / Assigned-tickets "doesn't load half the time"
// race: the role-swap remount fires concurrent fetches against an
// auth state still settling.
//
// `authReady` gates the grid on `auth.user.uuid` being set, so the
// grid only mounts once with the correct role. The watch below
// still reloads the layout if the user object re-arrives later
// (SSO refresh, profile refetch).
const authReady = computed(() => !!authStore.user?.uuid)

onMounted(() => dashboardLayout.loadFromUser())
watch(() => authStore.user?.uuid, () => dashboardLayout.loadFromUser())

function enterEditMode() {
  dashboardLayout.beginEdit()
}

// Navigate-away guard: prompt the user to confirm if they're
// leaving with pending edits. Discard semantics — they explicitly
// chose to lose the changes — vs. cancel which keeps them on the
// dashboard so they can hit Done.
onBeforeRouteLeave((_to, _from, next) => {
  if (!dashboardLayout.isDirty) {
    next()
    return
  }
  const confirmed = window.confirm(fluent.$t('dashboard-leave-confirm'))
  if (confirmed) {
    dashboardLayout.discard()
    next()
  } else {
    next(false)
  }
})

useCreateTicketAction()

// Anchor rail integration: the rail (xl+) lists the canonical
// SECTIONS and highlights the active one as the user scrolls. The
// dashboard canvas seeds an H2 marker per section here (Wave 8) so
// the rail's clicks land somewhere reasonable even before per-
// section widget grouping ships.
const anchorScroll = useAnchorScroll()
function registerAnchor(id: string) {
  // Vue's template ref accepts `unknown` so the runtime can pass
  // either an Element (DOM) or a component instance. The
  // anchor-scroll composable only needs an Element-or-null, so we
  // narrow at the boundary and silently drop instance-shaped
  // refs (the H2 only ever yields an Element).
  return (el: Element | ComponentPublicInstance | null) => {
    anchorScroll.register(id, el instanceof Element ? el : null)
  }
}

useDashboardKeybindings({
  anchorScroll,
  onEditMode: enterEditMode,
  onRefresh: refreshPage,
})
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- Two-column layout on xl+: AnchorRail (left, sticky) and the
         canvas (right). Below xl the rail collapses (its `hidden
         xl:flex` class) and the canvas owns the full width. -->
    <div class="flex gap-6 p-4 sm:px-6">
      <AnchorRail :anchor-scroll="anchorScroll" class="xl:w-40 xl:flex-shrink-0" />

      <div class="flex flex-col gap-3 flex-1 min-w-0">
        <!-- Chrome row: greeting (left), time-range + compare +
             annotations + refresh + Edit (right). Stable across
             every dashboard render; widget add / time-range
             switches / refreshes act against the same header. -->
        <header class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
          <div class="min-w-0 flex-1">
            <h2 class="text-lg sm:text-xl font-medium text-primary flex items-center gap-3">
              <span v-if="currentTheme === 'red-horizon'" class="hal-eye flex-shrink-0" aria-hidden="true">
                <span class="hal-eye-inner"></span>
              </span>
              <span>{{ formattedGreeting }}</span>
            </h2>
            <p class="text-xs text-secondary mt-0.5">
              {{ subtitle }}
            </p>
          </div>
          <div class="flex flex-wrap items-center gap-2">
            <TimeRangeChipCluster />
            <CompareToggle />
            <AnnotationsToggle />
            <RefreshButton
              :updated-at="refreshedAt"
              @refresh="refreshPage"
            />
            <button
              v-if="!dashboardLayout.editMode"
              type="button"
              class="inline-flex items-center gap-1.5 rounded-md border border-default bg-surface px-2 py-1 text-xs text-secondary transition-colors hover:bg-surface-hover hover:text-primary"
              @click="enterEditMode"
            >
              <Icon name="rename" class="w-3.5 h-3.5" />
              <span>{{ $t('dashboard-edit-button') }}</span>
            </button>
          </div>
        </header>

        <DashboardEditBar v-if="dashboardLayout.editMode" />

        <!-- Section anchor markers. Each marker is the
             IntersectionObserver target the AnchorRail tracks; the
             visible H2 doubles as the in-canvas section heading so
             scrolling top-to-bottom reads the same section
             structure on rail-less viewports (< xl). The widget-
             to-section assignment lands in a follow-up; for now
             every marker sits above the single shared widget grid. -->
        <div class="flex flex-col gap-2">
          <h2
            v-for="section in SECTIONS"
            :key="section.id"
            :id="section.id"
            :ref="registerAnchor(section.id)"
            class="text-xs uppercase tracking-wide text-tertiary font-semibold scroll-mt-20"
          >
            {{ $t(section.labelKey) }}
          </h2>
        </div>

        <DashboardGrid v-if="authReady" />
      </div>
    </div>
  </div>
</template>

<style scoped>
/* HAL 9000 eye — CSS only, only painted on the red-horizon theme. */
.hal-eye {
  width: 22px;
  height: 22px;
  border-radius: 50%;
  background:
    radial-gradient(
      circle at 50% 50%,
      transparent 0%,
      transparent 80%,
      #3a3a3a 81%,
      #1a1a1a 88%,
      #4a4a4a 92%,
      #2a2a2a 100%
    ),
    radial-gradient(
      circle at 50% 50%,
      transparent 0%,
      transparent 45%,
      #000000 46%,
      #050505 80%,
      transparent 81%
    ),
    radial-gradient(
      circle at 50% 50%,
      rgba(255, 120, 40, 0.95) 0%,
      rgba(255, 80, 0, 0.8) 18%,
      rgba(180, 40, 0, 0.6) 30%,
      rgba(100, 15, 0, 0.4) 42%,
      rgba(40, 5, 0, 0.3) 52%,
      rgba(10, 0, 0, 0.5) 60%,
      rgba(0, 0, 0, 0.8) 68%,
      rgba(0, 0, 0, 1) 75%
    ),
    #000;
  box-shadow:
    0 0 10px rgba(255, 80, 0, 0.5),
    inset 0 0 4px rgba(0, 0, 0, 1);
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.hal-eye-inner {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: radial-gradient(
    circle at 35% 35%,
    #ffffff 0%,
    #ffe0aa 25%,
    #ff7700 60%,
    #cc4400 100%
  );
  box-shadow:
    0 0 3px rgba(255, 200, 100, 1),
    0 0 6px rgba(255, 120, 0, 1),
    0 0 12px rgba(255, 80, 0, 0.7);
  animation: hal-pulse 4s ease-in-out infinite;
}

@keyframes hal-pulse {
  0%, 100% {
    box-shadow:
      0 0 3px rgba(255, 200, 100, 1),
      0 0 6px rgba(255, 120, 0, 1),
      0 0 12px rgba(255, 80, 0, 0.7);
  }
  50% {
    box-shadow:
      0 0 4px rgba(255, 220, 150, 1),
      0 0 10px rgba(255, 140, 0, 1),
      0 0 20px rgba(255, 80, 0, 0.8);
  }
}

@media (min-width: 640px) {
  .hal-eye {
    width: 26px;
    height: 26px;
  }
}
</style>
