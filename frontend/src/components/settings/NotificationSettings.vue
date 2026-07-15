<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { useFluent } from 'fluent-vue';
import { useAuthStore } from '@/stores/auth';
import Icon from '@/components/common/Icon.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import Button from '@/components/common/Button.vue';
import BaseDropdown, { type DropdownOption } from '@/components/common/BaseDropdown.vue';
import {
  getNotificationPreferences,
  updateNotificationPreference,
  channelSupportsDigest,
  NOTIFICATION_CHANNELS,
  type NotificationPreference,
  type NotificationFrequency,
} from '@nosdesk/core/services/notificationService';
import { requestNotificationPermission } from '@/composables/useNotificationSSE';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

// Fluent returns the key itself when a message is missing; treat that as a
// miss and fall back to the API-provided (English) string so a not-yet-
// translated notification type still renders a sensible label.
const tr = (key: string, fallback: string): string => {
  const value = t(key);
  return value === key ? fallback : value;
};

const keyFragment = (code: string) => code.replace(/_/g, '-');

const props = defineProps<{
  targetUserUuid?: string;
}>();

const authStore = useAuthStore();

const isManagingOtherUser = computed(
  () => !!props.targetUserUuid && props.targetUserUuid !== authStore.user?.uuid
);

const emit = defineEmits<{
  (e: 'success', message: string): void;
  (e: 'error', message: string): void;
}>();

const isLoading = ref(true);
// Key of the cell currently saving (`${type}-${channel}`), or 'channel:<code>'
// while a bulk apply runs — used to disable the affected control(s).
const isSaving = ref<string | null>(null);
const preferences = ref<NotificationPreference[]>([]);
const browserPermission = ref<NotificationPermission>('default');

// Localized channel columns (canonical order + which channels exist come from
// the shared NOTIFICATION_CHANNELS list: in_app, email, push).
const channels = computed(() =>
  NOTIFICATION_CHANNELS.map((channel) => ({
    code: channel.code,
    name: tr(`settings-notifications-channel-${keyFragment(channel.code)}-name`, channel.name),
    description: tr(
      `settings-notifications-channel-${keyFragment(channel.code)}-description`,
      channel.description
    ),
  }))
);

// Per-category icon (raw SVG path); label/description come from i18n.
const CATEGORY_ICON: Record<string, string> = {
  ticket:
    '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" />',
  comment:
    '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />',
  mention:
    '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 12a4 4 0 10-8 0 4 4 0 008 0zm0 0v1.5a2.5 2.5 0 005 0V12a9 9 0 10-9 9" />',
  documentation:
    '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />',
  asset:
    '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M20 7l-8-4-8 4m16 0l-8 4m8-4v10l-8 4m0-10L4 7m8 4v10M4 7v10l8 4" />',
};

// Group the API preference rows by category, preserving first-seen order so
// the layout is stable across locales. Labels react to locale via `tr`.
const groupedPreferences = computed(() => {
  const groups: { category: string; label: string; icon: string; rows: NotificationPreference[] }[] =
    [];
  for (const row of preferences.value) {
    let group = groups.find((g) => g.category === row.category);
    if (!group) {
      group = {
        category: row.category,
        label: tr(`settings-notifications-category-${keyFragment(row.category)}-label`, row.category),
        icon: CATEGORY_ICON[row.category] ?? CATEGORY_ICON.ticket,
        rows: [],
      };
      groups.push(group);
    }
    group.rows.push(row);
  }
  return groups;
});

const typeName = (row: NotificationPreference) =>
  tr(`settings-notifications-type-${keyFragment(row.notification_type)}-name`, row.notification_name);
const typeDescription = (row: NotificationPreference) =>
  tr(
    `settings-notifications-type-${keyFragment(row.notification_type)}-description`,
    row.description ?? ''
  );

// Frequency choices for a channel — `digest` only where the channel batches.
const frequencyOptions = (channelCode: string): DropdownOption[] => {
  const options: DropdownOption[] = [
    { value: 'instant', label: t('settings-notifications-frequency-instant') },
  ];
  if (channelSupportsDigest(channelCode)) {
    options.push({ value: 'digest', label: t('settings-notifications-frequency-digest') });
  }
  options.push({ value: 'off', label: t('settings-notifications-frequency-off') });
  return options;
};

const getFrequency = (row: NotificationPreference, channelCode: string): NotificationFrequency =>
  row.frequencies[channelCode] ?? 'off';

const isLocked = (row: NotificationPreference, channelCode: string): boolean =>
  row.locked[channelCode] === true;

const cellKey = (typeCode: string, channelCode: string) => `${typeCode}-${channelCode}`;

const setFrequency = async (
  row: NotificationPreference,
  channelCode: string,
  frequency: NotificationFrequency
) => {
  if (isLocked(row, channelCode) || frequency === getFrequency(row, channelCode)) return;
  const key = cellKey(row.notification_type, channelCode);
  isSaving.value = key;
  try {
    await updateNotificationPreference(row.notification_type, channelCode, frequency);
    row.frequencies[channelCode] = frequency;
    row.channels[channelCode] = frequency !== 'off';
    emit('success', t('settings-notifications-preference-update-success'));
  } catch {
    emit('error', t('settings-notifications-preference-update-error'));
  } finally {
    isSaving.value = null;
  }
};

// Aggregate frequency across the unlocked rows of a channel: the shared value,
// or 'mixed' when rows differ (drives the quick-settings control).
const channelAggregate = (channelCode: string): NotificationFrequency | 'mixed' => {
  const freqs = preferences.value
    .filter((p) => !isLocked(p, channelCode))
    .map((p) => getFrequency(p, channelCode));
  if (freqs.length === 0) return 'off';
  return freqs.every((f) => f === freqs[0]) ? freqs[0] : 'mixed';
};

const quickOptions = (channelCode: string): DropdownOption[] => [
  ...frequencyOptions(channelCode),
  { value: 'mixed', label: t('settings-notifications-frequency-mixed'), disabled: true },
];

const applyAllForChannel = async (channelCode: string, frequency: NotificationFrequency) => {
  // The 'mixed' quick-settings option is disabled, so only real frequencies
  // reach here.
  isSaving.value = `channel:${channelCode}`;
  let failed = false;
  for (const row of preferences.value) {
    if (isLocked(row, channelCode) || getFrequency(row, channelCode) === frequency) continue;
    try {
      await updateNotificationPreference(row.notification_type, channelCode, frequency);
      row.frequencies[channelCode] = frequency;
      row.channels[channelCode] = frequency !== 'off';
    } catch {
      failed = true;
    }
  }
  isSaving.value = null;
  const channelLabel = channels.value.find((c) => c.code === channelCode)?.name ?? channelCode;
  if (failed) {
    emit('error', t('settings-notifications-preference-update-error'));
  } else {
    emit('success', t('settings-notifications-channel-bulk-applied', { channel: channelLabel }));
  }
};

const requestBrowserPermission = async () => {
  const granted = await requestNotificationPermission();
  browserPermission.value = Notification.permission;
  emit(
    granted ? 'success' : 'error',
    granted
      ? t('settings-notifications-browser-enabled-success')
      : t('settings-notifications-browser-denied-error')
  );
};

onMounted(async () => {
  try {
    preferences.value = await getNotificationPreferences();
    if ('Notification' in window) {
      browserPermission.value = Notification.permission;
    }
  } catch {
    emit('error', t('settings-notifications-load-error'));
  } finally {
    isLoading.value = false;
  }
});

const gridColumns = computed(
  () => `minmax(0, 1fr) repeat(${channels.value.length}, minmax(6.5rem, 8rem))`
);
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
              <span class="text-accent inline-flex"><Icon name="bell" /></span>
            </div>
            <div class="flex-1 min-w-0 flex flex-col gap-2">
              <div class="flex flex-col gap-1">
                <h3 class="text-sm font-medium text-primary">{{ $t('settings-notifications-browser-banner-title') }}</h3>
                <p class="text-xs text-secondary">{{ $t('settings-notifications-browser-banner-description') }}</p>
              </div>
              <div>
                <Button size="sm" @click="requestBrowserPermission">
                  {{ $t('settings-notifications-browser-banner-enable') }}
                </Button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Quick Settings: bulk-apply one frequency to every type on a channel. -->
      <SectionCard content-padding="p-4 sm:p-6">
        <template #leading>
          <span class="text-accent inline-flex"><Icon name="settings" /></span>
        </template>
        <template #title>{{ $t('settings-notifications-quick-settings-title') }}</template>

        <div class="flex flex-col gap-2">
          <div
            v-for="channel in channels"
            :key="`quick-${channel.code}`"
            class="bg-surface-alt rounded-lg border border-subtle px-3 py-2.5 flex items-center justify-between gap-3"
          >
            <div class="flex flex-col gap-0.5 min-w-0">
              <p class="text-sm font-medium text-primary">{{ channel.name }}</p>
              <p class="text-xs text-tertiary truncate">{{ channel.description }}</p>
            </div>
            <div class="w-32 flex-shrink-0">
              <BaseDropdown
                size="sm"
                :model-value="channelAggregate(channel.code) === 'mixed' ? '' : channelAggregate(channel.code)"
                :options="quickOptions(channel.code)"
                :placeholder="$t('settings-notifications-frequency-mixed')"
                :disabled="isSaving === `channel:${channel.code}`"
                @update:model-value="applyAllForChannel(channel.code, $event as NotificationFrequency)"
              />
            </div>
          </div>
        </div>
      </SectionCard>

      <!-- Category Cards -->
      <SectionCard
        v-for="group in groupedPreferences"
        :key="group.category"
        content-padding="p-4 sm:p-6"
      >
        <template #leading>
          <svg class="w-4 h-4 text-accent flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" v-html="group.icon"></svg>
        </template>
        <template #title>{{ group.label }}</template>

        <div>
          <!-- Desktop: matrix grid -->
          <div class="hidden sm:flex sm:flex-col sm:gap-3">
            <div class="grid items-center gap-4 px-3" :style="{ gridTemplateColumns: gridColumns }">
              <div class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ $t('settings-notifications-column-header') }}</div>
              <div
                v-for="channel in channels"
                :key="`header-${channel.code}`"
                class="text-xs font-medium text-tertiary uppercase tracking-wide text-center"
              >
                {{ channel.name }}
              </div>
            </div>

            <div class="flex flex-col gap-1.5">
              <div
                v-for="row in group.rows"
                :key="row.notification_type"
                class="grid items-center gap-4 bg-surface-alt rounded-lg border border-subtle px-3 py-2.5"
                :style="{ gridTemplateColumns: gridColumns }"
              >
                <div class="flex flex-col gap-0.5 min-w-0">
                  <p class="text-sm font-medium text-primary">{{ typeName(row) }}</p>
                  <p class="text-xs text-tertiary truncate">{{ typeDescription(row) }}</p>
                </div>
                <div
                  v-for="channel in channels"
                  :key="cellKey(row.notification_type, channel.code)"
                  class="flex items-center justify-center"
                >
                  <div
                    v-if="isLocked(row, channel.code)"
                    class="flex items-center gap-1 text-xs text-tertiary"
                    :title="$t('settings-notifications-locked-hint')"
                  >
                    <Icon name="lock" size="xs" />
                    <span class="capitalize">{{ getFrequency(row, channel.code) }}</span>
                  </div>
                  <BaseDropdown
                    v-else
                    size="sm"
                    :model-value="getFrequency(row, channel.code)"
                    :options="frequencyOptions(channel.code)"
                    :disabled="isSaving === cellKey(row.notification_type, channel.code)"
                    @update:model-value="setFrequency(row, channel.code, $event as NotificationFrequency)"
                  />
                </div>
              </div>
            </div>
          </div>

          <!-- Mobile: stacked -->
          <div class="sm:hidden flex flex-col gap-2">
            <div
              v-for="row in group.rows"
              :key="`m-${row.notification_type}`"
              class="bg-surface-alt rounded-lg border border-subtle px-3 py-2.5 flex flex-col gap-2"
            >
              <div class="flex flex-col gap-0.5">
                <p class="text-sm font-medium text-primary">{{ typeName(row) }}</p>
                <p class="text-xs text-tertiary">{{ typeDescription(row) }}</p>
              </div>
              <div class="flex flex-col gap-1.5">
                <div
                  v-for="channel in channels"
                  :key="`${cellKey(row.notification_type, channel.code)}-m`"
                  class="flex items-center justify-between gap-3"
                >
                  <span class="text-sm text-secondary">{{ channel.name }}</span>
                  <div
                    v-if="isLocked(row, channel.code)"
                    class="flex items-center gap-1 text-xs text-tertiary"
                    :title="$t('settings-notifications-locked-hint')"
                  >
                    <Icon name="lock" size="xs" />
                    <span class="capitalize">{{ getFrequency(row, channel.code) }}</span>
                  </div>
                  <div v-else class="w-32 flex-shrink-0">
                    <BaseDropdown
                      size="sm"
                      :model-value="getFrequency(row, channel.code)"
                      :options="frequencyOptions(channel.code)"
                      :disabled="isSaving === cellKey(row.notification_type, channel.code)"
                      @update:model-value="setFrequency(row, channel.code, $event as NotificationFrequency)"
                    />
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </SectionCard>

      <!-- Info Footer -->
      <div class="bg-surface rounded-xl border border-default overflow-hidden">
        <div class="px-4 py-3 flex flex-col gap-2">
          <div class="flex items-center gap-3">
            <span class="text-tertiary flex-shrink-0 inline-flex"><Icon name="info" /></span>
            <p class="text-xs text-secondary">{{ $t('settings-notifications-info-footer') }}</p>
          </div>
          <div class="flex items-center gap-3">
            <span class="text-tertiary flex-shrink-0 inline-flex"><Icon name="bell" /></span>
            <p class="text-xs text-secondary">{{ $t('settings-notifications-push-footer') }}</p>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
