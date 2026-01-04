<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import BackButton from '@/components/common/BackButton.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import { groupService } from '@/services/groupService';
import { formatDate } from '@/utils/dateUtils';
import { useAuthStore } from '@/stores/auth';
import { useColorFilter } from '@/composables/useColorFilter';
import type { GroupDetails } from '@/types/group';

const route = useRoute();
const router = useRouter();
const authStore = useAuthStore();
const { colorFilterStyle } = useColorFilter();
const group = ref<GroupDetails | null>(null);
const loading = ref(true);
const error = ref<string | null>(null);

const fetchGroupData = async () => {
  try {
    loading.value = true;
    error.value = null;

    const uuid = route.params.uuid as string;
    if (!uuid) {
      error.value = 'Invalid group ID';
      loading.value = false;
      return;
    }

    group.value = await groupService.getGroupDetails(uuid);
  } catch (e) {
    error.value = 'Failed to load group details';
    console.error('Error loading group:', e);
  } finally {
    loading.value = false;
  }
};

// Navigate to device detail
const navigateToDevice = (deviceId: number) => {
  router.push(`/devices/${deviceId}`);
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
      return 'Microsoft Entra ID';
    default:
      return group.value.external_source;
  }
});

// Get group type display
const groupTypeDisplay = computed(() => {
  if (!group.value) return null;
  const types = [];
  if (group.value.security_enabled) types.push('Security');
  if (group.value.mail_enabled) types.push('Mail-enabled');
  if (group.value.group_type) types.push(group.value.group_type);
  return types.length > 0 ? types.join(', ') : 'Standard';
});

onMounted(() => {
  fetchGroupData();
});
</script>

<template>
  <div class="flex-1">
    <!-- Loading State with Skeleton -->
    <div v-if="loading" class="flex flex-col">
      <!-- Skeleton Navigation bar -->
      <div class="pt-4 px-4 sm:px-6 flex items-center gap-4">
        <div class="h-8 w-24 bg-surface-alt rounded-lg animate-pulse"></div>
      </div>

      <!-- Skeleton Main Content -->
      <div class="flex flex-col gap-4 sm:gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-7xl">
        <!-- Skeleton Header -->
        <div class="flex items-center gap-3 sm:gap-4">
          <div class="w-10 h-10 sm:w-12 sm:h-12 rounded-lg bg-surface-alt animate-pulse"></div>
          <div class="flex-1">
            <div class="h-6 sm:h-7 w-48 bg-surface-alt rounded animate-pulse"></div>
            <div class="h-4 w-64 bg-surface-alt rounded animate-pulse mt-2"></div>
          </div>
        </div>

        <!-- Skeleton Cards Grid -->
        <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 sm:gap-6">
          <!-- Skeleton Card 1 -->
          <div class="bg-surface border border-default rounded-xl overflow-hidden">
            <div class="px-4 py-3 bg-surface-alt border-b border-default">
              <div class="h-5 w-32 bg-surface rounded animate-pulse"></div>
            </div>
            <div class="p-4 space-y-4">
              <div class="space-y-2">
                <div class="h-3 w-16 bg-surface-alt rounded animate-pulse"></div>
                <div class="h-4 w-24 bg-surface-alt rounded animate-pulse"></div>
              </div>
              <div class="space-y-2">
                <div class="h-3 w-20 bg-surface-alt rounded animate-pulse"></div>
                <div class="h-4 w-32 bg-surface-alt rounded animate-pulse"></div>
              </div>
            </div>
          </div>

          <!-- Skeleton Card 2 -->
          <div class="bg-surface border border-default rounded-xl overflow-hidden">
            <div class="px-4 py-3 bg-surface-alt border-b border-default">
              <div class="h-5 w-24 bg-surface rounded animate-pulse"></div>
            </div>
            <div class="divide-y divide-default">
              <div v-for="i in 3" :key="i" class="p-3 flex items-center gap-3">
                <div class="w-8 h-8 rounded-full bg-surface-alt animate-pulse"></div>
                <div class="h-4 w-28 bg-surface-alt rounded animate-pulse"></div>
              </div>
            </div>
          </div>

          <!-- Skeleton Card 3 -->
          <div class="bg-surface border border-default rounded-xl overflow-hidden">
            <div class="px-4 py-3 bg-surface-alt border-b border-default">
              <div class="h-5 w-20 bg-surface rounded animate-pulse"></div>
            </div>
            <div class="divide-y divide-default">
              <div v-for="i in 3" :key="i" class="p-3 flex items-center gap-3">
                <div class="w-8 h-8 rounded-lg bg-surface-alt animate-pulse"></div>
                <div class="flex-1">
                  <div class="h-4 w-32 bg-surface-alt rounded animate-pulse"></div>
                  <div class="h-3 w-24 bg-surface-alt rounded animate-pulse mt-1"></div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Error State -->
    <div v-else-if="error" class="p-4 sm:p-6">
      <div class="bg-status-error/10 border border-status-error/30 rounded-lg p-4 text-status-error text-sm">
        {{ error }}
      </div>
    </div>

    <!-- Group Details -->
    <div v-else-if="group" class="flex flex-col">
      <!-- Navigation bar -->
      <div class="pt-4 px-4 sm:px-6 flex flex-col sm:flex-row justify-between items-start sm:items-center gap-3 sm:gap-4">
        <div class="flex items-center gap-3 sm:gap-4">
          <BackButton fallbackRoute="/admin/groups" label="Go back" />

          <!-- Sync indicator -->
          <div v-if="group.external_source" class="hidden sm:flex items-center gap-2 text-sm">
            <div class="w-2 h-2 rounded-full bg-accent animate-pulse"></div>
            <span class="text-secondary">Synced from {{ syncSourceDisplay }}</span>
          </div>
        </div>

        <!-- Admin Configure Button -->
        <button
          v-if="authStore.isAdmin"
          @click="navigateToConfiguration"
          class="px-3 py-1.5 bg-surface-alt hover:bg-surface-hover border border-default rounded-lg text-sm font-medium text-primary transition-colors flex items-center gap-1.5"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
            <path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
          </svg>
          Configure
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
              <span class="text-secondary">Synced from {{ syncSourceDisplay }}</span>
            </div>
          </div>
        </div>

        <!-- Info Cards Grid -->
        <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 sm:gap-6">
          <!-- Group Information -->
          <SectionCard content-padding="p-3 sm:p-4">
            <template #title>Group Information</template>

            <div class="flex flex-col gap-3 sm:gap-4">
              <!-- Type -->
              <div class="flex flex-col gap-1">
                <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Type</h3>
                <p class="text-primary text-sm">{{ groupTypeDisplay }}</p>
              </div>

              <!-- Sync Source -->
              <div v-if="syncSourceDisplay" class="flex flex-col gap-1">
                <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Sync Source</h3>
                <p class="text-primary text-sm">{{ syncSourceDisplay }}</p>
              </div>

              <!-- Last Synced -->
              <div v-if="group.last_synced_at" class="flex flex-col gap-1">
                <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Last Synced</h3>
                <p class="text-primary text-sm">{{ formatDate(group.last_synced_at, 'MMM d, yyyy h:mm a') }}</p>
              </div>

              <!-- Created/Updated -->
              <div class="grid grid-cols-2 gap-3 sm:gap-4 pt-2 sm:pt-3 border-t border-default">
                <div class="flex flex-col gap-1">
                  <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">Created</h4>
                  <p class="text-primary text-sm">{{ formatDate(group.created_at, 'MMM d, yyyy') }}</p>
                </div>
                <div class="flex flex-col gap-1">
                  <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">Updated</h4>
                  <p class="text-primary text-sm">{{ formatDate(group.updated_at, 'MMM d, yyyy') }}</p>
                </div>
              </div>
            </div>
          </SectionCard>

          <!-- Members -->
          <SectionCard content-padding="p-0">
            <template #title>
              <div class="flex items-center justify-between">
                <span>Members</span>
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
                    :name="member.uuid"
                    :userName="member.name"
                    :avatar="member.avatar_thumb || member.avatar_url"
                    size="sm"
                    :clickable="false"
                    :show-name="false"
                    class="flex-shrink-0"
                  />
                  <span class="text-sm font-medium text-primary truncate group-hover/item:text-accent transition-colors">
                    {{ member.name }}
                  </span>
                  <svg class="w-4 h-4 text-tertiary ml-auto opacity-0 group-hover/item:opacity-100 transition-opacity flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
                  </svg>
                </div>
              </div>
            </div>

            <div v-else class="p-6 sm:p-8 text-center">
              <div class="w-10 h-10 sm:w-12 sm:h-12 bg-surface-alt rounded-full inline-flex items-center justify-center mx-auto mb-3">
                <svg class="w-5 h-5 sm:w-6 sm:h-6 shrink-0 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0z" />
                </svg>
              </div>
              <p class="text-secondary text-sm">No members in this group</p>
            </div>
          </SectionCard>

          <!-- Devices -->
          <SectionCard content-padding="p-0">
            <template #title>
              <div class="flex items-center justify-between">
                <span>Devices</span>
                <span class="px-2 py-0.5 text-xs bg-surface rounded-full text-secondary font-normal">
                  {{ group.devices.length }}
                </span>
              </div>
            </template>

            <div v-if="group.devices.length > 0" class="divide-y divide-default max-h-80 overflow-y-auto">
              <div
                v-for="device in group.devices"
                :key="device.id"
                @click="navigateToDevice(device.id)"
                class="p-2.5 sm:p-3 hover:bg-surface-hover cursor-pointer transition-colors group/item"
              >
                <div class="flex items-center gap-2.5 sm:gap-3">
                  <div class="flex-shrink-0 w-8 h-8 bg-surface-alt rounded-lg inline-flex items-center justify-center group-hover/item:bg-accent/10 transition-colors">
                    <svg class="w-4 h-4 shrink-0 text-secondary group-hover/item:text-accent transition-colors" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
                    </svg>
                  </div>
                  <div class="min-w-0 flex-1">
                    <p class="text-sm font-medium text-primary truncate group-hover/item:text-accent transition-colors">
                      {{ device.name || device.hostname }}
                    </p>
                    <p class="text-xs text-secondary truncate">
                      {{ [device.manufacturer, device.model].filter(Boolean).join(' ') || 'Unknown device' }}
                    </p>
                  </div>
                  <svg class="w-4 h-4 text-tertiary ml-auto opacity-0 group-hover/item:opacity-100 transition-opacity flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
                  </svg>
                </div>
              </div>
            </div>

            <div v-else class="p-6 sm:p-8 text-center">
              <div class="w-10 h-10 sm:w-12 sm:h-12 bg-surface-alt rounded-full inline-flex items-center justify-center mx-auto mb-3">
                <svg class="w-5 h-5 sm:w-6 sm:h-6 shrink-0 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
                </svg>
              </div>
              <p class="text-secondary text-sm">No devices in this group</p>
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
      <p class="text-secondary">Group not found</p>
    </div>
  </div>
</template>
