<template>
  <div class="flex-1">
    <!-- Navigation and actions bar -->
    <div class="pt-4 px-6 flex justify-between items-center">
      <BackButton fallbackRoute="/admin/settings" label="Back to Administration" />
    </div>
    
    <div class="flex flex-col gap-4 px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-6">
        <h1 class="text-2xl font-bold text-white">System Settings</h1>
        <p class="text-slate-400 mt-2">
          Manage system maintenance, storage, and configuration options
        </p>
      </div>

      <!-- Storage Management Section -->
      <div class="mb-8">
        <h2 class="text-xl font-semibold text-white mb-4">Storage Management</h2>
        
        <!-- Stale Image Cleanup Card -->
        <div class="bg-slate-800 border border-slate-700 rounded-lg p-6 mb-4">
          <div class="flex items-start justify-between">
            <div class="flex-1">
              <h3 class="text-lg font-medium text-white mb-2">Clean Up Stale Images</h3>
              <p class="text-slate-400 mb-4">
                Remove old user profile images and avatars that are no longer needed. This will free up disk space
                and clean up files created by the old naming system.
              </p>
              
              <!-- Cleanup Results -->
              <div v-if="cleanupResults" class="mb-4 p-4 rounded-lg" :class="cleanupResults.success ? 'bg-green-900/20 border border-green-700' : 'bg-red-900/20 border border-red-700'">
                <div class="flex items-center mb-2">
                  <svg v-if="cleanupResults.success" class="w-5 h-5 text-green-400 mr-2" fill="currentColor" viewBox="0 0 20 20">
                    <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z" clip-rule="evenodd" />
                  </svg>
                  <svg v-else class="w-5 h-5 text-red-400 mr-2" fill="currentColor" viewBox="0 0 20 20">
                    <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.707 7.293a1 1 0 00-1.414 1.414L8.586 10l-1.293 1.293a1 1 0 101.414 1.414L10 11.414l1.293 1.293a1 1 0 001.414-1.414L11.414 10l1.293-1.293a1 1 0 00-1.414-1.414L10 8.586 8.707 7.293z" clip-rule="evenodd" />
                  </svg>
                  <span class="font-medium" :class="cleanupResults.success ? 'text-green-400' : 'text-red-400'">
                    {{ cleanupResults.success ? 'Cleanup Completed Successfully' : 'Cleanup Failed' }}
                  </span>
                </div>
                
                <div v-if="cleanupResults.success && cleanupResults.stats" class="text-sm text-slate-300">
                  <div class="grid grid-cols-2 md:grid-cols-5 gap-4">
                    <div>
                      <span class="text-slate-400">Avatars Removed:</span>
                      <span class="ml-1 font-medium">{{ cleanupResults.stats.avatars_removed }}</span>
                    </div>
                    <div>
                      <span class="text-slate-400">Banners Removed:</span>
                      <span class="ml-1 font-medium">{{ cleanupResults.stats.banners_removed }}</span>
                    </div>
                    <div>
                      <span class="text-slate-400">Thumbnails Removed:</span>
                      <span class="ml-1 font-medium">{{ cleanupResults.stats.thumbnails_removed || 0 }}</span>
                    </div>
                    <div>
                      <span class="text-slate-400">Files Checked:</span>
                      <span class="ml-1 font-medium">{{ cleanupResults.stats.total_files_checked }}</span>
                    </div>
                    <div>
                      <span class="text-slate-400">Errors:</span>
                      <span class="ml-1 font-medium" :class="cleanupResults.stats.errors.length > 0 ? 'text-yellow-400' : 'text-green-400'">
                        {{ cleanupResults.stats.errors.length }}
                      </span>
                    </div>
                  </div>
                  
                  <!-- Show errors if any -->
                  <div v-if="cleanupResults.stats.errors.length > 0" class="mt-3">
                    <details class="text-sm">
                      <summary class="cursor-pointer text-yellow-400 hover:text-yellow-300">
                        View Errors ({{ cleanupResults.stats.errors.length }})
                      </summary>
                      <div class="mt-2 pl-4 border-l-2 border-yellow-400">
                        <div v-for="(error, index) in cleanupResults.stats.errors" :key="index" class="text-yellow-300 mb-1">
                          {{ error }}
                        </div>
                      </div>
                    </details>
                  </div>
                </div>
                
                <div v-if="!cleanupResults.success" class="text-sm text-red-300">
                  {{ cleanupResults.message }}
                </div>
              </div>
            </div>
            
            <div class="ml-6">
              <button
                @click="cleanupStaleImages"
                :disabled="isCleaningUp"
                class="px-4 py-2 bg-red-600 hover:bg-red-700 disabled:bg-red-800 disabled:cursor-not-allowed text-white rounded-lg transition-colors flex items-center"
              >
                <svg v-if="isCleaningUp" class="animate-spin -ml-1 mr-2 h-4 w-4 text-white" fill="none" viewBox="0 0 24 24">
                  <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                  <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                </svg>
                <svg v-else class="w-4 h-4 mr-2" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                </svg>
                {{ isCleaningUp ? 'Cleaning...' : 'Clean Up Now' }}
              </button>
            </div>
          </div>
        </div>
      </div>

      <!-- System Information Section -->
      <div class="mb-8">
        <h2 class="text-xl font-semibold text-white mb-4">System Information</h2>
        
        <div class="bg-slate-800 border border-slate-700 rounded-lg p-6">
          <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
            <div>
              <h3 class="text-sm font-medium text-slate-400 mb-1">Application Version</h3>
              <p class="text-white">{{ systemInfo.version }}</p>
            </div>
            <div>
              <h3 class="text-sm font-medium text-slate-400 mb-1">Environment</h3>
              <p class="text-white">{{ systemInfo.environment }}</p>
            </div>
            <div>
              <h3 class="text-sm font-medium text-slate-400 mb-1">Uptime</h3>
              <p class="text-white">{{ systemInfo.uptime }}</p>
            </div>
          </div>
        </div>
      </div>

      <!-- Maintenance Section -->
      <div class="mb-8">
        <h2 class="text-xl font-semibold text-white mb-4">Maintenance</h2>
        
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
          <!-- Database Optimization -->
          <div class="bg-slate-800 border border-slate-700 rounded-lg p-6">
            <h3 class="text-lg font-medium text-white mb-2">Database Optimization</h3>
            <p class="text-slate-400 mb-4">Optimize database performance and clean up unused data.</p>
            <button
              class="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg transition-colors"
              disabled
            >
              Coming Soon
            </button>
          </div>

          <!-- Cache Management -->
          <div class="bg-slate-800 border border-slate-700 rounded-lg p-6">
            <h3 class="text-lg font-medium text-white mb-2">Cache Management</h3>
            <p class="text-slate-400 mb-4">Clear application caches and temporary files.</p>
            <button
              class="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg transition-colors"
              disabled
            >
              Coming Soon
            </button>
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
  uptime: string
}

// Reactive data
const isCleaningUp = ref(false)
const cleanupResults = ref<CleanupResults | null>(null)
const systemInfo = ref<SystemInfo>({
  version: '1.0.0',
  environment: 'Development',
  uptime: 'Loading...'
})

// Check if user is admin
onMounted(() => {
  if (!authStore.user || authStore.user.role !== 'admin') {
    router.push('/admin')
    return
  }
  
  // Load system info
  loadSystemInfo()
})

const loadSystemInfo = () => {
  // Mock system info for now
  systemInfo.value = {
    version: '1.0.0',
    environment: import.meta.env.MODE || 'development',
    uptime: formatUptime(Date.now() - (performance.timeOrigin || 0))
  }
}

const formatUptime = (milliseconds: number): string => {
  const seconds = Math.floor(milliseconds / 1000)
  const minutes = Math.floor(seconds / 60)
  const hours = Math.floor(minutes / 60)
  const days = Math.floor(hours / 24)
  
  if (days > 0) {
    return `${days}d ${hours % 24}h ${minutes % 60}m`
  } else if (hours > 0) {
    return `${hours}h ${minutes % 60}m`
  } else if (minutes > 0) {
    return `${minutes}m ${seconds % 60}s`
  } else {
    return `${seconds}s`
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