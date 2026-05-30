<script setup lang="ts">
/**
 * Admin CRUD page for the canned-response library. Workspace-wide
 * shared list; any tech can read in the composer picker, only
 * admins reach this page.
 *
 * Create / edit lives on a separate full-page route so the editor
 * has room for chip-aware variable tokens on the left and a live
 * preview pane on the right (see CannedResponseEditView).
 */
import { ref, computed } from 'vue';
import { useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import { formatDistanceToNow } from 'date-fns';

import AlertMessage from '@/components/common/AlertMessage.vue';
import Button from '@/components/common/Button.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Icon from '@/components/common/Icon.vue';
import SearchInput from '@/components/common/SearchInput.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import StarterCatalogModal from '@/components/cannedResponseComponents/StarterCatalogModal.vue';
import cannedResponsesService, {
  type CannedResponseListItem,
  type CannedResponseStarter,
} from '@/services/cannedResponsesService';
import { extractErrorMessage } from '@/utils/errors';
import { highlightTerms } from '@/utils/highlight';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const router = useRouter();

// Pinia Colada keys the canned-response list. The picker will read
// this same key in a follow-up commit so admin saves invalidate it
// for every open composer in the session, no manual refetch needed.
const CANNED_RESPONSES_KEY = ['canned-responses'] as const;
const queryCache = useQueryCache();
const responsesQuery = useQuery({
  key: CANNED_RESPONSES_KEY,
  query: () => cannedResponsesService.list(),
});

const responses = computed<CannedResponseListItem[]>(() =>
  Array.isArray(responsesQuery.data.value) ? responsesQuery.data.value : [],
);
const isFirstLoad = computed(
  () => responsesQuery.status.value === 'pending' && responsesQuery.data.value === undefined,
);
const loadError = computed(() =>
  responsesQuery.error.value ? t('admin-canned-responses-error-load') : '',
);

// Search input (matches title + body, case-insensitive, multi-term
// AND so "password reset" narrows in one pass). Trimmed terms drive
// the filter and the per-row hit highlighter.
const search = ref('');
const searchTerms = computed<string[]>(() =>
  search.value
    .toLowerCase()
    .split(/\s+/)
    .map((s) => s.trim())
    .filter(Boolean),
);

// Column sort. Name ascending is the default (matches every
// competitor surveyed); click a header to switch axis or direction.
type SortKey = 'title' | 'updated' | 'inserts';
const sortKey = ref<SortKey>('title');
const sortDir = ref<'asc' | 'desc'>('asc');
const sorted = computed<CannedResponseListItem[]>(() => {
  const arr = [...responses.value];
  arr.sort((a, b) => {
    let cmp = 0;
    if (sortKey.value === 'title') cmp = a.title.localeCompare(b.title);
    else if (sortKey.value === 'updated') cmp = a.updated_at.localeCompare(b.updated_at);
    else cmp = a.inserts_30d - b.inserts_30d;
    return sortDir.value === 'asc' ? cmp : -cmp;
  });
  return arr;
});
const filtered = computed<CannedResponseListItem[]>(() => {
  if (searchTerms.value.length === 0) return sorted.value;
  return sorted.value.filter((r) => {
    const haystack = `${r.title}\n${r.body}`.toLowerCase();
    return searchTerms.value.every((term) => haystack.includes(term));
  });
});
function toggleSort(key: SortKey): void {
  if (sortKey.value === key) {
    sortDir.value = sortDir.value === 'asc' ? 'desc' : 'asc';
  } else {
    sortKey.value = key;
    // Name default ascending; usage / updated default descending,
    // since "most used / most recent first" is the obviously useful
    // axis the moment an admin clicks those columns.
    sortDir.value = key === 'title' ? 'asc' : 'desc';
  }
}

// Hit highlighter for the title column. Body matches drive the
// filter but the body is shown collapsed on the row, so it doesn't
// get the highlight pass here.
function highlight(value: string): string {
  return highlightTerms(value, searchTerms.value);
}

// Mutation feedback (delete) lives in local refs. Create / update
// feedback is owned by the editor view and surfaces on its next
// mount via Pinia Colada invalidation.
const successMessage = ref('');
const errorMessage = ref('');

// Browse-starters modal. Selecting a starter navigates to the
// editor route with `?starter=<slug>` so the editor can pre-fill
// without us having to round-trip the catalog twice.
const showStarters = ref(false);
function openStarters(): void {
  showStarters.value = true;
}
function pickStarter(starter: CannedResponseStarter): void {
  showStarters.value = false;
  router.push({
    name: 'admin-canned-responses-new',
    query: { starter: starter.slug },
  });
}

// Navigation helpers. Create + edit are full-page routes (full
// editor with preview pane), not modals, so click handlers push
// the router instead of opening overlays.
function openCreate(): void {
  router.push({ name: 'admin-canned-responses-new' });
}
function openEdit(row: CannedResponseListItem): void {
  router.push({ name: 'admin-canned-responses-edit', params: { id: row.id } });
}

// Delete confirmation
const showDeleteConfirm = ref(false);
const deleting = ref<CannedResponseListItem | null>(null);
const isDeleting = ref(false);
function confirmDelete(row: CannedResponseListItem): void {
  deleting.value = row;
  showDeleteConfirm.value = true;
}
async function doDelete(): Promise<void> {
  if (!deleting.value) return;
  isDeleting.value = true;
  try {
    await cannedResponsesService.remove(deleting.value.id);
    successMessage.value = t('admin-canned-responses-success-deleted');
    showDeleteConfirm.value = false;
    deleting.value = null;
    await queryCache.invalidateQueries({ key: CANNED_RESPONSES_KEY });
    setTimeout(() => (successMessage.value = ''), 3000);
  } catch (error) {
    errorMessage.value = extractErrorMessage(
      error,
      t('admin-canned-responses-error-delete'),
    );
    setTimeout(() => (errorMessage.value = ''), 5000);
  } finally {
    isDeleting.value = false;
  }
}

function relativeTime(iso: string): string {
  try {
    return formatDistanceToNow(new Date(iso), { addSuffix: true });
  } catch {
    return iso;
  }
}
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <!-- Heading + create CTAs -->
      <div class="mb-2 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
        <div>
          <h1 class="text-xl sm:text-2xl font-bold text-primary">
            {{ $t('admin-canned-responses-title') }}
          </h1>
          <p class="text-secondary text-sm sm:text-base mt-1">
            {{ $t('admin-canned-responses-description') }}
          </p>
        </div>
        <div class="flex items-center gap-2 self-start sm:self-auto">
          <Button variant="secondary" icon="document" @click="openStarters">
            {{ $t('admin-canned-responses-browse-starters') }}
          </Button>
          <Button icon="add" @click="openCreate">
            {{ $t('admin-canned-responses-create') }}
          </Button>
        </div>
      </div>

      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />
      <AlertMessage
        v-if="loadError && responses.length === 0"
        type="error"
        :message="loadError"
      />

      <SearchInput
        v-if="!isFirstLoad && responses.length > 0"
        v-model="search"
        :placeholder="$t('admin-canned-responses-search-placeholder')"
      />

      <Skeleton
        v-if="isFirstLoad"
        :label="$t('admin-canned-responses-loading')"
        class="flex flex-col gap-2"
      >
        <div
          v-for="n in 4"
          :key="n"
          class="bg-surface border border-default rounded-lg p-3 sm:p-4 flex items-center gap-3"
        >
          <SkeletonBar class="h-4 w-48 max-w-full" />
          <SkeletonBar class="h-3 w-24 ml-auto" />
        </div>
      </Skeleton>

      <EmptyState
        v-else-if="responses.length === 0"
        icon="document"
        :title="$t('admin-canned-responses-empty-title')"
        :description="$t('admin-canned-responses-empty-description')"
        :action-label="$t('admin-canned-responses-browse-starters')"
        variant="card"
        @action="openStarters"
      />
      <EmptyState
        v-else-if="filtered.length === 0"
        icon="search"
        :title="$t('admin-canned-responses-no-matches-title')"
        :description="$t('admin-canned-responses-no-matches-description', { query: search })"
        variant="card"
      />

      <!-- Responsive list. On sm+ the row lays out as a 4-column grid
           with sortable headers (Name, Updated, Inserts, Actions). On
           mobile the row flips to a stacked card with title + body
           preview on top and a metadata + delete strip below; sort
           headers hide because the cells no longer line up under them.
           CSS `display: contents` on `<sm` sublayouts would have let
           the headers stay, but the cost in code is worse than just
           hiding them for the narrowest viewport. -->
      <div
        v-else
        class="bg-surface border border-default rounded-lg overflow-hidden"
      >
        <div
          class="hidden sm:grid sm:grid-cols-[1fr_140px_120px_56px] text-xs uppercase tracking-wide text-secondary border-b border-default"
        >
          <button
            class="text-left px-4 py-2 hover:bg-surface-hover flex items-center gap-1"
            @click="toggleSort('title')"
          >
            {{ $t('admin-canned-responses-column-name') }}
            <Icon
              v-if="sortKey === 'title'"
              :name="sortDir === 'asc' ? 'chevronUp' : 'chevronDown'"
              class="h-3 w-3"
            />
          </button>
          <button
            class="text-left px-4 py-2 hover:bg-surface-hover flex items-center gap-1"
            @click="toggleSort('updated')"
          >
            {{ $t('admin-canned-responses-column-updated') }}
            <Icon
              v-if="sortKey === 'updated'"
              :name="sortDir === 'asc' ? 'chevronUp' : 'chevronDown'"
              class="h-3 w-3"
            />
          </button>
          <button
            class="text-right px-4 py-2 hover:bg-surface-hover flex items-center justify-end gap-1"
            :title="$t('admin-canned-responses-column-inserts-title')"
            @click="toggleSort('inserts')"
          >
            {{ $t('admin-canned-responses-column-inserts') }}
            <Icon
              v-if="sortKey === 'inserts'"
              :name="sortDir === 'asc' ? 'chevronUp' : 'chevronDown'"
              class="h-3 w-3"
            />
          </button>
          <span class="px-4 py-2"></span>
        </div>
        <div
          v-for="row in filtered"
          :key="row.id"
          class="flex flex-col sm:grid sm:grid-cols-[1fr_140px_120px_56px] sm:items-center border-b border-default last:border-b-0 hover:bg-surface-hover transition-colors"
        >
          <button class="text-left px-4 pt-3 pb-1 sm:py-3 min-w-0" @click="openEdit(row)">
            <div class="font-medium text-primary truncate" v-html="highlight(row.title)" />
            <div class="text-xs text-tertiary truncate mt-0.5">
              {{ row.body.length > 120 ? row.body.slice(0, 120) + '…' : row.body }}
            </div>
          </button>
          <!-- Mobile-only meta strip: updated + inserts inline below
               the name, with the delete button at the far right. On
               sm+ this whole strip dissolves and each child sits in
               its own grid cell. -->
          <div
            class="flex items-center gap-3 px-4 pb-3 sm:pb-0 sm:contents text-xs text-tertiary"
          >
            <span
              class="sm:px-4 sm:py-3 sm:text-sm sm:text-secondary"
            >
              <span class="sm:hidden">{{ $t('admin-canned-responses-column-updated') }}: </span>
              {{ relativeTime(row.updated_at) }}
            </span>
            <span
              class="sm:px-4 sm:py-3 sm:text-sm sm:text-secondary sm:text-right sm:tabular-nums"
            >
              <span class="sm:hidden">{{ $t('admin-canned-responses-column-inserts') }}: </span>
              {{ row.inserts_30d }}
            </span>
            <div class="ml-auto sm:ml-0 sm:px-2 sm:py-3 sm:flex sm:justify-end">
              <button
                class="p-1.5 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-md transition-colors"
                :title="$t('admin-canned-responses-delete-title')"
                :aria-label="$t('admin-canned-responses-delete-aria', { name: row.title })"
                @click="confirmDelete(row)"
              >
                <Icon name="trash" />
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>

    <StarterCatalogModal
      :show="showStarters"
      @close="showStarters = false"
      @select="pickStarter"
    />

    <ConfirmModal
      :show="showDeleteConfirm"
      variant="danger"
      :title="$t('admin-canned-responses-delete-confirm-title')"
      :message="
        deleting
          ? $t('admin-canned-responses-delete-confirm-message', { name: deleting.title })
          : ''
      "
      :confirm-label="$t('admin-canned-responses-delete-confirm-button')"
      :loading="isDeleting"
      @confirm="doDelete"
      @close="showDeleteConfirm = false"
    />
  </div>
</template>

<style scoped>
:deep(mark) {
  background-color: rgb(var(--color-accent) / 0.25);
  color: inherit;
  padding: 0 2px;
  border-radius: 2px;
}
</style>
