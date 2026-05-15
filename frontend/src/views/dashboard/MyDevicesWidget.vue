<!--
Devices whose primary user is the current user.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQuery } from '@pinia/colada'
import { useAuthStore } from '@/stores/auth'
import { getDevicesByUser } from '@/services/deviceService'
import type { Device } from '@/types'
import DashboardWidgetShell from './DashboardWidgetShell.vue'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

const auth = useAuthStore()

// Reactive key includes the user uuid so a session swap (login as
// a different user) yields a fresh cache entry rather than serving
// the previous user's devices.
// `isPending` is true only on the initial fetch (no cached data yet).
// `isLoading` is true on every in-flight request, so we use it for
// the background-refresh shimmer rather than the body-blanking
// skeleton, otherwise the widget would flash on every dashboard
// remount when Pinia Colada serves cached data while refetching.
const { data, isPending, isLoading, error } = useQuery({
  key: () => ['devices', 'by-user', auth.user?.uuid ?? ''],
  query: async () => {
    const uuid = auth.user?.uuid
    if (!uuid) return [] as Device[]
    return (await getDevicesByUser(uuid)).slice(0, 5)
  },
  enabled: () => !!auth.user?.uuid,
})

const devices = computed<Device[]>(() => data.value ?? [])
const isRefreshing = computed(() => isLoading.value && data.value !== undefined)
const errorMessage = computed(() =>
  error.value ? t('dashboard-my-devices-error') : null,
)
</script>

<template>
  <DashboardWidgetShell
    :title="t('dashboard-my-devices-title')"
    action-to="/devices"
    :loading="isPending"
    :refreshing="isRefreshing"
    :error="errorMessage"
    :empty="!errorMessage && devices.length === 0"
    :empty-title="t('dashboard-my-devices-empty-title')"
    :empty-description="t('dashboard-my-devices-empty-description')"
    min-body-height="200px"
  >
    <ul class="divide-y divide-default">
      <li v-for="d in devices" :key="d.id">
        <router-link
          :to="`/devices/${d.id}`"
          class="block px-4 py-2 hover:bg-surface-hover transition-colors group"
        >
          <p class="text-sm text-primary truncate group-hover:text-accent transition-colors">{{ d.name }}</p>
          <p class="mt-0.5 text-[11px] text-tertiary truncate">
            {{ d.model || t('dashboard-my-devices-unknown-model') }}<template v-if="d.hostname"> · {{ d.hostname }}</template>
          </p>
        </router-link>
      </li>
    </ul>
  </DashboardWidgetShell>
</template>
