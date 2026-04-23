<!--
Devices whose primary user is the current user.
-->
<script setup lang="ts">
import { useAuthStore } from '@/stores/auth'
import { getDevicesByUser } from '@/services/deviceService'
import type { Device } from '@/types'
import { useAsyncResource } from '@/composables/useAsyncResource'
import DashboardWidgetShell from './DashboardWidgetShell.vue'

const auth = useAuthStore()

const { data: devices, loading, error } = useAsyncResource<Device[]>(
  async () => {
    const uuid = auth.user?.uuid
    if (!uuid) return []
    return (await getDevicesByUser(uuid)).slice(0, 5)
  },
  [],
  'Failed to load devices',
)
</script>

<template>
  <DashboardWidgetShell
    title="My Devices"
    action-to="/devices"
    :loading="loading"
    :error="error"
    :empty="!error && devices.length === 0"
    empty-title="No devices assigned"
    empty-description="Devices linked to your account will show here."
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
            {{ d.model || 'Unknown model' }}<template v-if="d.hostname"> · {{ d.hostname }}</template>
          </p>
        </router-link>
      </li>
    </ul>
  </DashboardWidgetShell>
</template>
