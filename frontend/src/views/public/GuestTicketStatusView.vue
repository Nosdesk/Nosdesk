<template>
  <PublicLayout content-class="max-w-md mx-auto w-full">
    <!-- Skeleton of the status card, same dimensions as the real content
         so nothing shifts when data arrives. -->
    <div
      v-if="loading"
      class="bg-surface border border-default rounded-xl shadow-sm overflow-hidden flex flex-col"
      aria-busy="true"
      :aria-label="t('guest-status-loading-aria')"
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
      :title="t('guest-status-disabled-title')"
      :message="t('guest-status-disabled-message')"
    />

    <template v-else-if="ticket">
      <div class="bg-surface border border-default rounded-xl shadow-sm overflow-hidden flex flex-col">
        <div class="p-5 sm:p-6 border-b border-default flex items-start justify-between gap-3 flex-wrap">
          <div class="min-w-0 flex flex-col gap-1">
            <div class="text-xs font-medium text-tertiary uppercase tracking-wide">
              {{ t('guest-status-ticket-number', { id: ticket.ticket_id }) }}
            </div>
            <h1 class="text-xl font-semibold text-primary break-words">{{ ticket.title }}</h1>
          </div>
          <span
            class="shrink-0 inline-flex items-center px-2.5 py-1 rounded-full text-xs font-medium border"
            :class="statusBadge"
          >
            {{ statusLabel }}
          </span>
        </div>

        <dl class="p-5 sm:p-6 grid grid-cols-2 gap-x-6 gap-y-4 text-sm">
          <div class="flex flex-col gap-0.5">
            <dt class="text-tertiary">{{ t('guest-status-priority') }}</dt>
            <dd class="text-primary capitalize">{{ ticket.priority }}</dd>
          </div>
          <div class="flex flex-col gap-0.5">
            <dt class="text-tertiary">{{ t('guest-status-opened') }}</dt>
            <dd class="text-primary">{{ formatDate(ticket.created_at) }}</dd>
          </div>
          <div class="flex flex-col gap-0.5">
            <dt class="text-tertiary">{{ t('guest-status-last-updated') }}</dt>
            <dd class="text-primary">{{ formatDate(ticket.updated_at) }}</dd>
          </div>
          <div v-if="ticket.closed_at" class="flex flex-col gap-0.5">
            <dt class="text-tertiary">{{ t('guest-status-closed') }}</dt>
            <dd class="text-primary">{{ formatDate(ticket.closed_at) }}</dd>
          </div>
        </dl>
      </div>

      <p class="text-sm text-tertiary text-center">
        {{ t('guest-status-reply-prefix') }}
        <RouterLink to="/login" class="text-accent hover:opacity-90 font-medium">{{ t('guest-submit-sign-in') }}</RouterLink>
        {{ t('guest-status-reply-suffix') }}
      </p>
    </template>

    <div
      v-else
      class="bg-surface border border-default rounded-xl shadow-sm p-8 flex flex-col items-center gap-4 text-center"
    >
      <div class="w-12 h-12 rounded-full bg-surface-alt flex items-center justify-center">
        <Icon name="search" size="lg" class="text-tertiary" />
      </div>
      <div class="flex flex-col gap-1">
        <h2 class="text-lg font-semibold text-primary">{{ t('guest-status-not-found-title') }}</h2>
        <p class="text-sm text-secondary">{{ t('guest-status-not-found-message') }}</p>
      </div>
    </div>
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
import Icon from '@/components/common/Icon.vue';
import { usePublicSettingsStore } from '@/stores/publicSettings';
import { publicService, type GuestTicketStatus } from '@nosdesk/core/services/publicService';
import { coarseStatusBucket } from '@nosdesk/core/types/workflow';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const props = defineProps<{ token: string }>();

const store = usePublicSettingsStore();
const settingsLoaded = ref(false);
const enabled = computed(() => store.settings?.guest_ticket_lookup_enabled === true);

// Cache-first: the status is keyed by the lookup token, so a revisit
// renders instantly then refreshes silently. Gated on the lookup feature
// being on (and settings having loaded).
const ticketQuery = useQuery({
  key: () => ['guest-ticket-status', props.token],
  query: () => publicService.getTicketStatus(props.token),
  enabled: () => settingsLoaded.value && enabled.value,
});
const ticket = computed<GuestTicketStatus | null>(() => ticketQuery.data.value ?? null);
const loading = computed(
  () =>
    !settingsLoaded.value ||
    (enabled.value && ticketQuery.asyncStatus.value === 'loading' && !ticket.value),
);

const statusBadge = computed(() => {
  const c = ticket.value?.category;
  if (!c) return 'bg-status-open-muted border-status-open/40 text-status-open';
  const b = coarseStatusBucket(c);
  if (b === 'closed') {
    return 'bg-status-closed-muted border-status-closed/40 text-status-closed';
  }
  if (b === 'in-progress') {
    return 'bg-status-in-progress-muted border-status-in-progress/40 text-status-in-progress';
  }
  return 'bg-status-open-muted border-status-open/40 text-status-open';
});

const statusLabel = computed(() => {
  const c = ticket.value?.category;
  if (!c) return '';
  const b = coarseStatusBucket(c);
  return b === 'closed' ? t('status-closed') : b === 'in-progress' ? t('status-in-progress') : t('status-open');
});

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
  try {
    await store.load();
  } finally {
    settingsLoaded.value = true;
  }
});
</script>
