<template>
  <PublicLayout content-class="max-w-md mx-auto w-full">
    <!-- Skeleton of the status card — same dimensions as the real content
         so nothing shifts when data arrives. -->
    <div
      v-if="loading"
      class="bg-surface border border-default rounded-xl shadow-sm overflow-hidden flex flex-col"
      aria-busy="true"
      aria-label="Loading ticket"
    >
      <div class="p-5 sm:p-6 border-b border-default flex items-start justify-between gap-3">
        <div class="min-w-0 flex flex-col gap-2 flex-1">
          <SkeletonBlock width="5rem" height="0.75rem" />
          <SkeletonBlock width="75%" height="1.25rem" />
        </div>
        <SkeletonBlock width="4.5rem" height="1.5rem" rounded="rounded-full" />
      </div>
      <dl class="p-5 sm:p-6 grid grid-cols-2 gap-x-6 gap-y-4">
        <div v-for="n in 3" :key="n" class="flex flex-col gap-1.5">
          <SkeletonBlock width="4rem" height="0.75rem" />
          <SkeletonBlock width="70%" height="0.875rem" />
        </div>
      </dl>
    </div>

    <FeatureDisabledNotice
      v-else-if="!enabled"
      title="Status lookup is not available"
      message="Guest ticket status lookup is currently disabled."
    />

    <template v-else-if="ticket">
      <div class="bg-surface border border-default rounded-xl shadow-sm overflow-hidden flex flex-col">
        <div class="p-5 sm:p-6 border-b border-default flex items-start justify-between gap-3 flex-wrap">
          <div class="min-w-0 flex flex-col gap-1">
            <div class="text-xs font-medium text-tertiary uppercase tracking-wide">
              Ticket #{{ ticket.ticket_id }}
            </div>
            <h1 class="text-xl font-semibold text-primary break-words">{{ ticket.title }}</h1>
          </div>
          <span
            class="shrink-0 inline-flex items-center px-2.5 py-1 rounded-full text-xs font-medium border"
            :class="statusBadge"
          >
            {{ formatStatus(ticket.status) }}
          </span>
        </div>

        <dl class="p-5 sm:p-6 grid grid-cols-2 gap-x-6 gap-y-4 text-sm">
          <div class="flex flex-col gap-0.5">
            <dt class="text-tertiary">Priority</dt>
            <dd class="text-primary capitalize">{{ ticket.priority }}</dd>
          </div>
          <div class="flex flex-col gap-0.5">
            <dt class="text-tertiary">Opened</dt>
            <dd class="text-primary">{{ formatDate(ticket.created_at) }}</dd>
          </div>
          <div class="flex flex-col gap-0.5">
            <dt class="text-tertiary">Last updated</dt>
            <dd class="text-primary">{{ formatDate(ticket.updated_at) }}</dd>
          </div>
          <div v-if="ticket.closed_at" class="flex flex-col gap-0.5">
            <dt class="text-tertiary">Closed</dt>
            <dd class="text-primary">{{ formatDate(ticket.closed_at) }}</dd>
          </div>
        </dl>
      </div>

      <p class="text-sm text-tertiary text-center">
        Need to reply?
        <RouterLink to="/login" class="text-accent hover:opacity-90 font-medium">Sign in</RouterLink>
        to add a comment.
      </p>
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
        <h2 class="text-lg font-semibold text-primary">Ticket not found</h2>
        <p class="text-sm text-secondary">The link may have expired or been mistyped.</p>
      </div>
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
import { publicService, type GuestTicketStatus } from '@/services/publicService';

const props = defineProps<{ token: string }>();

const store = usePublicSettingsStore();
const loading = ref(true);
const ticket = ref<GuestTicketStatus | null>(null);
const enabled = computed(() => store.settings?.guest_ticket_lookup_enabled === true);

const statusBadge = computed(() => {
  const s = ticket.value?.status?.toLowerCase() ?? '';
  if (s.includes('closed')) {
    return 'bg-status-closed-muted border-status-closed/40 text-status-closed';
  }
  if (s.includes('progress')) {
    return 'bg-status-in-progress-muted border-status-in-progress/40 text-status-in-progress';
  }
  return 'bg-status-open-muted border-status-open/40 text-status-open';
});

function formatStatus(raw: string) {
  return raw.replace(/[-_]/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase());
}

function formatDate(iso: string) {
  try {
    return new Date(iso).toLocaleString(undefined, {
      dateStyle: 'medium',
      timeStyle: 'short'
    });
  } catch {
    return iso;
  }
}

onMounted(async () => {
  await store.load();
  if (enabled.value) {
    try {
      ticket.value = await publicService.getTicketStatus(props.token);
    } catch {
      ticket.value = null;
    }
  }
  loading.value = false;
});
</script>
