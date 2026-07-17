<script setup lang="ts">
/**
 * Admin editor for the workspace's notification DEFAULTS — the middle layer of
 * the system → workspace → user inheritance. Sets the default delivery
 * frequency per (type, channel) that members inherit, and lets an admin `lock`
 * a cell so members cannot override it (the ceiling pattern: a locked default
 * is enforced and shown read-only on each member's own settings).
 */
import { ref, onMounted, computed } from 'vue';
import { useFluent } from 'fluent-vue';

import BackButton from '@/components/common/BackButton.vue';
import Icon from '@/components/common/Icon.vue';
import Callout from '@/components/common/Callout.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import BaseDropdown, { type DropdownOption } from '@/components/common/BaseDropdown.vue';
import SegmentedControl from '@/components/common/SegmentedControl.vue';
import {
  getWorkspaceNotificationDefaults,
  updateWorkspaceNotificationDefault,
  getNotificationContentLevel,
  setNotificationContentLevel,
  channelSupportsDigest,
  NOTIFICATION_CHANNELS,
  type WorkspaceNotificationDefault,
  type NotificationFrequency,
  type NotificationContentLevel,
} from '@nosdesk/core/services/notificationService';
import { useToastStore } from '@nosdesk/core/stores/toast';
import { extractErrorMessage } from '@/utils/errors';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const toast = useToastStore();

// Fluent returns the key when a message is missing; fall back to the API's
// English string so a not-yet-translated type still renders sensibly.
const tr = (key: string, fallback: string): string => {
  const value = t(key);
  return value === key ? fallback : value;
};
const keyFragment = (code: string) => code.replace(/_/g, '-');

const isLoading = ref(true);
const loadError = ref('');
const saving = ref<string | null>(null);
const defaults = ref<WorkspaceNotificationDefault[]>([]);

// Push content level (workspace-wide): detailed context vs private "tap to view".
const contentLevel = ref<NotificationContentLevel>('detailed');
const contentSaving = ref(false);
const contentOptions = computed(() => [
  { value: 'detailed', label: t('admin-notification-content-detailed') },
  { value: 'private', label: t('admin-notification-content-private') },
]);

const setContentLevel = async (level: NotificationContentLevel) => {
  if (level === contentLevel.value) return;
  const previous = contentLevel.value;
  contentLevel.value = level;
  contentSaving.value = true;
  try {
    await setNotificationContentLevel(level);
    toast.success(t('admin-notification-content-saved'));
  } catch (err) {
    contentLevel.value = previous;
    toast.error(extractErrorMessage(err, t('admin-notification-content-error')));
  } finally {
    contentSaving.value = false;
  }
};

const channels = computed(() =>
  NOTIFICATION_CHANNELS.map((channel) => ({
    code: channel.code,
    name: tr(`settings-notifications-channel-${keyFragment(channel.code)}-name`, channel.name),
  }))
);

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

const groupedDefaults = computed(() => {
  const groups: {
    category: string;
    label: string;
    icon: string;
    rows: WorkspaceNotificationDefault[];
  }[] = [];
  for (const row of defaults.value) {
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

const typeName = (row: WorkspaceNotificationDefault) =>
  tr(`settings-notifications-type-${keyFragment(row.notification_type)}-name`, row.notification_name);
const typeDescription = (row: WorkspaceNotificationDefault) =>
  tr(
    `settings-notifications-type-${keyFragment(row.notification_type)}-description`,
    row.description ?? ''
  );

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

const getFrequency = (row: WorkspaceNotificationDefault, channelCode: string): NotificationFrequency =>
  row.frequencies[channelCode] ?? 'off';
const isLocked = (row: WorkspaceNotificationDefault, channelCode: string): boolean =>
  row.locked[channelCode] === true;
const cellKey = (typeCode: string, channelCode: string) => `${typeCode}-${channelCode}`;

// The backend PUT sets frequency + locked together, so every cell edit sends
// both — the unchanged one comes from local state.
const saveCell = async (
  row: WorkspaceNotificationDefault,
  channelCode: string,
  frequency: NotificationFrequency,
  locked: boolean
) => {
  const key = cellKey(row.notification_type, channelCode);
  saving.value = key;
  try {
    await updateWorkspaceNotificationDefault(row.notification_type, channelCode, frequency, locked);
    row.frequencies[channelCode] = frequency;
    row.locked[channelCode] = locked;
    toast.success(t('admin-notification-defaults-saved'));
  } catch (err) {
    toast.error(extractErrorMessage(err, t('admin-notification-defaults-error-save')));
  } finally {
    saving.value = null;
  }
};

const setDefaultFrequency = (
  row: WorkspaceNotificationDefault,
  channelCode: string,
  frequency: NotificationFrequency
) => {
  if (frequency === getFrequency(row, channelCode)) return;
  return saveCell(row, channelCode, frequency, isLocked(row, channelCode));
};
const toggleLock = (row: WorkspaceNotificationDefault, channelCode: string) =>
  saveCell(row, channelCode, getFrequency(row, channelCode), !isLocked(row, channelCode));

onMounted(async () => {
  try {
    const [rows, level] = await Promise.all([
      getWorkspaceNotificationDefaults(),
      getNotificationContentLevel().catch(() => 'detailed' as NotificationContentLevel),
    ]);
    defaults.value = rows;
    contentLevel.value = level;
  } catch (err) {
    loadError.value = extractErrorMessage(err, t('admin-notification-defaults-error-load'));
  } finally {
    isLoading.value = false;
  }
});

const gridColumns = computed(
  () => `minmax(0, 1fr) repeat(${channels.value.length}, minmax(8.5rem, 10rem))`
);
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-5xl">
      <div class="flex flex-col gap-2">
        <BackButton :fallback-route="'/admin'" :label="t('admin-notification-defaults-back-label')" compact />
        <div class="flex flex-col gap-1">
          <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ t('admin-notification-defaults-title') }}</h1>
          <p class="text-secondary text-sm sm:text-base">{{ t('admin-notification-defaults-description') }}</p>
        </div>
      </div>

      <Callout severity="info">
        <template #header>{{ t('admin-notification-defaults-lock-explainer-title') }}</template>
        {{ t('admin-notification-defaults-lock-explainer-body') }}
      </Callout>

      <!-- Push content level: detailed context vs private "tap to view". -->
      <SectionCard content-padding="p-4 sm:p-6">
        <template #leading>
          <span class="text-accent inline-flex"><Icon name="bell" /></span>
        </template>
        <template #title>{{ t('admin-notification-content-title') }}</template>
        <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <p class="text-sm text-secondary">{{ t('admin-notification-content-description') }}</p>
          <div class="flex-shrink-0" :class="{ 'opacity-60 pointer-events-none': contentSaving }">
            <SegmentedControl
              :model-value="contentLevel"
              :options="contentOptions"
              :aria-label="t('admin-notification-content-title')"
              @update:model-value="setContentLevel($event as NotificationContentLevel)"
            />
          </div>
        </div>
      </SectionCard>

      <AlertMessage v-if="loadError" type="error" :message="loadError" />

      <!-- Loading -->
      <div v-else-if="isLoading" class="bg-surface rounded-xl border border-default overflow-hidden">
        <div class="p-4 flex flex-col gap-3">
          <div v-for="i in 4" :key="i" class="h-12 bg-surface-alt rounded-lg animate-pulse"></div>
        </div>
      </div>

      <template v-else>
        <SectionCard
          v-for="group in groupedDefaults"
          :key="group.category"
          content-padding="p-4 sm:p-6"
        >
          <template #leading>
            <svg class="w-4 h-4 text-accent flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" v-html="group.icon"></svg>
          </template>
          <template #title>{{ group.label }}</template>

          <div>
            <!-- Desktop: matrix -->
            <div class="hidden sm:flex sm:flex-col sm:gap-3">
              <div class="grid items-center gap-4 px-3" :style="{ gridTemplateColumns: gridColumns }">
                <div class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ t('settings-notifications-column-header') }}</div>
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
                    class="flex items-center gap-1.5"
                  >
                    <div class="flex-1 min-w-0">
                      <BaseDropdown
                        size="sm"
                        :model-value="getFrequency(row, channel.code)"
                        :options="frequencyOptions(channel.code)"
                        :disabled="saving === cellKey(row.notification_type, channel.code)"
                        @update:model-value="setDefaultFrequency(row, channel.code, $event as NotificationFrequency)"
                      />
                    </div>
                    <button
                      type="button"
                      class="flex items-center justify-center w-7 h-7 rounded-md border transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-1 focus-visible:ring-offset-surface"
                      :class="isLocked(row, channel.code)
                        ? 'bg-accent/10 border-accent/30 text-accent'
                        : 'border-subtle text-tertiary hover:text-secondary hover:border-default'"
                      :disabled="saving === cellKey(row.notification_type, channel.code)"
                      :aria-pressed="isLocked(row, channel.code)"
                      :title="isLocked(row, channel.code)
                        ? t('admin-notification-defaults-locked-title')
                        : t('admin-notification-defaults-unlocked-title')"
                      @click="toggleLock(row, channel.code)"
                    >
                      <Icon name="lock" size="xs" />
                    </button>
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
                    class="flex items-center justify-between gap-2"
                  >
                    <span class="text-sm text-secondary">{{ channel.name }}</span>
                    <div class="flex items-center gap-1.5">
                      <div class="w-32">
                        <BaseDropdown
                          size="sm"
                          :model-value="getFrequency(row, channel.code)"
                          :options="frequencyOptions(channel.code)"
                          :disabled="saving === cellKey(row.notification_type, channel.code)"
                          @update:model-value="setDefaultFrequency(row, channel.code, $event as NotificationFrequency)"
                        />
                      </div>
                      <button
                        type="button"
                        class="flex items-center justify-center w-7 h-7 rounded-md border transition-colors"
                        :class="isLocked(row, channel.code)
                          ? 'bg-accent/10 border-accent/30 text-accent'
                          : 'border-subtle text-tertiary'"
                        :disabled="saving === cellKey(row.notification_type, channel.code)"
                        :aria-pressed="isLocked(row, channel.code)"
                        :title="isLocked(row, channel.code)
                          ? t('admin-notification-defaults-locked-title')
                          : t('admin-notification-defaults-unlocked-title')"
                        @click="toggleLock(row, channel.code)"
                      >
                        <Icon name="lock" size="xs" />
                      </button>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </SectionCard>
      </template>
    </div>
  </div>
</template>
