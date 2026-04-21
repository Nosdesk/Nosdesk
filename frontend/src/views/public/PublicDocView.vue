<template>
  <PublicLayout content-class="max-w-lg mx-auto w-full">
    <article
      v-if="loading"
      class="bg-surface border border-default rounded-xl shadow-sm overflow-hidden flex flex-col"
      aria-busy="true"
      aria-label="Loading article"
    >
      <header class="p-5 sm:p-6 border-b border-default flex flex-col gap-2">
        <SkeletonBlock width="70%" height="1.5rem" />
        <SkeletonBlock width="8rem" height="0.75rem" />
      </header>
      <div class="p-5 sm:p-6 flex flex-col gap-2">
        <SkeletonBlock width="100%" height="0.875rem" />
        <SkeletonBlock width="95%" height="0.875rem" />
        <SkeletonBlock width="80%" height="0.875rem" />
      </div>
    </article>

    <FeatureDisabledNotice
      v-else-if="!enabled"
      title="Documentation is not available"
      message="Public documentation is currently disabled."
    />

    <template v-else-if="doc">
      <RouterLink
        to="/docs"
        class="inline-flex items-center gap-1.5 text-sm text-accent hover:opacity-90 font-medium self-start"
      >
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7" />
        </svg>
        All docs
      </RouterLink>

      <article class="bg-surface border border-default rounded-xl shadow-sm overflow-hidden flex flex-col">
        <header class="p-5 sm:p-6 border-b border-default flex items-start gap-3">
          <div v-if="doc.icon" class="shrink-0 w-10 h-10 rounded-lg bg-surface-alt flex items-center justify-center text-xl">
            {{ doc.icon }}
          </div>
          <div class="flex-1 min-w-0 flex flex-col gap-1">
            <h1 class="text-xl sm:text-2xl font-bold text-primary break-words">{{ doc.title }}</h1>
            <p class="text-xs text-tertiary">Last updated {{ formatDate(doc.updated_at) }}</p>
          </div>
        </header>

        <div class="p-5 sm:p-6">
          <div class="bg-status-info-muted border border-status-info/30 rounded-lg p-4 text-sm text-secondary">
            This article uses collaborative rich-text editing. A simplified view is shown here — for the full
            experience with comments and attachments please
            <RouterLink to="/login" class="text-accent hover:opacity-90 font-medium">sign in</RouterLink>.
          </div>
        </div>
      </article>
    </template>

    <div
      v-else
      class="bg-surface border border-default rounded-xl shadow-sm p-8 flex flex-col items-center gap-4 text-center"
    >
      <div class="w-12 h-12 rounded-full bg-surface-alt flex items-center justify-center">
        <svg class="w-6 h-6 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
            d="M9.172 16.172a4 4 0 015.656 0M9 10h.01M15 10h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
      </div>
      <div class="flex flex-col gap-1">
        <h2 class="text-lg font-semibold text-primary">Document not found</h2>
        <p class="text-sm text-secondary">It may have been moved or set to private.</p>
      </div>
      <RouterLink
        to="/docs"
        class="inline-flex items-center justify-center px-4 py-2 rounded-lg text-sm font-medium text-secondary bg-surface border border-default hover:bg-surface-hover hover:text-primary transition-colors"
      >
        Back to docs
      </RouterLink>
    </div>
  </PublicLayout>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { RouterLink } from 'vue-router';
import PublicLayout from './PublicLayout.vue';
import SkeletonBlock from './SkeletonBlock.vue';
import FeatureDisabledNotice from './FeatureDisabledNotice.vue';
import { usePublicSettingsStore } from '@/stores/publicSettings';
import { publicService, type PublicDoc } from '@/services/publicService';

const props = defineProps<{ slug: string }>();

const store = usePublicSettingsStore();
const loading = ref(true);
const doc = ref<PublicDoc | null>(null);
const enabled = computed(() => store.settings?.guest_public_docs_enabled === true);

function formatDate(iso: string) {
  try {
    return new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
  } catch {
    return iso;
  }
}

onMounted(async () => {
  await store.load();
  if (enabled.value) {
    try {
      doc.value = await publicService.getDoc(props.slug);
    } catch {
      doc.value = null;
    }
  }
  loading.value = false;
});
</script>
