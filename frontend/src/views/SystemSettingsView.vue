<template>
  <div class="flex-1">
    <!-- Navigation and actions bar -->
    <div class="pt-4 px-6 flex justify-between items-center">
      <BackButton fallbackRoute="/admin/settings" label="Back to Administration" />
    </div>

    <div class="flex flex-col gap-6 px-6 py-4 mx-auto w-full max-w-8xl">
      <div>
        <h1 class="text-2xl font-bold text-primary">System Settings</h1>
      </div>

      <!-- System Information Section -->
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
        <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
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
            <p class="text-primary font-medium">{{ systemInfo.uptime_formatted }}</p>
          </div>
        </div>
      </div>

      <!-- Storage Management Section -->
      <div class="bg-surface border border-default rounded-xl p-6">
        <div class="flex flex-col md:flex-row md:items-center md:justify-between gap-4">
          <div class="flex-1">
            <h2 class="text-lg font-semibold text-primary mb-2">Storage Management</h2>
            <p class="text-secondary text-sm">
              Remove old user profile images and avatars that are no longer needed. This will free up disk space
              and clean up files created by the old naming system.
            </p>
          </div>

          <div class="flex-shrink-0">
            <button
              @click="cleanupStaleImages"
              :disabled="isCleaningUp"
              class="px-4 py-2 bg-surface hover:bg-surface-hover border border-red-500 text-primary rounded-lg transition-colors flex items-center disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <svg v-if="isCleaningUp" class="animate-spin -ml-1 mr-2 h-4 w-4" fill="none" viewBox="0 0 24 24">
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
              <svg v-else class="w-4 h-4 mr-2 text-red-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
              </svg>
              {{ isCleaningUp ? 'Cleaning...' : 'Clean Up Images' }}
            </button>
          </div>
        </div>

        <!-- Cleanup Results -->
        <div v-if="cleanupResults" class="mt-4 p-4 rounded-lg bg-surface-alt border" :class="cleanupResults.success ? 'border-green-500' : 'border-red-500'">
          <div class="flex items-center mb-3">
            <svg v-if="cleanupResults.success" class="w-5 h-5 text-green-500 mr-2" fill="currentColor" viewBox="0 0 20 20">
              <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z" clip-rule="evenodd" />
            </svg>
            <svg v-else class="w-5 h-5 text-red-500 mr-2" fill="currentColor" viewBox="0 0 20 20">
              <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.707 7.293a1 1 0 00-1.414 1.414L8.586 10l-1.293 1.293a1 1 0 101.414 1.414L10 11.414l1.293 1.293a1 1 0 001.414-1.414L11.414 10l1.293-1.293a1 1 0 00-1.414-1.414L10 8.586 8.707 7.293z" clip-rule="evenodd" />
            </svg>
            <span class="font-medium text-primary">
              {{ cleanupResults.success ? 'Cleanup Completed' : 'Cleanup Failed' }}
            </span>
          </div>

          <div v-if="cleanupResults.success && cleanupResults.stats" class="text-sm">
            <div class="grid grid-cols-2 sm:grid-cols-5 gap-3">
              <div class="bg-surface rounded p-2">
                <span class="text-tertiary text-xs block">Avatars</span>
                <span class="font-medium text-primary">{{ cleanupResults.stats.avatars_removed }}</span>
              </div>
              <div class="bg-surface rounded p-2">
                <span class="text-tertiary text-xs block">Banners</span>
                <span class="font-medium text-primary">{{ cleanupResults.stats.banners_removed }}</span>
              </div>
              <div class="bg-surface rounded p-2">
                <span class="text-tertiary text-xs block">Thumbnails</span>
                <span class="font-medium text-primary">{{ cleanupResults.stats.thumbnails_removed || 0 }}</span>
              </div>
              <div class="bg-surface rounded p-2">
                <span class="text-tertiary text-xs block">Checked</span>
                <span class="font-medium text-primary">{{ cleanupResults.stats.total_files_checked }}</span>
              </div>
              <div class="bg-surface rounded p-2">
                <span class="text-tertiary text-xs block">Errors</span>
                <span class="font-medium" :class="cleanupResults.stats.errors.length > 0 ? 'text-yellow-500' : 'text-primary'">
                  {{ cleanupResults.stats.errors.length }}
                </span>
              </div>
            </div>

            <!-- Show errors if any -->
            <div v-if="cleanupResults.stats.errors.length > 0" class="mt-3">
              <details class="text-sm">
                <summary class="cursor-pointer text-yellow-500 hover:text-yellow-400">
                  View Errors ({{ cleanupResults.stats.errors.length }})
                </summary>
                <div class="mt-2 pl-4 border-l-2 border-yellow-500">
                  <div v-for="(error, index) in cleanupResults.stats.errors" :key="index" class="text-secondary mb-1">
                    {{ error }}
                  </div>
                </div>
              </details>
            </div>
          </div>

          <div v-if="!cleanupResults.success" class="text-sm text-red-500">
            {{ cleanupResults.message }}
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useRouter } from 'vue-router'
import axios from 'axios'
import BackButton from '@/components/common/BackButton.vue'
import userService from '@/services/userService'

const authStore = useAuthStore()
const router = useRouter()

// Define types for cleanup results
interface CleanupStats {
  avatars_removed: number
  banners_removed: number
  thumbnails_removed?: number
  total_files_checked: number
  errors: string[]
}

interface CleanupResults {
  success: boolean
  message: string
  stats?: CleanupStats
}

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

// Reactive data
const isCleaningUp = ref(false)
const cleanupResults = ref<CleanupResults | null>(null)
const systemInfo = ref<SystemInfo>({
  version: '...',
  environment: '...',
  uptime_seconds: 0,
  uptime_formatted: '...'
})
const updateInfo = ref<UpdateInfo | null>(null)

// Check if user is admin
onMounted(() => {
  if (!authStore.user || authStore.user.role !== 'admin') {
    router.push('/admin')
    return
  }

  // Load system info immediately
  loadSystemInfo()
  // Check for updates asynchronously (don't block UI)
  checkForUpdates()
})

const loadSystemInfo = async () => {
  try {
    const response = await axios.get('/api/admin/system/info')
    systemInfo.value = response.data
  } catch (error) {
    console.error('Failed to load system info:', error)
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

const cleanupStaleImages = async () => {
  if (isCleaningUp.value) return

  // Confirm action
  if (!confirm('Are you sure you want to clean up stale images? This action cannot be undone.')) {
    return
  }

  isCleaningUp.value = true
  cleanupResults.value = null

  try {
    const data = await userService.cleanupStaleImages()
    cleanupResults.value = data
  } catch (error) {
    console.error('Error cleaning up stale images:', error)
    cleanupResults.value = {
      success: false,
      message: 'An unexpected error occurred while cleaning up images'
    }
  } finally {
    isCleaningUp.value = false
  }
}
</script> 