<template>
  <div class="flex-1">
    <!-- Navigation and actions bar -->
    <div class="pt-4 px-6 flex justify-between items-center">
      <BackButton fallbackRoute="/admin/settings" label="Back to Administration" />
    </div>

    <div class="flex flex-col gap-6 px-6 py-4 mx-auto w-full max-w-8xl">
      <div>
        <h1 class="text-2xl font-bold text-primary">Search Index Management</h1>
        <p class="text-secondary mt-1">Manage the full-text search index for tickets, documentation, devices, and users.</p>
      </div>

      <!-- Index Statistics -->
      <div class="bg-surface border border-default rounded-xl">
        <div class="p-4 flex flex-col gap-3">
          <!-- Header row with icon -->
          <div class="flex items-center gap-3">
            <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-accent/15 flex items-center justify-center text-accent">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M9 19v-6a2 2 0 00-2-2H5a2 2 0 00-2 2v6a2 2 0 002 2h2a2 2 0 002-2zm0 0V9a2 2 0 012-2h2a2 2 0 012 2v10m-6 0a2 2 0 002 2h2a2 2 0 002-2m0 0V5a2 2 0 012-2h2a2 2 0 012 2v14a2 2 0 01-2 2h-2a2 2 0 01-2-2z" />
              </svg>
            </div>
            <div class="flex-1">
              <span class="font-medium text-primary">Index Statistics</span>
            </div>
            <button
              @click="fetchStats"
              :disabled="isLoadingStats"
              class="px-3 py-1.5 bg-surface-alt text-secondary border border-default rounded-lg text-sm hover:bg-surface-hover font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-1.5"
            >
              <svg v-if="isLoadingStats" class="animate-spin h-3.5 w-3.5" fill="none" viewBox="0 0 24 24">
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
              <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
              </svg>
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
                  <svg class="animate-spin h-5 w-5" fill="none" viewBox="0 0 24 24">
                    <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                    <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                  </svg>
                  <span class="text-lg font-bold">Rebuilding</span>
                </div>
                <div v-else class="flex items-center gap-1.5 text-status-success">
                  <svg class="h-5 w-5" fill="currentColor" viewBox="0 0 20 20">
                    <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z" clip-rule="evenodd" />
                  </svg>
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
            <svg class="animate-spin h-6 w-6 text-accent" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
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
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
              </svg>
            </div>
            <div class="flex-1">
              <span class="font-medium text-primary">Rebuild Search Index</span>
            </div>
            <button
              @click="rebuildIndex"
              :disabled="isRebuilding"
              class="px-3 py-1.5 bg-status-warning/20 text-status-warning border border-status-warning/50 rounded-lg text-sm hover:bg-status-warning/30 font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-1.5 whitespace-nowrap"
            >
              <svg v-if="isRebuilding" class="animate-spin h-3.5 w-3.5" fill="none" viewBox="0 0 24 24">
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
              <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
              </svg>
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
            <svg v-if="rebuildResults.success" class="w-4 h-4 text-status-success" fill="currentColor" viewBox="0 0 20 20">
              <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z" clip-rule="evenodd" />
            </svg>
            <svg v-else class="w-4 h-4 text-status-error" fill="currentColor" viewBox="0 0 20 20">
              <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.707 7.293a1 1 0 00-1.414 1.414L8.586 10l-1.293 1.293a1 1 0 101.414 1.414L10 11.414l1.293 1.293a1 1 0 001.414-1.414L11.414 10l1.293-1.293a1 1 0 00-1.414-1.414L10 8.586 8.707 7.293z" clip-rule="evenodd" />
            </svg>
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

      <!-- Search Tips Section -->
      <div class="bg-surface border border-default rounded-xl">
        <div class="p-4 flex flex-col gap-3">
          <div class="flex items-center gap-3">
            <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-blue-500/15 flex items-center justify-center text-blue-500">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
            </div>
            <div class="flex-1">
              <span class="font-medium text-primary">About the Search Index</span>
            </div>
          </div>

          <div class="text-secondary text-sm space-y-2">
            <p>
              The search index uses <strong class="text-primary">Tantivy</strong>, a high-performance full-text search engine.
              It provides fast, relevant search results across all your data.
            </p>
            <ul class="list-disc list-inside space-y-1 text-tertiary">
              <li>Search is updated automatically when content is created or modified</li>
              <li>Use <kbd class="px-1.5 py-0.5 text-xs bg-surface-alt rounded border border-default">{{ isMac ? '\u2318K' : 'Ctrl+K' }}</kbd> to open global search from anywhere</li>
              <li>Results are ranked by relevance with title matches weighted higher</li>
              <li>Rebuild the index if you notice missing or stale search results</li>
            </ul>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useRouter } from 'vue-router'
import BackButton from '@/components/common/BackButton.vue'
import { searchService } from '@/services/searchService'
import type { IndexStats, RebuildResponse } from '@/types/search'

const authStore = useAuthStore()
const router = useRouter()

// Platform detection for keyboard shortcut display
const isMac = computed(() => navigator.platform.toUpperCase().indexOf('MAC') >= 0)

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

// Rebuild the search index
const rebuildIndex = async () => {
  if (isRebuilding.value) return

  if (!confirm('Are you sure you want to rebuild the search index? This may take a few moments depending on the amount of data.')) {
    return
  }

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
    router.push('/admin/settings')
    return
  }

  await fetchStats()
})
</script>
