<template>
  <PublicLayout content-class="max-w-lg mx-auto w-full">
    <!-- Skeleton: header + list rows. Matches the real layout. -->
    <template v-if="loading">
      <div class="flex flex-col gap-1 text-center">
        <SkeletonBlock width="10rem" height="1.5rem" rounded="rounded" />
        <SkeletonBlock width="14rem" height="0.875rem" rounded="rounded" />
      </div>
      <ul
        class="bg-surface border border-default rounded-xl shadow-sm divide-y divide-default overflow-hidden"
        aria-busy="true"
        :aria-label="t('public-docs-loading-aria')"
      >
        <li v-for="n in 4" :key="n" class="flex items-center gap-3 p-4">
          <SkeletonBlock width="2.25rem" height="2.25rem" rounded="rounded-lg" />
          <div class="flex-1 flex flex-col gap-1.5">
            <SkeletonBlock width="60%" height="0.875rem" />
            <SkeletonBlock width="30%" height="0.75rem" />
          </div>
        </li>
      </ul>
    </template>

    <FeatureDisabledNotice
      v-else-if="!enabled"
      :title="t('public-docs-disabled-title')"
      :message="t('public-docs-disabled-message')"
    />

    <template v-else>
      <div class="flex flex-col gap-1 text-center">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ t('public-docs-heading') }}</h1>
        <p class="text-sm text-secondary">{{ t('public-docs-tagline') }}</p>
      </div>

      <div v-if="searchEnabled" class="relative">
        <svg
          class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-tertiary pointer-events-none"
          fill="none" stroke="currentColor" viewBox="0 0 24 24"
        >
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
            d="M21 21l-4.35-4.35M11 19a8 8 0 100-16 8 8 0 000 16z" />
        </svg>
        <input
          v-model.trim="query"
          @input="runSearch"
          :placeholder="t('public-docs-search-placeholder')"
          :aria-label="t('public-docs-search-aria')"
          class="w-full pl-10 pr-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent transition-colors"
        />
      </div>

      <div
        v-if="!visible.length"
        class="bg-surface border border-default rounded-xl shadow-sm p-8 flex flex-col items-center gap-3 text-center"
      >
        <div class="w-12 h-12 rounded-full bg-surface-alt flex items-center justify-center">
          <svg class="w-6 h-6 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
              d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
          </svg>
        </div>
        <p class="text-secondary text-sm">
          {{ query ? t('public-docs-no-results') : t('public-docs-empty') }}
        </p>
      </div>

      <ul v-else class="bg-surface border border-default rounded-xl shadow-sm divide-y divide-default overflow-hidden">
        <li v-for="doc in visible" :key="doc.id">
          <RouterLink
            :to="`/docs/${doc.slug}`"
            class="flex items-center gap-3 p-4 hover:bg-surface-hover transition-colors"
          >
            <div class="shrink-0 w-9 h-9 rounded-lg bg-surface-alt flex items-center justify-center text-base">
              <span v-if="doc.icon">{{ doc.icon }}</span>
              <svg v-else class="w-4 h-4 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
                  d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
              </svg>
            </div>
            <div class="flex-1 min-w-0 flex flex-col gap-0.5">
              <div class="text-primary font-medium truncate">{{ doc.title }}</div>
              <div class="text-tertiary text-xs">{{ t('public-docs-updated', { date: formatDate(doc.updated_at) }) }}</div>
            </div>
            <svg class="shrink-0 w-4 h-4 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
            </svg>
          </RouterLink>
        </li>
      </ul>
    </template>
  </PublicLayout>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { RouterLink } from 'vue-router';
import { useQuery } from '@pinia/colada';
import { useFluent } from 'fluent-vue';
import PublicLayout from './PublicLayout.vue';
import SkeletonBlock from './SkeletonBlock.vue';
import FeatureDisabledNotice from './FeatureDisabledNotice.vue';
import { usePublicSettingsStore } from '@nosdesk/core/stores/publicSettings';
import { publicService, type PublicDocSummary } from '@nosdesk/core/services/publicService';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const store = usePublicSettingsStore();
const settingsLoaded = ref(false);
const searchResults = ref<PublicDocSummary[] | null>(null);
const query = ref('');

const enabled = computed(() => store.settings?.guest_public_docs_enabled === true);
const searchEnabled = computed(() => store.settings?.guest_kb_search_enabled === true);

// Cache-first: the doc index renders instantly on revisit then refreshes
// silently. Live search stays a debounced manual fetch (it's transient,
// not revisit-cacheable content).
const docsQuery = useQuery({
  key: () => ['public-docs'],
  query: () => publicService.listDocs(),
  enabled: () => settingsLoaded.value && enabled.value,
});
const docs = computed<PublicDocSummary[]>(() => docsQuery.data.value ?? []);
const loading = computed(
  () =>
    !settingsLoaded.value ||
    (enabled.value && docsQuery.asyncStatus.value === 'loading' && docs.value.length === 0),
);
const visible = computed(() => searchResults.value ?? docs.value);

function formatDate(iso: string) {
  try {
    return new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
  } catch {
    return iso;
  }
}

let searchTimer: number | undefined;
function runSearch() {
  window.clearTimeout(searchTimer);
  const q = query.value;
  if (!q) {
    searchResults.value = null;
    return;
  }
  searchTimer = window.setTimeout(async () => {
    try {
      searchResults.value = await publicService.searchDocs(q);
    } catch {
      searchResults.value = [];
    }
  }, 200);
}

onMounted(async () => {
  try {
    await store.load();
  } finally {
    settingsLoaded.value = true;
  }
});
</script>
