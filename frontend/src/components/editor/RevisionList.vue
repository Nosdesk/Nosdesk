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
import { formatDate, parseDate } from '@nosdesk/core/utils/dateUtils';
import { ref, onMounted, watch, computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { useVersionHistory } from '@/composables/useVersionHistory'
import type { ArticleRevision } from '@/services/versionHistoryService'
import UserAvatar from '@/components/UserAvatar.vue'
import Spinner from '@/components/common/Spinner.vue'
import Icon from '@/components/common/Icon.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import { useUsersDirectory } from '@/composables/useUsersDirectory'
import apiClient from '@nosdesk/core/apiClient'

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

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const { getUserHandle } = useUsersDirectory()

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
    docError.value = error.message || t('editor-revisions-load-error')
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
    docError.value = error.message || t('editor-revisions-restore-error')
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

// Each `getUserHandle(uuid)` call lazily fires a fetch on first
// access; the directory's batch scheduler coalesces concurrent
// uuids from one render pass into a single /users/batch call,
// so we don't need an explicit prefetch step here.
const getUserName = (uuid: string): string | undefined => {
  return getUserHandle(uuid).user.value?.name
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

  if (diffMins < 1) return t('editor-revisions-just-now')
  if (diffMins < 60) return t('editor-revisions-minutes-ago', { minutes: diffMins })
  if (diffHours < 24) return t('editor-revisions-hours-ago', { hours: diffHours })
  if (diffDays < 7) return t('editor-revisions-days-ago', { days: diffDays })

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
      <p class="text-sm text-center">{{ t('editor-revisions-empty-title') }}</p>
      <p class="text-xs text-tertiary text-center mt-1">{{ t('editor-revisions-empty-hint') }}</p>
    </div>

    <div v-else class="flex-1 overflow-y-auto">
      <div class="px-4 py-3 bg-surface-alt border-b border-default">
        <div class="flex items-center gap-2 text-sm">
          <div class="current-version-indicator"></div>
          <span class="font-medium text-primary">{{ t('editor-revisions-current-version') }}</span>
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
            <span class="text-xs text-tertiary">{{ t('editor-revisions-by') }}</span>
            <UserAvatar
              :uuid="revision.contributed_by[0] || null"
              :fallbackName="getUserName(revision.contributed_by[0] || '') || t('editor-revisions-unknown-user')"
              :show-name="false"
              size="xs"
              :clickable="true"
            />
            <span class="text-xs text-secondary">{{ getUserName(revision.contributed_by[0] || '') || t('editor-revisions-unknown-user') }}</span>
          </div>
          <div v-else class="flex items-center gap-1">
            <span class="text-xs text-tertiary">{{ t('editor-revisions-by') }}</span>
            <div class="flex items-center gap-1">
              <UserAvatar
                v-for="(userId, index) in revision.contributed_by.slice(0, 3)"
                :key="userId || index"
                :uuid="userId || null"
                :fallbackName="getUserName(userId || '') || t('editor-revisions-unknown-user')"
                :show-name="false"
                size="xs"
                :clickable="true"
              />
              <span v-if="revision.contributed_by.length > 3" class="text-xs text-tertiary">
                {{ t('editor-revisions-more-contributors', { count: revision.contributed_by.length - 3 }) }}
              </span>
            </div>
          </div>
        </div>

        <div v-if="revision.word_count" class="text-xs text-tertiary">
          {{ t('editor-revisions-word-count', { count: revision.word_count }) }}
        </div>

        <button
          v-if="selectedRevision?.id === revision.id"
          @click.stop="confirmRestore(revision)"
          :disabled="isRestoring"
          class="mt-2 w-full px-3 py-1.5 text-xs font-medium text-on-accent bg-accent hover:bg-accent-hover disabled:opacity-50 disabled:cursor-not-allowed rounded transition-colors"
        >
          {{ isRestoring ? t('editor-revisions-restoring') : t('editor-revisions-restore-button') }}
        </button>
      </div>
    </div>

    <ConfirmModal
      :show="showRestoreConfirm"
      :title="t('editor-revisions-confirm-title')"
      :message="t('editor-revisions-confirm-body', { revision: revisionToRestore ?? '' })"
      :confirm-label="t('editor-revisions-confirm-restore')"
      :cancel-label="t('editor-revisions-confirm-cancel')"
      :loading="isRestoring"
      @confirm="executeRestore"
      @close="cancelRestore"
    >
      <p class="text-xs text-tertiary">{{ t('editor-revisions-confirm-note') }}</p>
    </ConfirmModal>
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
