<script setup lang="ts">
/**
 * Admin list of asset kinds. Read + delete + navigate-to-editor;
 * full CRUD lives on the AssetKindEditView at /new and /:id so the
 * editor surface (especially the schema builder coming next) has
 * the room it needs without crowding the registry view.
 *
 * The list reads from the shared `useAssetKindsQuery` so edits
 * from the editor route invalidate this view automatically, and
 * every asset-detail picker is on the same cache.
 */
import { computed, ref } from 'vue';
import { useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { formatDistanceToNow } from 'date-fns';

import AlertMessage from '@/components/common/AlertMessage.vue';
import BackButton from '@/components/common/BackButton.vue';
import Button from '@/components/common/Button.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Icon from '@/components/common/Icon.vue';
import SearchInput from '@/components/common/SearchInput.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import { useAssetKindsQuery } from '@/composables/useAssetKindsQuery';
import {
  assetKindsService,
  ASSET_KINDS_QUERY_KEY,
  type AssetKind,
} from '@/services/assetKindsService';
import { extractErrorMessage } from '@/utils/errors';
import { highlightTerms } from '@nosdesk/core/utils/highlight';
import { useToastStore } from '@/stores/toast';
import { useQueryCache } from '@pinia/colada';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const router = useRouter();
const toast = useToastStore();
const queryCache = useQueryCache();

const { kinds, error: loadQueryError, isFirstLoad } = useAssetKindsQuery();
const loadError = computed(() =>
  loadQueryError.value ? t('admin-asset-kinds-error-load') : '',
);

// Search across label + slug + description; multi-term AND, case-
// insensitive. Filters the same list the registry returns.
const search = ref('');
const searchTerms = computed<string[]>(() =>
  search.value
    .toLowerCase()
    .split(/\s+/)
    .map((s) => s.trim())
    .filter(Boolean),
);
const filteredKinds = computed<AssetKind[]>(() => {
  if (searchTerms.value.length === 0) return kinds.value;
  return kinds.value.filter((k) => {
    const haystack = `${k.label} ${k.slug} ${k.description ?? ''}`.toLowerCase();
    return searchTerms.value.every((term) => haystack.includes(term));
  });
});

// Builtins first (most-used path), then custom; within each group
// the registry's own sort_order + label ordering is preserved.
const builtinKinds = computed(() => filteredKinds.value.filter((k) => k.is_builtin));
const customKinds = computed(() => filteredKinds.value.filter((k) => !k.is_builtin));

// Delete flow. We fetch the usage count when the admin clicks the
// trash button so the ConfirmModal can warn "N assets currently
// use this kind" instead of silently orphaning rows. Failure to
// fetch the count doesn't block the confirm; the modal degrades
// to the old (count-less) wording so the admin isn't stuck.
const pendingDelete = ref<AssetKind | null>(null);
const pendingDeleteUsage = ref<number | null>(null);
const isLoadingUsage = ref(false);
const isDeleting = ref(false);
const errorMessage = ref('');

async function startDelete(kind: AssetKind): Promise<void> {
  pendingDelete.value = kind;
  pendingDeleteUsage.value = null;
  isLoadingUsage.value = true;
  try {
    const { asset_count } = await assetKindsService.getUsage(kind.id);
    pendingDeleteUsage.value = asset_count;
  } catch {
    // Leave usage null; modal renders the count-less variant.
  } finally {
    isLoadingUsage.value = false;
  }
}

function cancelDelete(): void {
  pendingDelete.value = null;
  pendingDeleteUsage.value = null;
}

async function confirmDelete(): Promise<void> {
  if (!pendingDelete.value) return;
  const kind = pendingDelete.value;
  isDeleting.value = true;
  try {
    await assetKindsService.delete(kind.id);
    toast.success(t('admin-asset-kinds-deleted', { label: kind.label }));
    cancelDelete();
    await queryCache.invalidateQueries({ key: ASSET_KINDS_QUERY_KEY });
  } catch (error) {
    errorMessage.value = extractErrorMessage(error, t('admin-asset-kinds-error-delete'));
    setTimeout(() => (errorMessage.value = ''), 5000);
  } finally {
    isDeleting.value = false;
  }
}

const deleteMessage = computed<string>(() => {
  if (!pendingDelete.value) return '';
  const label = pendingDelete.value.label;
  if (isLoadingUsage.value || pendingDeleteUsage.value === null) {
    return t('admin-asset-kinds-delete-confirm', { label });
  }
  if (pendingDeleteUsage.value === 0) {
    return t('admin-asset-kinds-delete-confirm-zero', { label });
  }
  return t('admin-asset-kinds-delete-confirm-with-count', {
    label,
    count: pendingDeleteUsage.value,
  });
});

// Highlight + relative-time helpers, same shape as the
// canned-responses sweep.
const highlight = (value: string): string => highlightTerms(value, searchTerms.value);
function relativeTime(iso: string): string {
  try {
    return formatDistanceToNow(new Date(iso), { addSuffix: true });
  } catch {
    return iso;
  }
}

function openCreate(): void {
  router.push({ name: 'admin-asset-kinds-new' });
}
function openEdit(kind: AssetKind): void {
  router.push({ name: 'admin-asset-kinds-edit', params: { id: kind.id } });
}
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-5xl">
      <!-- Heading + create CTA. BackButton lands on whatever the
           previous route was (typically the admin landing), and the
           shared component renders the same chevron + label every
           other detail-aware admin view does. -->
      <div class="flex flex-col gap-2">
        <BackButton :fallback-route="'/admin'" :label="t('admin-asset-kinds-back-label')" compact />
        <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
          <div class="flex flex-col gap-1">
            <h1 class="text-xl sm:text-2xl font-bold text-primary">
              {{ t('admin-asset-kinds-title') }}
            </h1>
            <p class="text-secondary text-sm sm:text-base">
              {{ t('admin-asset-kinds-description') }}
            </p>
          </div>
          <Button icon="add" class="self-start sm:self-auto" @click="openCreate">
            {{ t('admin-asset-kinds-new') }}
          </Button>
        </div>
      </div>

      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />
      <AlertMessage
        v-if="loadError && kinds.length === 0"
        type="error"
        :message="loadError"
      />

      <SearchInput
        v-if="!isFirstLoad && kinds.length > 0"
        v-model="search"
        :placeholder="t('admin-asset-kinds-search-placeholder')"
      />

      <!-- First-load skeleton mirrors the row shape so the shell
           doesn't shift when data lands. -->
      <Skeleton
        v-if="isFirstLoad"
        :label="t('admin-asset-kinds-loading')"
        class="flex flex-col gap-2"
      >
        <div
          v-for="n in 5"
          :key="n"
          class="bg-surface border border-default rounded-lg p-4 flex items-center gap-3"
        >
          <SkeletonBar class="h-4 w-48 max-w-full" />
          <SkeletonBar class="h-3 w-24 ml-auto" />
        </div>
      </Skeleton>

      <EmptyState
        v-else-if="kinds.length === 0"
        icon="document"
        :title="t('admin-asset-kinds-empty-title')"
        :description="t('admin-asset-kinds-empty-description')"
        :action-label="t('admin-asset-kinds-new')"
        variant="card"
        @action="openCreate"
      />
      <EmptyState
        v-else-if="filteredKinds.length === 0"
        icon="search"
        :title="t('admin-asset-kinds-no-matches-title')"
        :description="t('admin-asset-kinds-no-matches-description', { query: search })"
        variant="card"
      />

      <!-- Built-in kinds. Shown as a separate group so the
           registry's two tiers stay visible: shipped-with-product
           vs admin-authored. Same row shape as custom; the only
           difference is the delete button is disabled with a
           tooltip explaining why instead of being hidden (the
           previous UI hid it, which read as "missing affordance"
           rather than "deliberate constraint"). -->
      <section
        v-else-if="builtinKinds.length > 0 || customKinds.length > 0"
        class="flex flex-col gap-6"
      >
        <div v-if="builtinKinds.length > 0" class="flex flex-col gap-2">
          <div class="flex items-center gap-2">
            <h2 class="text-sm font-medium uppercase tracking-wide text-secondary">
              {{ t('admin-asset-kinds-builtin-heading') }}
            </h2>
            <span class="text-xs text-tertiary">{{ builtinKinds.length }}</span>
          </div>
          <p class="text-xs text-tertiary">
            {{ t('admin-asset-kinds-builtin-description') }}
          </p>
          <div class="bg-surface border border-default rounded-lg overflow-hidden">
            <div
              v-for="(kind, i) in builtinKinds"
              :key="kind.id"
              :class="[
                'flex flex-col sm:flex-row sm:items-center gap-2 sm:gap-3 p-3 sm:p-4',
                i < builtinKinds.length - 1 ? 'border-b border-default' : '',
              ]"
            >
              <button
                class="flex-1 min-w-0 text-left flex flex-col gap-1"
                @click="openEdit(kind)"
              >
                <div class="flex flex-wrap items-center gap-2">
                  <span
                    class="font-medium text-primary truncate"
                    v-html="highlight(kind.label)"
                  />
                  <code
                    class="text-xs text-tertiary"
                    v-html="highlight(kind.slug)"
                  />
                  <span
                    class="text-xs px-1.5 py-0.5 rounded bg-accent/10 text-accent"
                  >
                    {{ t(`admin-asset-kinds-category-${kind.category}`) }}
                  </span>
                  <span
                    class="text-xs px-1.5 py-0.5 rounded bg-surface-alt text-tertiary"
                  >
                    {{ t('admin-asset-kinds-builtin-tag') }}
                  </span>
                </div>
                <p
                  v-if="kind.description"
                  class="text-sm text-secondary truncate"
                  v-html="highlight(kind.description)"
                />
              </button>
              <div class="flex items-center gap-3 text-xs text-tertiary">
                <span>{{ t('admin-asset-kinds-updated', { when: relativeTime(kind.updated_at) }) }}</span>
                <button
                  class="p-1.5 text-tertiary rounded-md cursor-not-allowed opacity-50"
                  :title="t('admin-asset-kinds-builtin-no-delete')"
                  :aria-label="t('admin-asset-kinds-builtin-no-delete')"
                  disabled
                >
                  <Icon name="trash" />
                </button>
              </div>
            </div>
          </div>
        </div>

        <div v-if="customKinds.length > 0" class="flex flex-col gap-2">
          <div class="flex items-center gap-2">
            <h2 class="text-sm font-medium uppercase tracking-wide text-secondary">
              {{ t('admin-asset-kinds-custom-heading') }}
            </h2>
            <span class="text-xs text-tertiary">{{ customKinds.length }}</span>
          </div>
          <p class="text-xs text-tertiary">
            {{ t('admin-asset-kinds-custom-description') }}
          </p>
          <div class="bg-surface border border-default rounded-lg overflow-hidden">
            <div
              v-for="(kind, i) in customKinds"
              :key="kind.id"
              :class="[
                'flex flex-col sm:flex-row sm:items-center gap-2 sm:gap-3 p-3 sm:p-4 hover:bg-surface-hover transition-colors',
                i < customKinds.length - 1 ? 'border-b border-default' : '',
              ]"
            >
              <button
                class="flex-1 min-w-0 text-left flex flex-col gap-1"
                @click="openEdit(kind)"
              >
                <div class="flex flex-wrap items-center gap-2">
                  <span
                    class="font-medium text-primary truncate"
                    v-html="highlight(kind.label)"
                  />
                  <code
                    class="text-xs text-tertiary"
                    v-html="highlight(kind.slug)"
                  />
                  <span
                    class="text-xs px-1.5 py-0.5 rounded bg-accent/10 text-accent"
                  >
                    {{ t(`admin-asset-kinds-category-${kind.category}`) }}
                  </span>
                </div>
                <p
                  v-if="kind.description"
                  class="text-sm text-secondary truncate"
                  v-html="highlight(kind.description)"
                />
              </button>
              <div class="flex items-center gap-3 text-xs text-tertiary">
                <span>{{ t('admin-asset-kinds-updated', { when: relativeTime(kind.updated_at) }) }}</span>
                <button
                  class="p-1.5 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-md transition-colors"
                  :title="t('admin-asset-kinds-delete')"
                  :aria-label="t('admin-asset-kinds-delete-aria', { label: kind.label })"
                  @click="startDelete(kind)"
                >
                  <Icon name="trash" />
                </button>
              </div>
            </div>
          </div>
        </div>
      </section>
    </div>

    <ConfirmModal
      :show="pendingDelete !== null"
      :title="t('admin-asset-kinds-delete-confirm-title')"
      :confirm-label="t('admin-asset-kinds-delete')"
      :loading="isDeleting"
      variant="danger"
      :message="deleteMessage"
      @confirm="confirmDelete"
      @close="cancelDelete"
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
