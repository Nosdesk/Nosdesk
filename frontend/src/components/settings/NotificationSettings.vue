<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { useFluent } from 'fluent-vue';
import { useAuthStore } from '@/stores/auth';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import Icon from '@/components/common/Icon.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import {
  getNotificationPreferences,
  updateNotificationPreference,
  NOTIFICATION_TYPES,
  NOTIFICATION_CHANNELS,
  type NotificationPreference,
} from '@/services/notificationService';
import { requestNotificationPermission } from '@/composables/useNotificationSSE';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

// Map backend codes to localization key suffixes
const TYPE_KEY_SUFFIX: Record<string, string> = {
  ticket_assigned: 'ticket-assigned',
  ticket_status_changed: 'ticket-status-changed',
  comment_added: 'comment-added',
  mentioned: 'mentioned',
  ticket_created_requester: 'ticket-created-requester',
  doc_page_updated: 'doc-page-updated',
  asset_low_stock: 'asset-low-stock',
};

const CHANNEL_KEY_SUFFIX: Record<string, string> = {
  in_app: 'in-app',
  email: 'email',
};

// Localized notification types
const localizedTypes = computed(() =>
  NOTIFICATION_TYPES.map((type) => {
    const suffix = TYPE_KEY_SUFFIX[type.code] ?? type.code;
    return {
      ...type,
      name: t(`settings-notifications-type-${suffix}-name`),
      description: t(`settings-notifications-type-${suffix}-description`),
    };
  })
);

// Localized notification channels
const localizedChannels = computed(() =>
  NOTIFICATION_CHANNELS.map((channel) => {
    const suffix = CHANNEL_KEY_SUFFIX[channel.code] ?? channel.code;
    return {
      ...channel,
      name: t(`settings-notifications-channel-${suffix}-name`),
      description: t(`settings-notifications-channel-${suffix}-description`),
    };
  })
);

// Localized category metadata (icons stay as raw SVG path strings).
const categoryMeta = computed<Record<string, { label: string; description: string; icon: string }>>(() => ({
  ticket: {
    label: t('settings-notifications-category-ticket-label'),
    description: t('settings-notifications-category-ticket-description'),
    icon: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" />',
  },
  comment: {
    label: t('settings-notifications-category-comment-label'),
    description: t('settings-notifications-category-comment-description'),
    icon: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />',
  },
  mention: {
    label: t('settings-notifications-category-mention-label'),
    description: t('settings-notifications-category-mention-description'),
    icon: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 12a4 4 0 10-8 0 4 4 0 008 0zm0 0v1.5a2.5 2.5 0 005 0V12a9 9 0 10-9 9" />',
  },
  documentation: {
    label: t('settings-notifications-category-documentation-label'),
    description: t('settings-notifications-category-documentation-description'),
    icon: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />',
  },
}));

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

// Group localized notification types by category so labels react to locale changes.
const groupedNotificationTypes = computed(() => {
  const groups: Record<string, typeof localizedTypes.value[number][]> = {};
  for (const type of localizedTypes.value) {
    if (!groups[type.category]) {
      groups[type.category] = [];
    }
    groups[type.category].push(type);
  }
  return groups;
});

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

    emit('success', t('settings-notifications-preference-update-success'));
  } catch {
    emit('error', t('settings-notifications-preference-update-error'));
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
  const channelLabel =
    localizedChannels.value.find((c) => c.code === channelCode)?.name ?? channelCode;
  emit(
    'success',
    t('settings-notifications-channel-bulk-success', {
      channel: channelLabel,
      state: newValue ? 'enabled' : 'disabled',
    }),
  );
};

// Request browser notification permission
const requestBrowserPermission = async () => {
  const granted = await requestNotificationPermission();
  browserPermission.value = Notification.permission;

  if (granted) {
    emit('success', t('settings-notifications-browser-enabled-success'));
  } else {
    emit('error', t('settings-notifications-browser-denied-error'));
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
    emit('error', t('settings-notifications-load-error'));
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
                <h3 class="text-sm font-medium text-primary">{{ $t('settings-notifications-browser-banner-title') }}</h3>
                <p class="text-xs text-secondary">
                  {{ $t('settings-notifications-browser-banner-description') }}
                </p>
              </div>
              <div>
                <button
                  @click="requestBrowserPermission"
                  class="px-3 py-1.5 text-sm font-medium text-white bg-accent hover:opacity-90 rounded-lg transition-colors"
                >
                  {{ $t('settings-notifications-browser-banner-enable') }}
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Quick Settings Card -->
      <SectionCard content-padding="p-4">
        <template #leading>
          <span class="text-accent inline-flex">
            <Icon name="settings" />
          </span>
        </template>
        <template #title>{{ $t('settings-notifications-quick-settings-title') }}</template>

        <div class="flex flex-col gap-2">
          <div
            v-for="channel in localizedChannels"
            :key="channel.code"
            class="bg-surface-alt rounded-lg border border-subtle px-3 py-2"
          >
            <ToggleSwitch
              :model-value="isChannelFullyEnabled(channel.code)"
              :label="$t('settings-notifications-channel-toggle-all-label', { channel: channel.name })"
              :description="channel.description"
              @update:model-value="toggleAllForChannel(channel.code)"
            />
          </div>
        </div>
      </SectionCard>

      <!-- Category Cards -->
      <SectionCard
        v-for="(types, category) in groupedNotificationTypes"
        :key="category"
        content-padding="p-4"
      >
        <template #leading>
          <svg class="w-4 h-4 text-accent flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" v-html="categoryMeta[category]?.icon || ''"></svg>
        </template>
        <template #title>{{ categoryMeta[category]?.label || category }}</template>

        <div>
          <!-- Desktop: table-like grid layout -->
          <div class="hidden sm:flex sm:flex-col sm:gap-3">
            <!-- Column headers -->
            <div class="grid items-center gap-4 px-3" :style="{ gridTemplateColumns: `1fr repeat(${localizedChannels.length}, 5rem)` }">
              <div class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ $t('settings-notifications-column-header') }}</div>
              <div
                v-for="channel in localizedChannels"
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
                :style="{ gridTemplateColumns: `1fr repeat(${localizedChannels.length}, 5rem)` }"
              >
                <div class="flex flex-col gap-0.5 min-w-0">
                  <p class="text-sm font-medium text-primary">{{ type.name }}</p>
                  <p class="text-xs text-tertiary truncate">{{ type.description }}</p>
                </div>
                <div
                  v-for="channel in localizedChannels"
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
                  v-for="channel in localizedChannels"
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
      </SectionCard>

      <!-- Info Footer -->
      <div class="bg-surface rounded-xl border border-default overflow-hidden">
        <div class="px-4 py-3">
          <div class="flex items-center gap-3">
            <span class="text-tertiary flex-shrink-0 inline-flex">
              <Icon name="info" />
            </span>
            <p class="text-xs text-secondary">
              {{ $t('settings-notifications-info-footer') }}
            </p>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
