<script setup lang="ts">
import { computed, ref } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery } from '@pinia/colada';
import BackButton from '@/components/common/BackButton.vue';
import Icon from '@/components/common/Icon.vue';
import Spinner from '@/components/common/Spinner.vue';
import AsyncBoundary from '@/components/common/AsyncBoundary.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import PullToRefresh from '@/components/common/PullToRefresh.vue';
import { groupService } from '@nosdesk/core/services/groupService';
import { formatDate } from '@nosdesk/core/utils/dateUtils';
import { useAuthStore } from '@/stores/auth';
import { useColorFilter } from '@/composables/useColorFilter';
import type { GroupDetails } from '@nosdesk/core/types/group';

const route = useRoute();
const router = useRouter();
const authStore = useAuthStore();

// Pull-to-refresh (Tauri app): the routed root is the scroll container
// (App.vue merges `h-full overflow-auto` onto it). Defaults to the
// global re-sync.
const rootEl = ref<HTMLElement | null>(null);
const { colorFilterStyle } = useColorFilter();
const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
// Cache-first: the group detail (with members + devices) is keyed on the
// uuid, so a revisit renders instantly from cache then refreshes silently
// (SWR). Group details aren't a synced aggregate, so this is the single
// source of truth (pattern 2).
const groupQuery = useQuery({
  key: () => ['group', (route.params.uuid as string) ?? ''],
  query: () => groupService.getGroupDetails(route.params.uuid as string),
  enabled: () => !!route.params.uuid,
});
const group = computed<GroupDetails | null>(() => groupQuery.data.value ?? null);
const loadOp = computed(() => ({
  isPending: groupQuery.asyncStatus.value === 'loading',
  isError: groupQuery.state.value.status === 'error',
  error: groupQuery.error.value,
}));

// Navigate to device detail
const navigateToAsset = (deviceId: number) => {
  router.push(`/assets/${deviceId}`);
};

// Navigate to user profile
const navigateToUser = (userUuid: string) => {
  router.push(`/users/${userUuid}`);
};

// Navigate to group configuration (admin only)
const navigateToConfiguration = () => {
  const uuid = route.params.uuid as string;
  router.push(`/admin/groups/${uuid}/configure`);
};

// Get sync source display text
const syncSourceDisplay = computed(() => {
  if (!group.value?.external_source) return null;
  switch (group.value.external_source) {
    case 'microsoft':
      return t('group-detail-sync-source-microsoft');
    default:
      return group.value.external_source;
  }
});

// Get group type display
const groupTypeDisplay = computed(() => {
  if (!group.value) return null;
  const types: string[] = [];
  if (group.value.security_enabled) types.push(t('group-detail-type-security'));
  if (group.value.mail_enabled) types.push(t('group-detail-type-mail-enabled'));
  if (group.value.group_type) types.push(group.value.group_type);
  return types.length > 0 ? types.join(', ') : t('group-detail-type-standard');
});

</script>

<template>
  <div ref="rootEl" class="flex-1">
    <PullToRefresh :target="rootEl" />
    <AsyncBoundary :op="loadOp" :has-data="!!group">
      <template #pending>
        <div class="flex items-center justify-center py-16"><Spinner size="md" /></div>
      </template>
      <template #error>
        <div class="p-4 sm:p-6">
          <div class="bg-status-error/10 border border-status-error/30 rounded-lg p-4 text-status-error text-sm">
            {{ $t('group-detail-error-load') }}
          </div>
        </div>
      </template>

    <!-- Group Details -->
    <div v-if="group" class="flex flex-col">
      <!-- Navigation bar -->
      <div class="pt-4 px-4 sm:px-6 flex flex-row justify-between items-center gap-3 sm:gap-4">
        <div class="flex items-center gap-3 sm:gap-4">
          <BackButton fallbackRoute="/admin/groups" />

          <!-- Sync indicator -->
          <div v-if="group.external_source" class="hidden sm:flex items-center gap-2 text-sm">
            <div class="w-2 h-2 rounded-full bg-accent animate-pulse"></div>
            <span class="text-secondary">{{ $t('group-detail-synced-from', { source: syncSourceDisplay ?? '' }) }}</span>
          </div>
        </div>

        <!-- Admin Configure Button -->
        <button
          v-if="authStore.isAdmin"
          @click="navigateToConfiguration"
          class="px-3 py-1.5 bg-surface-alt hover:bg-surface-hover border border-default rounded-lg text-sm font-medium text-primary transition-colors flex items-center gap-1.5"
        >
          <Icon name="settings" />
          {{ $t('group-detail-action-configure') }}
        </button>
      </div>

      <!-- Main Content -->
      <div class="flex flex-col gap-4 sm:gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-7xl">
        <!-- Group Header -->
        <div class="flex items-start sm:items-center gap-3 sm:gap-4">
          <div
            class="w-10 h-10 sm:w-12 sm:h-12 rounded-lg flex items-center justify-center text-white text-lg sm:text-xl font-semibold flex-shrink-0 shadow-sm"
            :style="{ backgroundColor: group.color || '#6366f1', ...colorFilterStyle }"
          >
            {{ group.name.charAt(0).toUpperCase() }}
          </div>
          <div class="min-w-0 flex-1">
            <h1 class="text-xl sm:text-2xl font-semibold text-primary truncate">{{ group.name }}</h1>
            <p v-if="group.description" class="text-secondary text-sm mt-0.5 sm:mt-1 line-clamp-2">{{ group.description }}</p>
            <!-- Mobile sync indicator -->
            <div v-if="group.external_source" class="sm:hidden flex items-center gap-2 text-xs mt-2">
              <div class="w-1.5 h-1.5 rounded-full bg-accent"></div>
              <span class="text-secondary">{{ $t('group-detail-synced-from', { source: syncSourceDisplay ?? '' }) }}</span>
            </div>
          </div>
        </div>

        <!-- Info Cards Grid -->
        <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 sm:gap-6">
          <!-- Group Information -->
          <SectionCard content-padding="p-3 sm:p-4">
            <template #title>{{ $t('group-detail-section-information') }}</template>

            <div class="flex flex-col gap-3 sm:gap-4">
              <!-- Type -->
              <div class="flex flex-col gap-1">
                <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('group-detail-field-type') }}</h3>
                <p class="text-primary text-sm">{{ groupTypeDisplay }}</p>
              </div>

              <!-- Sync Source -->
              <div v-if="syncSourceDisplay" class="flex flex-col gap-1">
                <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('group-detail-field-sync-source') }}</h3>
                <p class="text-primary text-sm">{{ syncSourceDisplay }}</p>
              </div>

              <!-- Last Synced -->
              <div v-if="group.last_synced_at" class="flex flex-col gap-1">
                <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('group-detail-field-last-synced') }}</h3>
                <p class="text-primary text-sm">{{ formatDate(group.last_synced_at, 'MMM d, yyyy h:mm a') }}</p>
              </div>

              <!-- Created/Updated -->
              <div class="grid grid-cols-2 gap-3 sm:gap-4 pt-2 sm:pt-3 border-t border-default">
                <div class="flex flex-col gap-1">
                  <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('group-detail-field-created') }}</h4>
                  <p class="text-primary text-sm">{{ formatDate(group.created_at, 'MMM d, yyyy') }}</p>
                </div>
                <div class="flex flex-col gap-1">
                  <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('group-detail-field-updated') }}</h4>
                  <p class="text-primary text-sm">{{ formatDate(group.updated_at, 'MMM d, yyyy') }}</p>
                </div>
              </div>
            </div>
          </SectionCard>

          <!-- Members -->
          <SectionCard content-padding="p-0">
            <template #title>
              <div class="flex items-center justify-between">
                <span>{{ $t('group-detail-section-members') }}</span>
                <span class="px-2 py-0.5 text-xs bg-surface rounded-full text-secondary font-normal">
                  {{ group.members.length }}
                </span>
              </div>
            </template>

            <div v-if="group.members.length > 0" class="divide-y divide-default max-h-80 overflow-y-auto">
              <div
                v-for="member in group.members"
                :key="member.uuid"
                @click="navigateToUser(member.uuid)"
                class="p-2.5 sm:p-3 hover:bg-surface-hover cursor-pointer transition-colors group/item"
              >
                <div class="flex items-center gap-2.5 sm:gap-3">
                  <UserAvatar
                    :uuid="member.uuid"
                    :fallbackName="member.name"
                    :fallbackAvatar="member.avatar_thumb || member.avatar_url"
                    size="sm"
                    :clickable="false"
                    :show-name="false"
                    class="flex-shrink-0"
                  />
                  <span class="text-sm font-medium text-primary truncate group-hover/item:text-accent transition-colors">
                    {{ member.name }}
                  </span>
                  <Icon name="chevronRight" class="text-tertiary ml-auto opacity-0 group-hover/item:opacity-100 transition-opacity flex-shrink-0" />
                </div>
              </div>
            </div>

            <div v-else class="p-4 text-center">
              <p class="text-tertiary text-sm">{{ $t('group-detail-no-members') }}</p>
            </div>
          </SectionCard>

          <!-- Devices -->
          <SectionCard content-padding="p-0">
            <template #title>
              <div class="flex items-center justify-between">
                <span>{{ $t('group-detail-section-devices') }}</span>
                <span class="px-2 py-0.5 text-xs bg-surface rounded-full text-secondary font-normal">
                  {{ group.devices.length }}
                </span>
              </div>
            </template>

            <div v-if="group.devices.length > 0" class="divide-y divide-default max-h-80 overflow-y-auto">
              <div
                v-for="device in group.devices"
                :key="device.id"
                @click="navigateToAsset(device.id)"
                class="p-2.5 sm:p-3 hover:bg-surface-hover cursor-pointer transition-colors group/item"
              >
                <div class="flex items-center gap-2.5 sm:gap-3">
                  <div class="flex-shrink-0 w-8 h-8 bg-surface-alt rounded-lg inline-flex items-center justify-center group-hover/item:bg-accent/10 transition-colors">
                    <Icon name="device" class="shrink-0 text-secondary group-hover/item:text-accent transition-colors" />
                  </div>
                  <div class="min-w-0 flex-1">
                    <p class="text-sm font-medium text-primary truncate group-hover/item:text-accent transition-colors">
                      {{ device.name || (device.attributes?.hostname as string | undefined) }}
                    </p>
                    <p class="text-xs text-secondary truncate">
                      {{ [device.manufacturer, device.model].filter(Boolean).join(' ') || $t('group-detail-unknown-device') }}
                    </p>
                  </div>
                  <Icon name="chevronRight" class="text-tertiary ml-auto opacity-0 group-hover/item:opacity-100 transition-opacity flex-shrink-0" />
                </div>
              </div>
            </div>

            <div v-else class="p-4 text-center">
              <p class="text-tertiary text-sm">{{ $t('group-detail-no-devices') }}</p>
            </div>
          </SectionCard>
        </div>
      </div>
    </div>

    <!-- Not Found -->
    <div v-else class="p-4 sm:p-6 text-center">
      <div class="w-12 h-12 bg-surface-alt rounded-full inline-flex items-center justify-center mx-auto mb-4">
        <svg class="w-6 h-6 shrink-0 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.172 16.172a4 4 0 015.656 0M9 10h.01M15 10h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
      </div>
      <p class="text-secondary">{{ $t('group-detail-not-found') }}</p>
    </div>
    </AsyncBoundary>
  </div>
</template>
