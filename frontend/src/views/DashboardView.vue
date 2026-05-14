<script setup lang="ts">
import { computed, onMounted, provide, watch } from 'vue'
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
import Icon from '@/components/common/Icon.vue'

const authStore = useAuthStore()
const dashboardLayout = useDashboardLayoutStore()
const fluent = useFluent()

// Single shared stats query for all stat-bearing widgets. The
// coordinator collects `dataNeeds` from active widgets and fires
// one /api/dashboard/stats request that serves them all. Widgets
// inject the handle via `useInjectedDashboardStats()`.
provide(DASHBOARD_STATS_KEY, useDashboardStats())

// First name only; the greeting template substitutes `{0}`.
// "Guest" fallback fires on the rare race where the dashboard
// mounts before /auth/me settles — falls through to the active
// locale's word for guest.
const username = computed(() => {
  if (!authStore.user?.name) return fluent.$t('dashboard-guest-fallback')
  return authStore.user.name.split(' ')[0]
})

const { currentTheme, formattedGreeting, subtitle } = useDashboardGreeting(username)

// Resolve the layout against the current user on mount and again if the
// user object re-arrives (SSO refresh, profile refetch).
onMounted(() => dashboardLayout.loadFromUser())
watch(() => authStore.user?.uuid, () => dashboardLayout.loadFromUser())

function enterEditMode() {
  dashboardLayout.editMode = true
}

useCreateTicketAction()
</script>

<template>
  <div class="flex flex-col h-full">
    <div class="flex flex-col gap-3 p-4 sm:px-6">
      <!-- Greeting + edit affordance. The Edit button is a subtle
           secondary, customising the dashboard is opt-in, not a
           primary flow. -->
      <header class="flex items-start justify-between gap-4">
        <div>
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
        <button
          v-if="!dashboardLayout.editMode"
          type="button"
          class="flex-shrink-0 inline-flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
          @click="enterEditMode"
        >
          <Icon name="rename" />
          {{ $t('dashboard-edit-button') }}
        </button>
      </header>

      <DashboardEditBar v-if="dashboardLayout.editMode" />

      <DashboardGrid />
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
