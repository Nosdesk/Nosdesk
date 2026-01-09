<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import {
  getNotificationPreferences,
  updateNotificationPreference,
  NOTIFICATION_TYPES,
  NOTIFICATION_CHANNELS,
  type NotificationPreference,
} from '@/services/notificationService';
import { requestNotificationPermission } from '@/composables/useNotificationSSE';

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

// Category labels
const categoryLabels: Record<string, string> = {
  ticket: 'Ticket Notifications',
  comment: 'Comment Notifications',
  mention: 'Mention Notifications',
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
  } catch (error) {
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
  } catch (error) {
    emit('error', 'Failed to load notification preferences');
  } finally {
    isLoading.value = false;
  }
});
</script>

<template>
  <div class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden">
    <div class="px-4 py-3 bg-surface-alt border-b border-default">
      <h2 class="text-lg font-medium text-primary">Notifications</h2>
      <p class="text-sm text-tertiary mt-1">Configure how you'd like to be notified about updates</p>
    </div>

    <!-- Loading State -->
    <div v-if="isLoading" class="p-6 flex items-center justify-center">
      <div class="animate-spin rounded-full h-6 w-6 border-2 border-accent border-t-transparent"></div>
      <span class="ml-2 text-secondary">Loading preferences...</span>
    </div>

    <div v-else class="p-6 space-y-6">
      <!-- Browser Notification Permission -->
      <div
        v-if="browserPermission !== 'granted'"
        class="p-4 rounded-lg bg-accent/10 border border-accent/30"
      >
        <div class="flex items-start gap-3">
          <svg
            class="w-5 h-5 text-accent flex-shrink-0 mt-0.5"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M15 17h5l-1.405-1.405A2.032 2.032 0 0118 14.158V11a6.002 6.002 0 00-4-5.659V5a2 2 0 10-4 0v.341C7.67 6.165 6 8.388 6 11v3.159c0 .538-.214 1.055-.595 1.436L4 17h5m6 0v1a3 3 0 11-6 0v-1m6 0H9"
            />
          </svg>
          <div class="flex-1">
            <h3 class="text-sm font-medium text-primary">Enable Browser Notifications</h3>
            <p class="text-sm text-secondary mt-1">
              Allow browser notifications to receive alerts even when the app isn't in focus.
            </p>
            <button
              @click="requestBrowserPermission"
              class="mt-3 px-3 py-1.5 text-sm font-medium text-white bg-accent hover:bg-accent-hover rounded-lg transition-colors"
            >
              Enable Notifications
            </button>
          </div>
        </div>
      </div>

      <!-- Channel Quick Toggles -->
      <div class="space-y-3">
        <h3 class="text-sm font-medium text-primary">Quick Settings</h3>
        <div class="flex flex-col gap-3">
          <div
            v-for="channel in NOTIFICATION_CHANNELS"
            :key="channel.code"
            class="p-3 rounded-lg bg-surface-alt border border-default"
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

      <!-- Detailed Preferences by Category -->
      <div
        v-for="(types, category) in groupedNotificationTypes"
        :key="category"
        class="space-y-3"
      >
        <h3 class="text-sm font-medium text-primary">
          {{ categoryLabels[category] || category }}
        </h3>

        <div class="space-y-2">
          <div
            v-for="type in types"
            :key="type.code"
            class="p-4 rounded-lg bg-surface-alt border border-default"
          >
            <div class="flex flex-col gap-3">
              <!-- Type header -->
              <div>
                <p class="text-sm font-medium text-primary">{{ type.name }}</p>
                <p class="text-xs text-tertiary mt-0.5">{{ type.description }}</p>
              </div>

              <!-- Channel toggles -->
              <div class="flex flex-col sm:flex-row sm:items-center gap-3 sm:gap-6 pl-0 sm:pl-2">
                <div
                  v-for="channel in NOTIFICATION_CHANNELS"
                  :key="`${type.code}-${channel.code}`"
                  class="flex items-center gap-3"
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

      <!-- Info about email rate limiting -->
      <div class="p-4 rounded-lg bg-surface-alt border border-default">
        <div class="flex items-start gap-3">
          <svg
            class="w-5 h-5 text-tertiary flex-shrink-0 mt-0.5"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
            />
          </svg>
          <div>
            <p class="text-sm text-secondary">
              Email notifications are rate limited to prevent inbox flooding. You'll receive at most
              one email per ticket every 5 minutes.
            </p>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
