<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <div>
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('admin-system-title') }}</h1>
      </div>

      <!-- System Information Section -->
      <SystemInfoCard />

      <!-- Workspace data export (owner-only, self-serve DSAR return) -->
      <WorkspaceDataExportCard v-if="authStore.isOwner" />

      <!-- Storage Management Section. Instance-wide storage maintenance is a
           platform-operator action (backend requires platform_admin); hidden
           from workspace admins, who would only hit permission denied. -->
      <div v-if="authStore.isPlatformAdmin" class="bg-surface border border-default rounded-xl hover:border-strong transition-colors">
        <div class="p-4 flex flex-col gap-3">
          <!-- Header row with icon -->
          <div class="flex items-center gap-3">
            <!-- Storage icon -->
            <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-status-error/20 flex items-center justify-center text-status-error">
              <Icon name="trash" size="md" />
            </div>

            <!-- Title -->
            <div class="flex-1">
              <span class="font-medium text-primary">{{ $t('admin-system-storage-title') }}</span>
            </div>

            <!-- Action button -->
            <Button
              variant="danger"
              size="sm"
              icon="trash"
              :loading="isCleaningUp"
              @click="cleanupStaleImages"
            >
              {{ isCleaningUp ? $t('admin-system-storage-cleaning') : $t('admin-system-storage-clean') }}
            </Button>
          </div>

          <!-- Description -->
          <p class="text-secondary text-sm">
            {{ $t('admin-system-storage-description') }}
          </p>
        </div>

        <!-- Cleanup Results -->
        <div v-if="cleanupResults" class="border-t border-default p-4 bg-surface-alt rounded-b-xl">
          <div class="flex items-center gap-2 mb-3">
            <Icon v-if="cleanupResults.success" name="checkCircle" class="text-status-success" />
            <Icon v-else name="warning" class="text-status-error" />
            <span class="text-sm font-medium" :class="cleanupResults.success ? 'text-status-success' : 'text-status-error'">
              {{ cleanupResults.success ? $t('admin-system-cleanup-success') : $t('admin-system-cleanup-failed') }}
            </span>
          </div>

          <div v-if="cleanupResults.success && cleanupResults.stats" class="grid grid-cols-2 sm:grid-cols-5 gap-2 text-sm">
            <div><span class="text-tertiary">{{ $t('admin-system-cleanup-stat-avatars') }}</span> <span class="text-primary">{{ cleanupResults.stats.avatars_removed }}</span></div>
            <div><span class="text-tertiary">{{ $t('admin-system-cleanup-stat-banners') }}</span> <span class="text-primary">{{ cleanupResults.stats.banners_removed }}</span></div>
            <div><span class="text-tertiary">{{ $t('admin-system-cleanup-stat-thumbnails') }}</span> <span class="text-primary">{{ cleanupResults.stats.thumbnails_removed || 0 }}</span></div>
            <div><span class="text-tertiary">{{ $t('admin-system-cleanup-stat-checked') }}</span> <span class="text-primary">{{ cleanupResults.stats.total_files_checked }}</span></div>
            <div><span class="text-tertiary">{{ $t('admin-system-cleanup-stat-errors') }}</span> <span :class="(cleanupResults.stats?.errors?.length ?? 0) > 0 ? 'text-status-warning' : 'text-primary'">{{ cleanupResults.stats?.errors?.length ?? 0 }}</span></div>
          </div>

          <!-- Show errors if any -->
          <div v-if="cleanupResults.success && (cleanupResults.stats?.errors?.length ?? 0) > 0" class="mt-3">
            <details class="text-sm">
              <summary class="cursor-pointer text-status-warning hover:text-status-warning/80">
                {{ $t('admin-system-cleanup-view-errors', { count: cleanupResults.stats?.errors?.length ?? 0 }) }}
              </summary>
              <div class="mt-2 pl-4 border-l-2 border-status-warning/50 text-secondary">
                <div v-for="(error, index) in cleanupResults.stats?.errors ?? []" :key="index" class="mb-1">
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

      <!-- Thumbnail Regeneration Section. Same as storage cleanup: an
           instance-wide platform-operator action, hidden from workspace admins. -->
      <div v-if="authStore.isPlatformAdmin" class="bg-surface border border-default rounded-xl hover:border-strong transition-colors">
        <div class="p-4 flex flex-col gap-3">
          <div class="flex items-center gap-3">
            <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-accent/20 flex items-center justify-center text-accent">
              <Icon name="refresh" size="md" />
            </div>

            <div class="flex-1">
              <span class="font-medium text-primary">{{ $t('admin-system-thumbnails-title') }}</span>
            </div>

            <Button
              variant="secondary"
              size="sm"
              icon="refresh"
              :loading="isRegenerating"
              @click="regenerateThumbnails"
            >
              {{ isRegenerating ? $t('admin-system-thumbnails-running') : $t('admin-system-thumbnails-action') }}
            </Button>
          </div>

          <p class="text-secondary text-sm">
            {{ $t('admin-system-thumbnails-description') }}
          </p>
        </div>

        <!-- Regeneration Results -->
        <div v-if="thumbnailResults" class="border-t border-default p-4 bg-surface-alt rounded-b-xl">
          <div class="flex items-center gap-2 mb-3">
            <Icon v-if="thumbnailResults.success" name="checkCircle" class="text-status-success" />
            <Icon v-else name="warning" class="text-status-error" />
            <span class="text-sm font-medium" :class="thumbnailResults.success ? 'text-status-success' : 'text-status-error'">
              {{ thumbnailResults.success ? $t('admin-system-thumbnails-success') : $t('admin-system-thumbnails-failed') }}
            </span>
          </div>

          <div v-if="thumbnailResults.success && thumbnailResults.stats" class="grid grid-cols-3 gap-2 text-sm">
            <div><span class="text-tertiary">{{ $t('admin-system-thumbnails-stat-checked') }}</span> <span class="text-primary">{{ thumbnailResults.stats.checked }}</span></div>
            <div><span class="text-tertiary">{{ $t('admin-system-thumbnails-stat-regenerated') }}</span> <span class="text-primary">{{ thumbnailResults.stats.regenerated }}</span></div>
            <div><span class="text-tertiary">{{ $t('admin-system-thumbnails-stat-failed') }}</span> <span :class="thumbnailResults.stats.failed > 0 ? 'text-status-warning' : 'text-primary'">{{ thumbnailResults.stats.failed }}</span></div>
          </div>

          <div v-if="!thumbnailResults.success" class="text-sm text-status-error">
            {{ thumbnailResults.message }}
          </div>
        </div>
      </div>
    </div>

    <ConfirmModal
      :show="showCleanupConfirm"
      variant="danger"
      :title="$t('admin-system-storage-confirm-title')"
      :message="$t('admin-system-storage-confirm-message')"
      :confirm-label="$t('admin-system-storage-confirm-label')"
      @confirm="doCleanupStaleImages"
      @close="showCleanupConfirm = false"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'

import SystemInfoCard from '@/components/admin/SystemInfoCard.vue'
import WorkspaceDataExportCard from '@/components/settings/WorkspaceDataExportCard.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import Icon from '@/components/common/Icon.vue'
import Button from '@/components/common/Button.vue'
import userService from '@/services/userService'

const authStore = useAuthStore()
const router = useRouter()
const fluent = useFluent()
const t = (key: string) => fluent.$t(key)

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

interface ThumbnailResults {
  success: boolean
  message?: string
  stats?: {
    checked: number
    regenerated: number
    failed: number
  }
}

// Reactive data
const isCleaningUp = ref(false)
const cleanupResults = ref<CleanupResults | null>(null)
const isRegenerating = ref(false)
const thumbnailResults = ref<ThumbnailResults | null>(null)

// Check if user is admin
onMounted(() => {
  if (!authStore.isAdmin) {
    router.push('/admin')
    return
  }
})

const showCleanupConfirm = ref(false)

const cleanupStaleImages = () => {
  if (isCleaningUp.value) return
  showCleanupConfirm.value = true
}

const doCleanupStaleImages = async () => {
  showCleanupConfirm.value = false
  isCleaningUp.value = true
  cleanupResults.value = null

  try {
    const data = await userService.cleanupStaleImages()
    cleanupResults.value = data
  } catch (error) {
    console.error('Error cleaning up stale images:', error)
    cleanupResults.value = {
      success: false,
      message: t('admin-system-cleanup-error-unexpected')
    }
  } finally {
    isCleaningUp.value = false
  }
}

const regenerateThumbnails = async () => {
  if (isRegenerating.value) return
  isRegenerating.value = true
  thumbnailResults.value = null

  try {
    const data = await userService.regenerateThumbnails()
    thumbnailResults.value = data
  } catch (error) {
    console.error('Error regenerating thumbnails:', error)
    thumbnailResults.value = {
      success: false,
      message: t('admin-system-thumbnails-error-unexpected')
    }
  } finally {
    isRegenerating.value = false
  }
}
</script> 