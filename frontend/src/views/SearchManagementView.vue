<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <div>
        <h1 class="text-xl sm:text-2xl font-bold text-primary">Search Index Management</h1>
        <p class="text-secondary mt-1">Manage the full-text search index for tickets, documentation, devices, and users.</p>
      </div>

      <!-- Index Statistics -->
      <div class="bg-surface border border-default rounded-xl">
        <div class="p-4 flex flex-col gap-3">
          <!-- Header row with icon -->
          <div class="flex items-center gap-3">
            <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-accent/15 flex items-center justify-center text-accent">
              <Icon name="insights" size="md" />
            </div>
            <div class="flex-1">
              <span class="font-medium text-primary">Index Statistics</span>
            </div>
            <button
              @click="fetchStats"
              :disabled="isLoadingStats"
              class="px-3 py-1.5 bg-surface-alt text-secondary border border-default rounded-lg text-sm hover:bg-surface-hover font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-1.5"
            >
              <Spinner v-if="isLoadingStats" />
              <Icon v-else name="refresh" />
              Refresh
            </button>
          </div>

          <!-- Stats Grid -->
          <div v-if="stats" class="grid grid-cols-2 sm:grid-cols-4 gap-4 mt-2">
            <div class="bg-surface-alt rounded-lg p-3">
              <div class="text-2xl font-bold text-primary">{{ stats.total_documents.toLocaleString() }}</div>
              <div class="text-xs text-secondary">Total Documents</div>
            </div>
            <div class="bg-surface-alt rounded-lg p-3">
              <div class="text-2xl font-bold text-primary">{{ formatBytes(stats.index_size_bytes) }}</div>
              <div class="text-xs text-secondary">Index Size</div>
            </div>
            <div class="bg-surface-alt rounded-lg p-3">
              <div class="flex items-center gap-2">
                <div v-if="stats.is_rebuilding" class="flex items-center gap-1.5 text-status-warning">
                  <Spinner size="md" />
                  <span class="text-lg font-bold">Rebuilding</span>
                </div>
                <div v-else class="flex items-center gap-1.5 text-status-success">
                  <Icon name="checkCircle" size="md" />
                  <span class="text-lg font-bold">Ready</span>
                </div>
              </div>
              <div class="text-xs text-secondary">Status</div>
            </div>
            <div class="bg-surface-alt rounded-lg p-3">
              <div class="text-2xl font-bold text-primary">{{ Object.keys(stats.by_type).length || 6 }}</div>
              <div class="text-xs text-secondary">Entity Types</div>
            </div>
          </div>

          <!-- Loading state -->
          <div v-else-if="isLoadingStats" class="flex items-center justify-center py-8">
            <Spinner size="lg" class="text-accent" />
          </div>

          <!-- Error state -->
          <div v-else-if="statsError" class="text-status-error text-sm py-4">
            {{ statsError }}
          </div>
        </div>
      </div>

      <!-- Rebuild Index Section -->
      <div class="bg-surface border border-default rounded-xl hover:border-strong transition-colors">
        <div class="p-4 flex flex-col gap-3">
          <!-- Header row with icon -->
          <div class="flex items-center gap-3">
            <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-status-warning/20 flex items-center justify-center text-status-warning">
              <Icon name="refresh" size="md" />
            </div>
            <div class="flex-1">
              <span class="font-medium text-primary">Rebuild Search Index</span>
            </div>
            <button
              @click="rebuildIndex"
              :disabled="isRebuilding"
              class="px-3 py-1.5 bg-status-warning/20 text-status-warning border border-status-warning/50 rounded-lg text-sm hover:bg-status-warning/30 font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-1.5 whitespace-nowrap"
            >
              <Spinner v-if="isRebuilding" />
              <Icon v-else name="refresh" />
              {{ isRebuilding ? 'Rebuilding...' : 'Rebuild Index' }}
            </button>
          </div>

          <!-- Description -->
          <p class="text-secondary text-sm">
            Rebuilds the entire search index from the database. This will re-index all tickets, comments, documentation pages, attachments, devices, and users. Use this if search results are missing or outdated.
          </p>
        </div>

        <!-- Rebuild Results -->
        <div v-if="rebuildResults" class="border-t border-default p-4 bg-surface-alt">
          <div class="flex items-center gap-2 mb-3">
            <Icon v-if="rebuildResults.success" name="checkCircle" class="text-status-success" />
            <Icon v-else name="warning" class="text-status-error" />
            <span class="text-sm font-medium" :class="rebuildResults.success ? 'text-status-success' : 'text-status-error'">
              {{ rebuildResults.success ? 'Index Rebuilt Successfully' : 'Rebuild Failed' }}
            </span>
          </div>

          <div v-if="rebuildResults.success && rebuildResults.stats" class="grid grid-cols-2 sm:grid-cols-4 lg:grid-cols-7 gap-2 text-sm">
            <div><span class="text-tertiary">Tickets:</span> <span class="text-primary font-medium">{{ rebuildResults.stats.tickets.toLocaleString() }}</span></div>
            <div><span class="text-tertiary">Comments:</span> <span class="text-primary font-medium">{{ rebuildResults.stats.comments.toLocaleString() }}</span></div>
            <div><span class="text-tertiary">Docs:</span> <span class="text-primary font-medium">{{ rebuildResults.stats.documentation.toLocaleString() }}</span></div>
            <div><span class="text-tertiary">Attachments:</span> <span class="text-primary font-medium">{{ rebuildResults.stats.attachments.toLocaleString() }}</span></div>
            <div><span class="text-tertiary">Devices:</span> <span class="text-primary font-medium">{{ rebuildResults.stats.devices.toLocaleString() }}</span></div>
            <div><span class="text-tertiary">Users:</span> <span class="text-primary font-medium">{{ rebuildResults.stats.users.toLocaleString() }}</span></div>
            <div><span class="text-tertiary">Total:</span> <span class="text-accent font-bold">{{ rebuildResults.stats.total.toLocaleString() }}</span></div>
          </div>

          <div v-if="!rebuildResults.success" class="text-sm text-status-error">
            {{ rebuildResults.message }}
          </div>
        </div>
      </div>

    </div>

    <ConfirmModal
      :show="showRebuildConfirm"
      variant="info"
      title="Rebuild the search index?"
      message="This may take a few moments depending on the amount of data."
      confirm-label="Rebuild"
      @confirm="doRebuildIndex"
      @close="showRebuildConfirm = false"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useRouter } from 'vue-router'

import { searchService } from '@/services/searchService'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import Icon from '@/components/common/Icon.vue'
import Spinner from '@/components/common/Spinner.vue'
import type { IndexStats, RebuildResponse } from '@/types/search'

const authStore = useAuthStore()
const router = useRouter()


// Stats state
const stats = ref<IndexStats | null>(null)
const isLoadingStats = ref(false)
const statsError = ref<string | null>(null)

// Rebuild state
const isRebuilding = ref(false)
const rebuildResults = ref<RebuildResponse | null>(null)

// Format bytes to human readable
const formatBytes = (bytes: number): string => {
  if (bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'KB', 'MB', 'GB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i]
}

// Fetch index statistics
const fetchStats = async () => {
  isLoadingStats.value = true
  statsError.value = null

  try {
    stats.value = await searchService.getStats()
  } catch (error) {
    console.error('Error fetching search stats:', error)
    statsError.value = 'Failed to fetch search index statistics'
  } finally {
    isLoadingStats.value = false
  }
}

const showRebuildConfirm = ref(false)

// Rebuild the search index
const rebuildIndex = () => {
  if (isRebuilding.value) return
  showRebuildConfirm.value = true
}

const doRebuildIndex = async () => {
  showRebuildConfirm.value = false
  isRebuilding.value = true
  rebuildResults.value = null

  try {
    rebuildResults.value = await searchService.rebuildIndex()
    // Refresh stats after rebuild
    await fetchStats()
  } catch (error) {
    console.error('Error rebuilding search index:', error)
    rebuildResults.value = {
      success: false,
      message: 'An unexpected error occurred while rebuilding the index',
      stats: { tickets: 0, comments: 0, documentation: 0, attachments: 0, devices: 0, users: 0, total: 0 }
    }
  } finally {
    isRebuilding.value = false
  }
}

// Check if user is admin and fetch initial data
onMounted(async () => {
  if (!authStore.user || authStore.user.role !== 'admin') {
    router.push('/admin')
    return
  }

  await fetchStats()
})
</script>
