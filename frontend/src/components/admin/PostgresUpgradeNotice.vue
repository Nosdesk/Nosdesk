<script setup lang="ts">
/**
 * Tells admins, once, that the next release needs a newer PostgreSQL than
 * this server runs. The server reports it on the system info it already
 * serves admins; a dismissal is remembered in this browser.
 */
import { onMounted, ref } from 'vue'
import apiClient from '@/services/apiConfig'

const DISMISSED_KEY = 'nosdesk:postgres-upgrade-notice-dismissed'

interface PostgresInfo {
  postgres_upgrade_needed?: boolean
  upgrade_guide_url?: string
}

const guideUrl = ref<string | null>(null)

function dismissed(): boolean {
  try {
    return localStorage.getItem(DISMISSED_KEY) === '1'
  } catch {
    return false
  }
}

function dismiss(): void {
  guideUrl.value = null
  try {
    localStorage.setItem(DISMISSED_KEY, '1')
  } catch {
    // Private windows: the notice comes back next time.
  }
}

onMounted(async () => {
  if (dismissed()) return
  try {
    const { data } = await apiClient.get<PostgresInfo>('/admin/system/info')
    if (data.postgres_upgrade_needed && data.upgrade_guide_url) {
      guideUrl.value = data.upgrade_guide_url
    }
  } catch {
    // Not an admin, or the server is unreachable: nothing to say.
  }
})

defineExpose({ dismiss })
</script>

<template>
  <div
    v-if="guideUrl"
    role="status"
    class="m-4 mb-0 flex items-start gap-3 rounded-xl border border-status-warning/30 bg-status-warning/10 p-4 text-sm text-primary"
  >
    <div class="flex-1 min-w-0">
      {{ $t('admin-postgres-upgrade-notice') }}
      <a
        :href="guideUrl"
        target="_blank"
        rel="noopener noreferrer"
        class="ml-1 font-medium text-accent hover:underline"
      >{{ $t('admin-postgres-upgrade-notice-link') }}</a>
    </div>
    <button
      type="button"
      class="flex-shrink-0 rounded p-1 text-secondary hover:bg-surface-hover hover:text-primary"
      :aria-label="$t('admin-postgres-upgrade-notice-dismiss')"
      :title="$t('admin-postgres-upgrade-notice-dismiss')"
      @click="dismiss"
    >
      <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" aria-hidden="true">
        <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
      </svg>
    </button>
  </div>
</template>
