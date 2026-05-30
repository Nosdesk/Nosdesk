<script setup lang="ts">
/**
 * Admin CRUD page for the canned-response library. Workspace-wide
 * shared list; any tech can read in the composer picker, only
 * admins reach this page.
 *
 * Modal-based create/edit lands here as the v1 surface; a follow-up
 * commit upgrades to a full-page editor with chip-aware variable
 * tokens and a sample-data preview pane.
 */
import { ref, computed, onMounted } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import { formatDistanceToNow } from 'date-fns';

import AlertMessage from '@/components/common/AlertMessage.vue';
import Button from '@/components/common/Button.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import FormInput from '@/components/common/FormInput.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import Icon from '@/components/common/Icon.vue';
import Modal from '@/components/Modal.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import cannedResponsesService, {
  findUnknownVariables,
  CANNED_RESPONSE_VARIABLES,
  type CannedResponseListItem,
} from '@/services/cannedResponsesService';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

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
const filtered = computed<CannedResponseListItem[]>(() => {
  if (searchTerms.value.length === 0) return sorted.value;
  return sorted.value.filter((r) => {
    const haystack = `${r.title}\n${r.body}`.toLowerCase();
    return searchTerms.value.every((term) => haystack.includes(term));
  });
});

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

// Hit highlighter. Wraps matched substrings of every search term
// in <mark> for the title column; body matches drive the filter
// but don't render (the body is collapsed on the row).
function highlight(value: string): string {
  if (searchTerms.value.length === 0) return escapeHtml(value);
  const pattern = new RegExp(
    `(${searchTerms.value.map(escapeRegex).join('|')})`,
    'gi',
  );
  return escapeHtml(value).replace(pattern, '<mark>$1</mark>');
}
function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}
function escapeRegex(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

// Editor modal. One modal handles both create and edit; `editing`
// being null is the create case, otherwise it's the row under edit.
const showEditor = ref(false);
const editing = ref<CannedResponseListItem | null>(null);
const form = ref({ title: '', body: '' });
const isSaving = ref(false);
const formError = ref('');
const successMessage = ref('');
const errorMessage = ref('');

function openCreate(): void {
  editing.value = null;
  form.value = { title: '', body: '' };
  formError.value = '';
  showEditor.value = true;
}
function openEdit(row: CannedResponseListItem): void {
  editing.value = row;
  form.value = { title: row.title, body: row.body };
  formError.value = '';
  showEditor.value = true;
}
function closeEditor(): void {
  showEditor.value = false;
  editing.value = null;
  formError.value = '';
}

// Inline unknown-variable warning. The save round-trip would reject
// these anyway, but surfacing them at edit time saves a round-trip
// and points at the typo directly.
const unknownVariables = computed<string[]>(() => findUnknownVariables(form.value.body));

const editorTitle = computed(() =>
  editing.value
    ? t('admin-canned-responses-edit-title')
    : t('admin-canned-responses-create-title'),
);
const editorSubmitLabel = computed(() =>
  editing.value
    ? t('admin-canned-responses-save')
    : t('admin-canned-responses-create-submit'),
);

async function submitEditor(): Promise<void> {
  const title = form.value.title.trim();
  const body = form.value.body.trim();
  if (!title) {
    formError.value = t('admin-canned-responses-error-title-required');
    return;
  }
  if (!body) {
    formError.value = t('admin-canned-responses-error-body-required');
    return;
  }
  if (unknownVariables.value.length > 0) {
    formError.value = t('admin-canned-responses-error-unknown-variables', {
      names: unknownVariables.value.join(', '),
    });
    return;
  }
  isSaving.value = true;
  formError.value = '';
  try {
    if (editing.value) {
      await cannedResponsesService.update(editing.value.id, { title, body });
      successMessage.value = t('admin-canned-responses-success-updated');
    } else {
      await cannedResponsesService.create({ title, body });
      successMessage.value = t('admin-canned-responses-success-created');
    }
    showEditor.value = false;
    editing.value = null;
    await queryCache.invalidateQueries({ key: CANNED_RESPONSES_KEY });
    setTimeout(() => (successMessage.value = ''), 3000);
  } catch (error) {
    const axiosError = error as { response?: { data?: string } };
    formError.value =
      axiosError.response?.data || t('admin-canned-responses-error-save');
  } finally {
    isSaving.value = false;
  }
}

// Delete confirmation. The list refresh is keyed off the same
// Pinia Colada invalidation as edit so the row vanishes the
// moment the request returns 2xx.
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
    const axiosError = error as { response?: { data?: string } };
    errorMessage.value =
      axiosError.response?.data || t('admin-canned-responses-error-delete');
    setTimeout(() => (errorMessage.value = ''), 5000);
  } finally {
    isDeleting.value = false;
  }
}

// Relative "updated 3 days ago" formatter, matches the convention
// used by ApiTokensView for last-used / created timestamps.
function relativeTime(iso: string): string {
  try {
    return formatDistanceToNow(new Date(iso), { addSuffix: true });
  } catch {
    return iso;
  }
}

onMounted(() => {
  // Query auto-fetches via useQuery; nothing else to prime here.
});
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <!-- Heading + create CTA -->
      <div class="mb-2 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
        <div>
          <h1 class="text-xl sm:text-2xl font-bold text-primary">
            {{ $t('admin-canned-responses-title') }}
          </h1>
          <p class="text-secondary text-sm sm:text-base mt-1">
            {{ $t('admin-canned-responses-description') }}
          </p>
        </div>
        <Button @click="openCreate">
          <Icon name="add" />
          <span>{{ $t('admin-canned-responses-create') }}</span>
        </Button>
      </div>

      <!-- Mutation feedback -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />
      <AlertMessage
        v-if="loadError && responses.length === 0"
        type="error"
        :message="loadError"
      />

      <!-- Search input (always rendered above the list when not in
           first-load skeleton state so admins can pre-type while
           the list is hydrating). -->
      <div v-if="!isFirstLoad" class="relative">
        <Icon
          name="search"
          class="absolute left-3 top-1/2 -translate-y-1/2 text-secondary pointer-events-none"
        />
        <input
          v-model="search"
          type="search"
          :placeholder="$t('admin-canned-responses-search-placeholder')"
          :aria-label="$t('admin-canned-responses-search-aria')"
          class="w-full pl-9 pr-3 py-2 text-sm border border-default rounded-lg bg-surface text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent/40"
        />
      </div>

      <!-- First-load skeleton -->
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

      <!-- Empty state. Shown for both "library is empty" and "search
           matched nothing"; the CTA varies by case. -->
      <EmptyState
        v-else-if="responses.length === 0"
        icon="document"
        :title="$t('admin-canned-responses-empty-title')"
        :description="$t('admin-canned-responses-empty-description')"
        :action-label="$t('admin-canned-responses-create')"
        variant="card"
        @action="openCreate"
      />
      <EmptyState
        v-else-if="filtered.length === 0"
        icon="search"
        :title="$t('admin-canned-responses-no-matches-title')"
        :description="$t('admin-canned-responses-no-matches-description', { query: search })"
        variant="card"
      />

      <!-- Main list, table-style with sortable column headers. -->
      <div
        v-else
        class="bg-surface border border-default rounded-lg overflow-hidden"
      >
        <div
          class="grid grid-cols-[1fr_140px_120px_56px] text-xs uppercase tracking-wide text-secondary border-b border-default"
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
            @click="toggleSort('inserts')"
            :title="$t('admin-canned-responses-column-inserts-title')"
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
          class="grid grid-cols-[1fr_140px_120px_56px] items-center border-b border-default last:border-b-0 hover:bg-surface-hover transition-colors"
        >
          <button
            class="text-left px-4 py-3 min-w-0"
            @click="openEdit(row)"
          >
            <div class="font-medium text-primary truncate" v-html="highlight(row.title)" />
            <div class="text-xs text-tertiary truncate mt-0.5">
              {{ row.body.length > 120 ? row.body.slice(0, 120) + '…' : row.body }}
            </div>
          </button>
          <div class="px-4 py-3 text-sm text-secondary">
            {{ relativeTime(row.updated_at) }}
          </div>
          <div class="px-4 py-3 text-sm text-secondary text-right tabular-nums">
            {{ row.inserts_30d }}
          </div>
          <div class="px-2 py-3 flex justify-end">
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

    <!-- Create / edit modal -->
    <Modal :show="showEditor" :title="editorTitle" size="lg" @close="closeEditor">
      <form class="flex flex-col gap-4" @submit.prevent="submitEditor">
        <AlertMessage v-if="formError" type="error" :message="formError" />
        <FormInput
          v-model="form.title"
          :label="$t('admin-canned-responses-field-title')"
          :placeholder="$t('admin-canned-responses-field-title-placeholder')"
          required
        />
        <FormTextarea
          v-model="form.body"
          :label="$t('admin-canned-responses-field-body')"
          :placeholder="$t('admin-canned-responses-field-body-placeholder')"
          :hint="
            $t('admin-canned-responses-field-body-hint', {
              variables: CANNED_RESPONSE_VARIABLES.map((v) => '{{' + v + '}}').join(', '),
            })
          "
          :rows="10"
          required
        />
        <div
          v-if="unknownVariables.length > 0"
          class="text-xs text-status-warning"
        >
          {{
            $t('admin-canned-responses-warn-unknown-variables', {
              names: unknownVariables.join(', '),
            })
          }}
        </div>
        <div class="flex justify-end gap-2">
          <Button variant="secondary" type="button" @click="closeEditor">
            {{ $t('admin-canned-responses-cancel') }}
          </Button>
          <Button type="submit" :loading="isSaving">
            {{ editorSubmitLabel }}
          </Button>
        </div>
      </form>
    </Modal>

    <!-- Delete confirmation -->
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
