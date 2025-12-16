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
      <SystemInfoCard />

      <!-- Storage Management Section -->
      <div class="bg-surface border border-default rounded-xl hover:border-strong transition-colors">
        <div class="p-4 flex flex-col gap-3">
          <!-- Header row with icon -->
          <div class="flex items-center gap-3">
            <!-- Storage icon -->
            <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-red-500/20 flex items-center justify-center text-red-400">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
              </svg>
            </div>

            <!-- Title -->
            <div class="flex-1">
              <span class="font-medium text-primary">Storage Management</span>
            </div>

            <!-- Action button -->
            <button
              @click="cleanupStaleImages"
              :disabled="isCleaningUp"
              class="px-3 py-1.5 bg-red-500/20 text-red-400 border border-red-500/50 rounded-lg text-sm hover:bg-red-500/30 font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-1.5 whitespace-nowrap"
            >
              <svg v-if="isCleaningUp" class="animate-spin h-3.5 w-3.5" fill="none" viewBox="0 0 24 24">
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
              <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
              </svg>
              {{ isCleaningUp ? 'Cleaning...' : 'Clean Up' }}
            </button>
          </div>

          <!-- Description -->
          <p class="text-secondary text-sm">
            Remove old user profile images and avatars that are no longer needed to free up disk space.
          </p>
        </div>

        <!-- Cleanup Results -->
        <div v-if="cleanupResults" class="border-t border-default p-4 bg-surface-alt">
          <div class="flex items-center gap-2 mb-3">
            <svg v-if="cleanupResults.success" class="w-4 h-4 text-status-success" fill="currentColor" viewBox="0 0 20 20">
              <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z" clip-rule="evenodd" />
            </svg>
            <svg v-else class="w-4 h-4 text-status-error" fill="currentColor" viewBox="0 0 20 20">
              <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.707 7.293a1 1 0 00-1.414 1.414L8.586 10l-1.293 1.293a1 1 0 101.414 1.414L10 11.414l1.293 1.293a1 1 0 001.414-1.414L11.414 10l1.293-1.293a1 1 0 00-1.414-1.414L10 8.586 8.707 7.293z" clip-rule="evenodd" />
            </svg>
            <span class="text-sm font-medium" :class="cleanupResults.success ? 'text-status-success' : 'text-status-error'">
              {{ cleanupResults.success ? 'Cleanup Completed' : 'Cleanup Failed' }}
            </span>
          </div>

          <div v-if="cleanupResults.success && cleanupResults.stats" class="grid grid-cols-2 sm:grid-cols-5 gap-2 text-sm">
            <div><span class="text-tertiary">Avatars:</span> <span class="text-primary">{{ cleanupResults.stats.avatars_removed }}</span></div>
            <div><span class="text-tertiary">Banners:</span> <span class="text-primary">{{ cleanupResults.stats.banners_removed }}</span></div>
            <div><span class="text-tertiary">Thumbnails:</span> <span class="text-primary">{{ cleanupResults.stats.thumbnails_removed || 0 }}</span></div>
            <div><span class="text-tertiary">Checked:</span> <span class="text-primary">{{ cleanupResults.stats.total_files_checked }}</span></div>
            <div><span class="text-tertiary">Errors:</span> <span :class="cleanupResults.stats.errors.length > 0 ? 'text-amber-400' : 'text-primary'">{{ cleanupResults.stats.errors.length }}</span></div>
          </div>

          <!-- Show errors if any -->
          <div v-if="cleanupResults.success && cleanupResults.stats?.errors.length > 0" class="mt-3">
            <details class="text-sm">
              <summary class="cursor-pointer text-amber-400 hover:text-amber-300">
                View Errors ({{ cleanupResults.stats.errors.length }})
              </summary>
              <div class="mt-2 pl-4 border-l-2 border-amber-500/50 text-secondary">
                <div v-for="(error, index) in cleanupResults.stats.errors" :key="index" class="mb-1">
                  {{ error }}
                </div>
              </div>
            </details>
          </div>

          <div v-if="!cleanupResults.success" class="text-sm text-status-error">
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
import BackButton from '@/components/common/BackButton.vue'
import SystemInfoCard from '@/components/admin/SystemInfoCard.vue'
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

// Reactive data
const isCleaningUp = ref(false)
const cleanupResults = ref<CleanupResults | null>(null)

// Check if user is admin
onMounted(() => {
  if (!authStore.user || authStore.user.role !== 'admin') {
    router.push('/admin')
    return
  }
})

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