<template>
  <div class="revision-history-sidebar">
    <!-- Header -->
    <div class="revision-history-header">
      <h3 class="text-lg font-semibold text-primary">Revision History</h3>
      <button
        @click="$emit('close')"
        class="text-secondary hover:text-primary transition-colors"
        title="Close revision history"
      >
        <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
        </svg>
      </button>
    </div>

    <!-- Loading State -->
    <div v-if="loading" class="flex items-center justify-center py-8">
      <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-primary"></div>
    </div>

    <!-- Error State -->
    <div v-else-if="error" class="error-banner">
      {{ error }}
    </div>

    <!-- Empty State -->
    <div v-else-if="revisions.length === 0" class="flex flex-col items-center px-4 py-8 text-secondary">
      <svg class="w-12 h-12 mb-3 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
      </svg>
      <p class="text-sm text-center">No revisions yet</p>
      <p class="text-xs text-tertiary text-center mt-1">Revisions are created when you make changes</p>
    </div>

    <!-- Revisions List -->
    <div v-else class="revision-list">
      <!-- Current Version Badge -->
      <div class="px-4 py-3 bg-surface-alt border-b border-default">
        <div class="flex items-center gap-2 text-sm">
          <div class="current-version-indicator"></div>
          <span class="font-medium text-primary">Current Version</span>
        </div>
      </div>

      <!-- Revision Items -->
      <div
        v-for="revision in revisions"
        :key="revision.id"
        @click="selectRevision(revision)"
        :class="[
          'revision-item',
          {
            'revision-item-selected': selectedRevision?.id === revision.id,
          },
        ]"
      >
        <!-- Revision Number & Date -->
        <div class="flex items-center justify-between mb-2">
          <div class="flex items-center gap-2">
            <span class="text-xs font-mono text-tertiary">v{{ revision.revision_number }}</span>
            <span class="text-xs text-tertiary">•</span>
            <span class="text-xs text-secondary">{{ formatRelativeDate(revision.created_at) }}</span>
          </div>
          <svg
            v-if="selectedRevision?.id === revision.id"
            class="w-4 h-4 text-primary flex-shrink-0"
            fill="currentColor"
            viewBox="0 0 20 20"
          >
            <path
              fill-rule="evenodd"
              d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z"
              clip-rule="evenodd"
            />
          </svg>
        </div>

        <!-- Contributors -->
        <div v-if="revision.contributed_by && revision.contributed_by.length > 0" class="flex items-center gap-1 mb-1">
          <!-- Single contributor: show name -->
          <div v-if="revision.contributed_by.length === 1" class="flex items-center gap-1">
            <span class="text-xs text-tertiary">By:</span>
            <UserAvatar
              :name="revision.contributed_by[0] || 'Unknown'"
              :user-name="getUserName(revision.contributed_by[0] || '')"
              :show-name="true"
              size="xs"
              :clickable="true"
            />
          </div>
          <!-- Multiple contributors: show avatars only -->
          <div v-else class="flex items-center gap-1">
            <span class="text-xs text-tertiary">By:</span>
            <div class="flex items-center gap-1">
              <UserAvatar
                v-for="(userId, index) in revision.contributed_by.slice(0, 3)"
                :key="userId || index"
                :name="userId || 'Unknown'"
                :user-name="getUserName(userId || '')"
                :show-name="false"
                size="xs"
                :clickable="true"
              />
              <span v-if="revision.contributed_by.length > 3" class="text-xs text-tertiary">
                +{{ revision.contributed_by.length - 3 }}
              </span>
            </div>
          </div>
        </div>

        <!-- Word Count -->
        <div v-if="revision.word_count" class="text-xs text-tertiary">
          {{ revision.word_count }} words
        </div>

        <!-- Restore Button -->
        <button
          v-if="selectedRevision?.id === revision.id"
          @click.stop="confirmRestore(revision)"
          :disabled="isRestoring"
          class="mt-2 w-full px-3 py-1.5 text-xs font-medium text-white bg-blue-600 hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed rounded transition-colors"
        >
          {{ isRestoring ? 'Restoring...' : 'Restore This Version' }}
        </button>
      </div>
    </div>

    <!-- Action Buttons Footer (when viewing a revision) -->
    <div v-if="selectedRevision" class="revision-history-footer">
      <button
        @click="exitRevisionView"
        class="w-full px-4 py-2 text-sm font-medium text-primary bg-surface-alt hover:bg-surface-hover border border-default rounded-lg transition-colors"
      >
        Exit Revision View
      </button>
    </div>

    <!-- Restore Confirmation Modal -->
    <div
      v-if="showRestoreConfirm"
      class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
      @click.self="cancelRestore"
    >
      <div class="bg-surface rounded-lg shadow-xl max-w-md w-full mx-4 p-6">
        <h3 class="text-lg font-semibold text-primary mb-2">Restore Revision?</h3>
        <p class="text-sm text-secondary mb-4">
          This will restore the ticket to revision {{ revisionToRestore }}. This action will replace the current content with the selected revision.
        </p>
        <p class="text-xs text-tertiary mb-6">
          Note: A new revision will be created so you can always undo this change.
        </p>
        <div class="flex gap-3">
          <button
            @click="cancelRestore"
            :disabled="isRestoring"
            class="flex-1 px-4 py-2 text-sm font-medium text-primary bg-surface-alt hover:bg-surface-hover border border-default rounded-lg transition-colors disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            @click="executeRestore"
            :disabled="isRestoring"
            class="flex-1 px-4 py-2 text-sm font-medium text-white bg-blue-600 hover:bg-blue-700 rounded-lg transition-colors disabled:opacity-50"
          >
            {{ isRestoring ? 'Restoring...' : 'Restore' }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { formatDate, formatDateTime } from '@/utils/dateUtils';
import { ref, onMounted, watch, computed } from 'vue'
import { useVersionHistory } from '@/composables/useVersionHistory'
import type { ArticleRevision } from '@/services/versionHistoryService'
import UserAvatar from '@/components/UserAvatar.vue'
import { useDataStore } from '@/stores/dataStore'

interface Props {
  ticketId: number
}

const props = defineProps<Props>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'selectRevision', revisionNumber: number | null): void
  (e: 'restored', revisionNumber: number): void
}>()

const dataStore = useDataStore()

// Use the composable
const {
  revisions,
  isLoading: loading,
  error: apiError,
  isRestoring,
  restoreError,
  loadRevisions,
  restoreToRevision,
} = useVersionHistory(computed(() => props.ticketId))

// Pre-fetch user data when revisions load
watch(revisions, async (newRevisions) => {
  if (!newRevisions || newRevisions.length === 0) return

  // Collect all unique user UUIDs
  const userUuids = new Set<string>()
  newRevisions.forEach(revision => {
    revision.contributed_by.forEach(uuid => {
      if (uuid) userUuids.add(uuid)
    })
  })

  // Pre-fetch user data for all contributors
  await Promise.all(
    Array.from(userUuids).map(uuid => dataStore.getUserByUuid(uuid))
  )
}, { immediate: true })

// Helper to get user name from UUID (from cache)
const getUserName = (uuid: string): string | undefined => {
  const user = dataStore.getCachedUserByUuid(uuid)
  return user?.name
}

// State
const selectedRevision = ref<ArticleRevision | null>(null)
const showRestoreConfirm = ref(false)
const revisionToRestore = ref<number | null>(null)

// Computed error message
const error = computed(() => {
  if (apiError.value) return apiError.value.message
  if (restoreError.value) return restoreError.value.message
  return null
})

// Select a revision to preview
async function selectRevision(revision: ArticleRevision) {
  selectedRevision.value = revision
  emit('selectRevision', revision.revision_number)
}

// Exit revision view and return to current version
function exitRevisionView() {
  selectedRevision.value = null
  emit('selectRevision', null)
}

// Show restore confirmation dialog
function confirmRestore(revision: ArticleRevision) {
  revisionToRestore.value = revision.revision_number
  showRestoreConfirm.value = true
}

// Cancel restore
function cancelRestore() {
  showRestoreConfirm.value = false
  revisionToRestore.value = null
}

// Actually restore the revision
async function executeRestore() {
  if (revisionToRestore.value === null) return

  const success = await restoreToRevision(revisionToRestore.value)

  if (success) {
    showRestoreConfirm.value = false
    selectedRevision.value = null
    // Don't emit selectRevision(null) here - we're not in revision view mode
    // The restore itself will update the editor content via WebSocket
    emit('restored', revisionToRestore.value)
    revisionToRestore.value = null
  }
}

// Format date for display
function formatRelativeDate(dateString: string): string {
  const date = new Date(dateString)
  const now = new Date()
  const diffMs = now.getTime() - date.getTime()
  const diffMins = Math.floor(diffMs / 60000)
  const diffHours = Math.floor(diffMs / 3600000)
  const diffDays = Math.floor(diffMs / 86400000)

  if (diffMins < 1) return 'Just now'
  if (diffMins < 60) return `${diffMins}m ago`
  if (diffHours < 24) return `${diffHours}h ago`
  if (diffDays < 7) return `${diffDays}d ago`

  return formatDate(dateString, "MMM d, yyyy")
}

// Watch for ticketId changes
watch(
  () => props.ticketId,
  () => {
    loadRevisions()
    selectedRevision.value = null
  }
)

// Initial fetch
onMounted(() => {
  loadRevisions()
})
</script>

<style scoped>
.revision-history-sidebar {
  display: flex;
  flex-direction: column;
  height: 100%;
  width: 320px;
  background-color: var(--color-surface);
  border-left: 1px solid var(--color-default);
}

.revision-history-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.75rem 1rem;
  border-bottom: 1px solid var(--color-default);
}

.revision-list {
  flex: 1;
  overflow-y: auto;
}

.revision-item {
  padding: 0.75rem 1rem;
  border-bottom: 1px solid var(--color-default);
  cursor: pointer;
  transition: background-color 0.2s, color 0.2s;
}

.revision-item:hover {
  background-color: var(--color-surface-hover);
}

.revision-item-selected {
  background-color: var(--color-surface-alt);
  border-left: 4px solid var(--color-primary);
}

.revision-history-footer {
  padding: 1rem;
  border-top: 1px solid var(--color-default);
  background-color: var(--color-surface);
}

/* Error banner styling using theme variables */
.error-banner {
  padding: 0.75rem 1rem;
  font-size: 0.875rem;
  color: var(--color-status-error);
  background-color: color-mix(in srgb, var(--color-status-error) 10%, transparent);
  border-radius: 0.5rem;
  margin: 1rem;
}

/* Current version indicator dot */
.current-version-indicator {
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 9999px;
  background-color: var(--color-status-success);
}
</style>
