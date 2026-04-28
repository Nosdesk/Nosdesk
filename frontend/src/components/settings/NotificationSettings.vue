<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { useAuthStore } from '@/stores/auth';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import Icon from '@/components/common/Icon.vue';
import {
  getNotificationPreferences,
  updateNotificationPreference,
  NOTIFICATION_TYPES,
  NOTIFICATION_CHANNELS,
  type NotificationPreference,
} from '@/services/notificationService';
import { requestNotificationPermission } from '@/composables/useNotificationSSE';

const props = defineProps<{
  targetUserUuid?: string;
}>();

const authStore = useAuthStore();

const isManagingOtherUser = computed(() => {
  return !!props.targetUserUuid && props.targetUserUuid !== authStore.user?.uuid;
});

// Loading state
const isLoading = ref(true);
const isSaving = ref<string | null>(null);

// Notification preferences from API
const preferences = ref<NotificationPreference[]>([]);

// Browser notification permission status
const browserPermission = ref<NotificationPermission>('default');

// Emits for notifications
const emit = defineEmits<{
  (e: 'success', message: string): void;
  (e: 'error', message: string): void;
}>();

// Group notification types by category
const groupedNotificationTypes = computed(() => {
  const groups: Record<string, typeof NOTIFICATION_TYPES[number][]> = {};
  for (const type of NOTIFICATION_TYPES) {
    if (!groups[type.category]) {
      groups[type.category] = [];
    }
    groups[type.category].push(type);
  }
  return groups;
});

// Category metadata
const categoryMeta: Record<string, { label: string; description: string; icon: string }> = {
  ticket: {
    label: 'Tickets',
    description: 'Notifications about ticket assignments and status changes',
    icon: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" />',
  },
  comment: {
    label: 'Comments',
    description: 'Notifications when someone comments on your tickets',
    icon: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />',
  },
  mention: {
    label: 'Mentions',
    description: 'Notifications when someone mentions you',
    icon: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 12a4 4 0 10-8 0 4 4 0 008 0zm0 0v1.5a2.5 2.5 0 005 0V12a9 9 0 10-9 9" />',
  },
  documentation: {
    label: 'Documentation',
    description: 'Notifications about documentation page updates',
    icon: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />',
  },
};

// Get preference value for a specific type/channel combination
const getPreference = (typeCode: string, channelCode: string): boolean => {
  const pref = preferences.value.find(
    (p) => p.notification_type === typeCode && p.channel === channelCode
  );
  // Default to true if no preference exists
  return pref?.enabled ?? true;
};

// Check if all preferences for a channel are enabled
const isChannelFullyEnabled = (channelCode: string): boolean => {
  return NOTIFICATION_TYPES.every((type) => getPreference(type.code, channelCode));
};

// Toggle a specific preference
const togglePreference = async (typeCode: string, channelCode: string) => {
  const currentValue = getPreference(typeCode, channelCode);
  const newValue = !currentValue;
  const key = `${typeCode}-${channelCode}`;

  isSaving.value = key;

  try {
    await updateNotificationPreference(typeCode, channelCode, newValue);

    // Update local state
    const existingIndex = preferences.value.findIndex(
      (p) => p.notification_type === typeCode && p.channel === channelCode
    );

    if (existingIndex >= 0) {
      preferences.value[existingIndex].enabled = newValue;
    } else {
      preferences.value.push({
        notification_type: typeCode,
        channel: channelCode,
        enabled: newValue,
      });
    }

    emit('success', `Preference updated`);
  } catch {
    emit('error', 'Failed to update preference');
  } finally {
    isSaving.value = null;
  }
};

// Toggle all preferences for a channel
const toggleAllForChannel = async (channelCode: string) => {
  const currentlyEnabled = isChannelFullyEnabled(channelCode);
  const newValue = !currentlyEnabled;

  for (const type of NOTIFICATION_TYPES) {
    const key = `${type.code}-${channelCode}`;
    isSaving.value = key;

    try {
      await updateNotificationPreference(type.code, channelCode, newValue);

      // Update local state
      const existingIndex = preferences.value.findIndex(
        (p) => p.notification_type === type.code && p.channel === channelCode
      );

      if (existingIndex >= 0) {
        preferences.value[existingIndex].enabled = newValue;
      } else {
        preferences.value.push({
          notification_type: type.code,
          channel: channelCode,
          enabled: newValue,
        });
      }
    } catch {
      // Continue with other preferences even if one fails
    }
  }

  isSaving.value = null;
  emit('success', `All ${channelCode === 'in_app' ? 'in-app' : channelCode} notifications ${newValue ? 'enabled' : 'disabled'}`);
};

// Request browser notification permission
const requestBrowserPermission = async () => {
  const granted = await requestNotificationPermission();
  browserPermission.value = Notification.permission;

  if (granted) {
    emit('success', 'Browser notifications enabled');
  } else {
    emit('error', 'Browser notification permission denied');
  }
};

// Load preferences on mount
onMounted(async () => {
  try {
    preferences.value = await getNotificationPreferences();

    // Check browser permission status
    if ('Notification' in window) {
      browserPermission.value = Notification.permission;
    }
  } catch {
    emit('error', 'Failed to load notification preferences');
  } finally {
    isLoading.value = false;
  }
});
</script>

<template>
  <div class="flex flex-col gap-4">
    <!-- Loading State -->
    <div v-if="isLoading" class="bg-surface rounded-xl border border-default overflow-hidden">
      <div class="px-4 py-3 bg-surface-alt border-b border-default flex flex-col gap-2">
        <div class="h-5 w-32 bg-surface-hover rounded animate-pulse"></div>
        <div class="h-4 w-56 bg-surface-hover rounded animate-pulse"></div>
      </div>
      <div class="p-4 flex flex-col gap-3">
        <div v-for="i in 3" :key="i" class="h-12 bg-surface-alt rounded-lg animate-pulse"></div>
      </div>
    </div>

    <template v-else>
      <!-- Browser Notification Permission Banner (only for own profile) -->
      <div
        v-if="!isManagingOtherUser && browserPermission !== 'granted'"
        class="bg-surface rounded-xl border border-accent/30 overflow-hidden"
      >
        <div class="p-4">
          <div class="flex items-start gap-3">
            <div class="w-9 h-9 bg-accent/15 rounded-lg flex items-center justify-center flex-shrink-0">
              <span class="text-accent inline-flex">
                <Icon name="bell" />
              </span>
            </div>
            <div class="flex-1 min-w-0 flex flex-col gap-2">
              <div class="flex flex-col gap-1">
                <h3 class="text-sm font-medium text-primary">Enable Browser Notifications</h3>
                <p class="text-xs text-secondary">
                  Allow browser notifications to receive alerts even when the app isn't in focus.
                </p>
              </div>
              <div>
                <button
                  @click="requestBrowserPermission"
                  class="px-3 py-1.5 text-sm font-medium text-white bg-accent hover:opacity-90 rounded-lg transition-colors"
                >
                  Enable Notifications
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Quick Settings Card -->
      <div class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden">
        <div class="px-4 py-3 bg-surface-alt border-b border-default">
          <div class="flex items-center gap-3">
            <div class="w-8 h-8 bg-accent/15 rounded-lg flex items-center justify-center flex-shrink-0">
              <span class="text-accent inline-flex">
                <Icon name="settings" />
              </span>
            </div>
            <div>
              <h2 class="text-base sm:text-lg font-semibold text-primary">Quick Settings</h2>
              <p class="text-xs text-secondary hidden sm:block">Enable or disable all notifications per channel</p>
            </div>
          </div>
        </div>

        <div class="p-4 flex flex-col gap-2">
          <div
            v-for="channel in NOTIFICATION_CHANNELS"
            :key="channel.code"
            class="bg-surface-alt rounded-lg border border-subtle px-3 py-2"
          >
            <ToggleSwitch
              :model-value="isChannelFullyEnabled(channel.code)"
              :label="`All ${channel.name} Notifications`"
              :description="channel.description"
              @update:model-value="toggleAllForChannel(channel.code)"
            />
          </div>
        </div>
      </div>

      <!-- Category Cards -->
      <div
        v-for="(types, category) in groupedNotificationTypes"
        :key="category"
        class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden"
      >
        <div class="px-4 py-3 bg-surface-alt border-b border-default">
          <div class="flex items-center gap-3">
            <div class="w-8 h-8 bg-accent/15 rounded-lg flex items-center justify-center flex-shrink-0">
              <svg class="w-4 h-4 text-accent" fill="none" viewBox="0 0 24 24" stroke="currentColor" v-html="categoryMeta[category]?.icon || ''"></svg>
            </div>
            <div>
              <h2 class="text-base sm:text-lg font-semibold text-primary">
                {{ categoryMeta[category]?.label || category }}
              </h2>
              <p class="text-xs text-secondary hidden sm:block">
                {{ categoryMeta[category]?.description || '' }}
              </p>
            </div>
          </div>
        </div>

        <div class="p-4">
          <!-- Desktop: table-like grid layout -->
          <div class="hidden sm:flex sm:flex-col sm:gap-3">
            <!-- Column headers -->
            <div class="grid items-center gap-4 px-3" :style="{ gridTemplateColumns: `1fr repeat(${NOTIFICATION_CHANNELS.length}, 5rem)` }">
              <div class="text-xs font-medium text-tertiary uppercase tracking-wide">Notification</div>
              <div
                v-for="channel in NOTIFICATION_CHANNELS"
                :key="`header-${channel.code}`"
                class="text-xs font-medium text-tertiary uppercase tracking-wide text-center"
              >
                {{ channel.name }}
              </div>
            </div>

            <!-- Notification type rows -->
            <div class="flex flex-col gap-1.5">
              <div
                v-for="type in types"
                :key="type.code"
                class="grid items-center gap-4 bg-surface-alt rounded-lg border border-subtle px-3 py-2.5"
                :style="{ gridTemplateColumns: `1fr repeat(${NOTIFICATION_CHANNELS.length}, 5rem)` }"
              >
                <div class="flex flex-col gap-0.5 min-w-0">
                  <p class="text-sm font-medium text-primary">{{ type.name }}</p>
                  <p class="text-xs text-tertiary truncate">{{ type.description }}</p>
                </div>
                <div
                  v-for="channel in NOTIFICATION_CHANNELS"
                  :key="`${type.code}-${channel.code}`"
                  class="flex justify-center"
                >
                  <ToggleSwitch
                    :model-value="getPreference(type.code, channel.code)"
                    :disabled="isSaving === `${type.code}-${channel.code}`"
                    size="sm"
                    @update:model-value="togglePreference(type.code, channel.code)"
                  />
                </div>
              </div>
            </div>
          </div>

          <!-- Mobile: stacked layout -->
          <div class="sm:hidden flex flex-col gap-2">
            <div
              v-for="type in types"
              :key="type.code"
              class="bg-surface-alt rounded-lg border border-subtle px-3 py-2.5 flex flex-col gap-2"
            >
              <div class="flex flex-col gap-0.5">
                <p class="text-sm font-medium text-primary">{{ type.name }}</p>
                <p class="text-xs text-tertiary">{{ type.description }}</p>
              </div>
              <div class="flex flex-col gap-1.5 pl-1">
                <div
                  v-for="channel in NOTIFICATION_CHANNELS"
                  :key="`${type.code}-${channel.code}-mobile`"
                >
                  <ToggleSwitch
                    :model-value="getPreference(type.code, channel.code)"
                    :disabled="isSaving === `${type.code}-${channel.code}`"
                    :label="channel.name"
                    size="sm"
                    @update:model-value="togglePreference(type.code, channel.code)"
                  />
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Info Footer -->
      <div class="bg-surface rounded-xl border border-default overflow-hidden">
        <div class="px-4 py-3">
          <div class="flex items-center gap-3">
            <span class="text-tertiary flex-shrink-0 inline-flex">
              <Icon name="info" />
            </span>
            <p class="text-xs text-secondary">
              Email notifications are rate limited to prevent inbox flooding. You'll receive at most
              one email per ticket every 5 minutes.
            </p>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
