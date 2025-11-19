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
      <p class="text-xs text-tertiary text-center mt-1">Revisions are created automatically as you edit</p>
    </div>

    <!-- Revisions List -->
    <div v-else class="revision-list">
      <!-- Current Version Badge -->
      <div
        @click="selectRevision(null)"
        :class="[
          'px-4 py-3 border-b border-default cursor-pointer hover:bg-surface-alt/50 transition-colors',
          { 'bg-surface-alt': selectedRevision === null }
        ]"
      >
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2 text-sm">
            <div class="current-version-indicator"></div>
            <span class="font-medium text-primary">Current Version</span>
          </div>
          <svg
            v-if="selectedRevision === null"
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

        <!-- Word Count -->
        <div v-if="revision.word_count" class="flex items-center gap-1 mb-1">
          <svg class="w-3 h-3 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
          </svg>
          <span class="text-xs text-tertiary">{{ revision.word_count }} words</span>
        </div>

        <!-- Restore Button (only show for non-current revisions) -->
        <button
          v-if="selectedRevision?.id === revision.id"
          @click.stop="restoreRevision(revision.revision_number)"
          class="mt-2 w-full px-2 py-1 text-xs bg-brand-blue text-white rounded hover:bg-brand-blue/80 transition-colors flex items-center justify-center gap-1"
          title="Restore this version"
        >
          <svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
          </svg>
          Restore this version
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { formatDate, formatDateTime } from '@/utils/dateUtils';
import { ref, onMounted, computed } from 'vue'
import apiClient from '@/services/apiConfig'

interface DocumentationRevision {
  id: number
  page_id: number
  revision_number: number
  title: string
  yjs_document_content: string
  created_by: string
  created_at: string
  word_count: number | null
  change_summary: string | null
}

interface Props {
  documentId: number
}

const props = defineProps<Props>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'selectRevision', revisionNumber: number | null): void
  (e: 'restored', revisionNumber: number): void
}>()

const loading = ref(true)
const error = ref<string | null>(null)
const revisions = ref<DocumentationRevision[]>([])
const selectedRevision = ref<DocumentationRevision | null>(null)

// Fetch revisions on mount
onMounted(async () => {
  try {
    loading.value = true
    error.value = null

    const response = await apiClient.get(`/collaboration/docs/${props.documentId}/revisions`)
    revisions.value = response.data
  } catch (err) {
    console.error('Failed to fetch documentation revisions:', err)
    error.value = 'Failed to load revision history'
  } finally {
    loading.value = false
  }
})

// Format date for display
const formatRelativeDate = (dateString: string) => {
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

// Select a revision to view
const selectRevision = (revision: DocumentationRevision | null) => {
  selectedRevision.value = revision
  emit('selectRevision', revision?.revision_number ?? null)
}

// Restore a specific revision
const restoreRevision = async (revisionNumber: number) => {
  if (!confirm('Are you sure you want to restore this version? This will replace the current content.')) {
    return
  }

  try {
    await apiClient.post(`/collaboration/docs/${props.documentId}/restore/${revisionNumber}`)
    emit('restored', revisionNumber)
    emit('close')
  } catch (err) {
    console.error('Failed to restore revision:', err)
    alert('Failed to restore revision. Please try again.')
  }
}
</script>

<style scoped>
.revision-history-sidebar {
  display: flex;
  flex-direction: column;
  height: 100%;
  background-color: var(--color-surface);
  border-left: 1px solid var(--color-default);
  width: 300px;
  max-width: 100vw;
}

.revision-history-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.75rem 1rem;
  border-bottom: 1px solid var(--color-default);
  background-color: var(--color-surface-alt);
}

.revision-list {
  flex: 1;
  overflow-y: auto;
}

.revision-item {
  padding: 0.75rem 1rem;
  border-bottom: 1px solid var(--color-default);
  cursor: pointer;
  transition: background-color 0.2s;
}

.revision-item:hover {
  background-color: var(--color-surface-alt);
  opacity: 0.5;
}

.revision-item-selected {
  background-color: var(--color-surface-alt);
}

.current-version-indicator {
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 9999px;
  background-color: #10b981;
  animation: pulse 2s cubic-bezier(0.4, 0, 0.6, 1) infinite;
}

@keyframes pulse {
  0%, 100% {
    opacity: 1;
  }
  50% {
    opacity: 0.5;
  }
}

.error-banner {
  margin: 0.75rem 1rem;
  padding: 0.5rem 1rem;
  background-color: rgba(239, 68, 68, 0.1);
  border: 1px solid rgba(239, 68, 68, 0.2);
  border-radius: 0.375rem;
  font-size: 0.875rem;
  color: #ef4444;
}
</style>
