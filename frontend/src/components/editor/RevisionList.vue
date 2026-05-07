<script setup lang="ts">
/**
 * Presentational revision list. Renders the loading / error /
 * empty / list states for an article's revision history, plus
 * the restore-confirmation modal — but no card or panel chrome.
 *
 * Container components decide how the list is framed:
 *   - `RevisionHistory.vue` wraps it in `<ResponsivePanel>` for
 *     the documentation side-sheet experience.
 *   - `CollaborativeTicketArticle.vue` renders it inline as a
 *     sibling column inside the Ticket Notes card body so the
 *     editor and the revision list share one card frame.
 *
 * Splitting like this keeps the data fetching, selection, and
 * restore flow in one place while letting each context choose
 * its own layout.
 */
import { formatDate, parseDate } from '@/utils/dateUtils';
import { ref, onMounted, watch, computed } from 'vue'
import { useVersionHistory } from '@/composables/useVersionHistory'
import type { ArticleRevision } from '@/services/versionHistoryService'
import UserAvatar from '@/components/UserAvatar.vue'
import Spinner from '@/components/common/Spinner.vue'
import Icon from '@/components/common/Icon.vue'
import { useDataStore } from '@/stores/dataStore'
import apiClient from '@/services/apiConfig'

interface DocumentationRevisionResponse {
  id: number;
  revision_number: number;
  created_by?: string | null;
  created_at: string;
  word_count?: number | null;
}

interface Props {
  ticketId?: number
  documentId?: number
  type?: 'ticket' | 'documentation'
}

const props = withDefaults(defineProps<Props>(), {
  type: 'ticket'
})

const emit = defineEmits<{
  (e: 'selectRevision', revisionNumber: number | null): void
  (e: 'restored', revisionNumber: number): void
}>()

const dataStore = useDataStore()

const effectiveId = computed(() => {
  if (props.type === 'documentation') {
    return props.documentId
  }
  return props.ticketId
})

const ticketHistory = props.type === 'ticket'
  ? useVersionHistory(computed(() => props.ticketId || 0))
  : null

const docRevisions = ref<ArticleRevision[]>([])
const docLoading = ref(false)
const docError = ref<string | null>(null)
const docRestoring = ref(false)

const revisions = computed(() => {
  if (props.type === 'documentation') {
    return docRevisions.value
  }
  return ticketHistory?.revisions.value || []
})

const loading = computed(() => {
  if (props.type === 'documentation') {
    return docLoading.value
  }
  return ticketHistory?.isLoading.value || false
})

const isRestoring = computed(() => {
  if (props.type === 'documentation') {
    return docRestoring.value
  }
  return ticketHistory?.isRestoring.value || false
})

async function loadDocumentationRevisions() {
  if (!props.documentId) return

  docLoading.value = true
  docError.value = null

  try {
    const response = await apiClient.get<DocumentationRevisionResponse[]>(`/collaboration/docs/${props.documentId}/revisions`)
    docRevisions.value = response.data.map((rev) => ({
      id: rev.id,
      revision_number: rev.revision_number,
      created_at: rev.created_at,
      article_content_id: 0,
      contributed_by: (rev.created_by ? [rev.created_by] : []) as (string | null)[],
      word_count: rev.word_count ?? null,
    }))
  } catch (err) {
    const error = err as Error;
    docError.value = error.message || 'Failed to load revisions'
    console.error('Failed to load documentation revisions:', err)
  } finally {
    docLoading.value = false
  }
}

async function restoreDocumentationRevision(revisionNumber: number): Promise<boolean> {
  if (!props.documentId) return false

  docRestoring.value = true

  try {
    await apiClient.post(`/collaboration/docs/${props.documentId}/revisions/${revisionNumber}/restore`)
    await loadDocumentationRevisions()
    return true
  } catch (err) {
    const error = err as Error;
    docError.value = error.message || 'Failed to restore revision'
    console.error('Failed to restore documentation revision:', err)
    return false
  } finally {
    docRestoring.value = false
  }
}

function loadRevisions() {
  if (props.type === 'documentation') {
    loadDocumentationRevisions()
  } else {
    ticketHistory?.loadRevisions()
  }
}

async function restoreToRevision(revisionNumber: number): Promise<boolean> {
  if (props.type === 'documentation') {
    return restoreDocumentationRevision(revisionNumber)
  }
  return ticketHistory?.restoreToRevision(revisionNumber) || false
}

watch(revisions, async (newRevisions) => {
  if (!newRevisions || newRevisions.length === 0) return

  const userUuids = new Set<string>()
  newRevisions.forEach(revision => {
    if (revision.contributed_by && Array.isArray(revision.contributed_by)) {
      revision.contributed_by.forEach(uuid => {
        if (uuid) userUuids.add(uuid)
      })
    }
  })

  if (userUuids.size > 0) {
    await Promise.all(
      Array.from(userUuids).map(uuid => dataStore.getUserByUuid(uuid))
    )
  }
}, { immediate: true })

const getUserName = (uuid: string): string | undefined => {
  const user = dataStore.getCachedUserByUuid(uuid)
  return user?.name
}

const selectedRevision = ref<ArticleRevision | null>(null)
const showRestoreConfirm = ref(false)
const revisionToRestore = ref<number | null>(null)

const error = computed(() => {
  if (props.type === 'documentation') {
    return docError.value
  }
  if (ticketHistory?.error.value) return ticketHistory.error.value.message
  if (ticketHistory?.restoreError.value) return ticketHistory.restoreError.value.message
  return null
})

async function selectRevision(revision: ArticleRevision) {
  selectedRevision.value = revision
  emit('selectRevision', revision.revision_number)
}

function confirmRestore(revision: ArticleRevision) {
  revisionToRestore.value = revision.revision_number
  showRestoreConfirm.value = true
}

function cancelRestore() {
  showRestoreConfirm.value = false
  revisionToRestore.value = null
}

async function executeRestore() {
  if (revisionToRestore.value === null) return

  const success = await restoreToRevision(revisionToRestore.value)

  if (success) {
    showRestoreConfirm.value = false
    selectedRevision.value = null
    emit('restored', revisionToRestore.value)
    revisionToRestore.value = null
  }
}

function formatRelativeDate(dateString: string): string {
  const date = parseDate(dateString)
  if (!date) return ''

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

watch(
  () => effectiveId.value,
  () => {
    loadRevisions()
    selectedRevision.value = null
  }
)

onMounted(() => {
  loadRevisions()
})
</script>

<template>
  <div class="flex flex-col flex-1 min-h-0">
    <div v-if="loading" class="flex items-center justify-center py-8 text-primary">
      <Spinner size="lg" />
    </div>

    <div v-else-if="error" class="error-banner">
      {{ error }}
    </div>

    <div v-else-if="revisions.length === 0" class="flex flex-col items-center px-4 py-8 text-secondary">
      <Icon name="clock" size="lg" class="mb-3 text-tertiary" />
      <p class="text-sm text-center">No revisions yet</p>
      <p class="text-xs text-tertiary text-center mt-1">Revisions are created when you make changes</p>
    </div>

    <div v-else class="flex-1 overflow-y-auto">
      <div class="px-4 py-3 bg-surface-alt border-b border-default">
        <div class="flex items-center gap-2 text-sm">
          <div class="current-version-indicator"></div>
          <span class="font-medium text-primary">Current Version</span>
        </div>
      </div>

      <div
        v-for="revision in revisions"
        :key="revision.id"
        @click="selectRevision(revision)"
        :class="[
          'revision-item',
          { 'revision-item-selected': selectedRevision?.id === revision.id },
        ]"
      >
        <div class="flex items-center justify-between mb-2">
          <div class="flex items-center gap-2">
            <span class="text-xs font-mono text-tertiary">v{{ revision.revision_number }}</span>
            <span class="text-xs text-tertiary">•</span>
            <span class="text-xs text-secondary">{{ formatRelativeDate(revision.created_at) }}</span>
          </div>
          <span
            v-if="selectedRevision?.id === revision.id"
            class="text-primary flex-shrink-0 inline-flex"
          >
            <Icon name="check" />
          </span>
        </div>

        <div v-if="revision.contributed_by && revision.contributed_by.length > 0" class="flex items-center gap-1 mb-1">
          <div v-if="revision.contributed_by.length === 1" class="flex items-center gap-1">
            <span class="text-xs text-tertiary">By:</span>
            <UserAvatar
              :name="revision.contributed_by[0] || 'Unknown'"
              :user-name="getUserName(revision.contributed_by[0] || '')"
              :show-name="false"
              size="xs"
              :clickable="true"
            />
            <span class="text-xs text-secondary">{{ getUserName(revision.contributed_by[0] || '') || 'Unknown' }}</span>
          </div>
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

        <div v-if="revision.word_count" class="text-xs text-tertiary">
          {{ revision.word_count }} words
        </div>

        <button
          v-if="selectedRevision?.id === revision.id"
          @click.stop="confirmRestore(revision)"
          :disabled="isRestoring"
          class="mt-2 w-full px-3 py-1.5 text-xs font-medium text-white bg-accent hover:bg-accent-hover disabled:opacity-50 disabled:cursor-not-allowed rounded transition-colors"
        >
          {{ isRestoring ? 'Restoring...' : 'Restore This Version' }}
        </button>
      </div>
    </div>

    <Teleport to="body">
      <div
        v-if="showRestoreConfirm"
        class="fixed inset-0 bg-black/50 flex items-center justify-center z-overlay"
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
              class="flex-1 px-4 py-2 text-sm font-medium text-white bg-accent hover:bg-accent-hover rounded-lg transition-colors disabled:opacity-50"
            >
              {{ isRestoring ? 'Restoring...' : 'Restore' }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
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

.error-banner {
  padding: 0.75rem 1rem;
  font-size: 0.875rem;
  color: var(--color-status-error);
  background-color: color-mix(in srgb, var(--color-status-error) 10%, transparent);
  border-radius: 0.5rem;
  margin: 1rem;
}

.current-version-indicator {
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 9999px;
  background-color: var(--color-status-success);
}
</style>
