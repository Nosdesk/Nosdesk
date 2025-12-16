<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed } from 'vue'
import axios from 'axios'

interface SystemInfo {
  version: string
  environment: string
  uptime_seconds: number
  uptime_formatted: string
}

interface UpdateInfo {
  update_available: boolean
  current_version: string
  latest_version: string | null
  release_url: string | null
}

const systemInfo = ref<SystemInfo>({
  version: '...',
  environment: '...',
  uptime_seconds: 0,
  uptime_formatted: '...'
})
const updateInfo = ref<UpdateInfo | null>(null)
const isLoading = ref(true)

// Track uptime locally for live updates
const localUptimeSeconds = ref(0)
let uptimeInterval: ReturnType<typeof setInterval> | null = null

// Format uptime from seconds
const formattedUptime = computed(() => {
  const seconds = localUptimeSeconds.value
  if (seconds === 0) return '...'

  const days = Math.floor(seconds / 86400)
  const hours = Math.floor((seconds % 86400) / 3600)
  const mins = Math.floor((seconds % 3600) / 60)
  const secs = seconds % 60

  const parts: string[] = []
  if (days > 0) parts.push(`${days}d`)
  if (hours > 0) parts.push(`${hours}h`)
  if (mins > 0) parts.push(`${mins}m`)
  parts.push(`${secs}s`)

  return parts.join(' ')
})

const loadSystemInfo = async () => {
  try {
    const response = await axios.get('/api/admin/system/info')
    systemInfo.value = response.data
    localUptimeSeconds.value = response.data.uptime_seconds
    isLoading.value = false
  } catch (error) {
    console.error('Failed to load system info:', error)
    isLoading.value = false
  }
}

const checkForUpdates = async () => {
  try {
    const response = await axios.get('/api/admin/system/updates')
    updateInfo.value = response.data
  } catch (error) {
    console.error('Failed to check for updates:', error)
  }
}

// Start the uptime counter
const startUptimeCounter = () => {
  uptimeInterval = setInterval(() => {
    localUptimeSeconds.value++
  }, 1000)
}

onMounted(async () => {
  await loadSystemInfo()
  startUptimeCounter()
  // Check for updates asynchronously
  checkForUpdates()
})

onUnmounted(() => {
  if (uptimeInterval) {
    clearInterval(uptimeInterval)
    uptimeInterval = null
  }
})
</script>

<template>
  <div class="bg-surface border border-default rounded-xl p-6 flex flex-col gap-4">
    <div class="flex items-center justify-between">
      <h2 class="text-lg font-semibold text-primary">System Information</h2>
      <a
        v-if="updateInfo?.update_available && updateInfo?.release_url"
        :href="updateInfo.release_url"
        target="_blank"
        class="px-3 py-1.5 bg-surface hover:bg-surface-hover border border-green-500 text-primary rounded-lg text-sm font-medium transition-colors flex items-center gap-2"
      >
        <svg class="w-4 h-4 text-green-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
        </svg>
        Update to {{ updateInfo.latest_version }}
      </a>
    </div>

    <!-- Loading skeleton -->
    <div v-if="isLoading" class="grid grid-cols-1 sm:grid-cols-3 gap-4">
      <div v-for="i in 3" :key="i" class="bg-surface-alt rounded-lg p-4">
        <div class="h-3 w-16 bg-surface-hover rounded animate-pulse mb-2"></div>
        <div class="h-5 w-24 bg-surface-hover rounded animate-pulse"></div>
      </div>
    </div>

    <!-- System info cards -->
    <div v-else class="grid grid-cols-1 sm:grid-cols-3 gap-4">
      <div class="bg-surface-alt rounded-lg p-4">
        <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide mb-1">Version</h3>
        <p class="text-primary font-medium">{{ systemInfo.version }}</p>
      </div>
      <div class="bg-surface-alt rounded-lg p-4">
        <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide mb-1">Environment</h3>
        <p class="text-primary font-medium">{{ systemInfo.environment }}</p>
      </div>
      <div class="bg-surface-alt rounded-lg p-4">
        <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide mb-1">Uptime</h3>
        <p class="text-primary font-medium font-mono tabular-nums">{{ formattedUptime }}</p>
      </div>
    </div>
  </div>
</template>
