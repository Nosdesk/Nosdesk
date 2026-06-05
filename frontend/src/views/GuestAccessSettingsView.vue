<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-6xl">
      <div class="flex flex-col gap-2">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('admin-guest-title') }}</h1>
        <p class="text-secondary">
          {{ $t('admin-guest-description') }}
        </p>
      </div>

      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <AlertMessage v-if="loadError && !settings" type="error" :message="loadError" />

      <!-- First-load skeleton: a couple of card shells mirroring the
           settings sections. Cold cache only; revisits seed the form
           from cache instantly and revalidate silently. -->
      <Skeleton
        v-if="isFirstLoad"
        :label="$t('admin-guest-loading')"
        class="flex flex-col gap-6"
      >
        <div
          v-for="n in 2"
          :key="n"
          class="bg-surface border border-default rounded-xl p-6 flex flex-col gap-4"
        >
          <SkeletonBar class="h-5 w-48 max-w-full" />
          <SkeletonBar class="h-4 w-full" />
          <SkeletonBar class="h-4 w-5/6" />
          <SkeletonBar class="h-4 w-2/3" />
        </div>
      </Skeleton>

      <div v-else-if="settings" class="flex flex-col gap-6">
        <!-- Feature toggles -->
        <div class="bg-surface border border-default rounded-xl p-6 hover:border-strong transition-colors flex flex-col gap-4">
          <h2 class="text-lg font-semibold text-primary">{{ $t('admin-guest-features-title') }}</h2>

          <div class="flex flex-col divide-y divide-default">
            <div v-for="toggle in toggles" :key="toggle.key" class="py-3 first:pt-0 last:pb-0">
              <ToggleSwitch
                :label="toggle.label"
                :description="toggle.description"
                :model-value="settings[toggle.key]"
                :disabled="toggle.key === 'guest_kb_search_enabled' && !settings.guest_public_docs_enabled"
                @update:model-value="settings![toggle.key] = $event"
              />
            </div>
          </div>
        </div>

        <!-- Ticket submission settings -->
        <div class="bg-surface border border-default rounded-xl p-6 hover:border-strong transition-colors flex flex-col gap-6">
          <div class="flex flex-col gap-1">
            <h2 class="text-lg font-semibold text-primary">{{ $t('admin-guest-submissions-title') }}</h2>
            <p class="text-sm text-secondary">{{ $t('admin-guest-submissions-description') }}</p>
          </div>

          <ToggleSwitch
            :label="$t('admin-guest-toggle-email-verification-label')"
            :description="$t('admin-guest-toggle-email-verification-description')"
            :model-value="settings.guest_ticket_email_verification"
            @update:model-value="settings!.guest_ticket_email_verification = $event"
          />

          <ToggleSwitch
            :label="$t('admin-guest-toggle-attachments-label')"
            :description="$t('admin-guest-toggle-attachments-description')"
            :model-value="settings.guest_ticket_attachments_enabled"
            @update:model-value="settings!.guest_ticket_attachments_enabled = $event"
          />

          <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
            <!-- Default priority -->
            <div class="flex flex-col gap-2">
              <label id="guest-default-priority-label" class="text-sm font-medium text-primary">
                {{ $t('admin-guest-default-priority-label') }}
              </label>
              <BaseDropdown
                :model-value="priorityValue"
                :options="priorityOptions"
                aria-labelledby="guest-default-priority-label"
                @update:model-value="onPriorityChange"
              />
              <p class="text-xs text-tertiary">
                {{ $t('admin-guest-default-priority-hint') }}
              </p>
            </div>

            <!-- Intro message (full-width across both columns) -->
            <div class="flex flex-col gap-2 md:col-span-2">
              <label for="guest-intro-message" class="text-sm font-medium text-primary">
                {{ $t('admin-guest-intro-message-label') }}
                <span class="text-tertiary font-normal">{{ $t('admin-guest-intro-message-optional') }}</span>
              </label>
              <textarea
                id="guest-intro-message"
                v-model="introMessage"
                rows="3"
                maxlength="500"
                :placeholder="$t('admin-guest-intro-message-placeholder')"
                class="w-full bg-surface-alt border border-default rounded-lg px-3 py-2.5 text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent resize-y transition-colors"
              ></textarea>
              <div class="flex items-center justify-between gap-2">
                <p class="text-xs text-tertiary">
                  {{ $t('admin-guest-intro-message-hint') }}
                </p>
                <p class="text-xs text-tertiary shrink-0">
                  {{ $t('admin-guest-intro-message-count', { count: introMessage.length }) }}
                </p>
              </div>
            </div>

            <!-- Rate limit -->
            <div class="flex flex-col gap-2">
              <label for="guest-rate-limit" class="text-sm font-medium text-primary">
                {{ $t('admin-guest-rate-limit-label') }}
              </label>
              <div class="relative">
                <input
                  id="guest-rate-limit"
                  type="number"
                  min="1"
                  max="1000"
                  v-model.number="settings.guest_ticket_rate_limit_per_hour"
                  class="w-full bg-surface-alt border border-default rounded-lg pl-3 pr-24 py-2.5 text-primary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none"
                />
                <span class="absolute right-3 top-1/2 -translate-y-1/2 text-sm text-tertiary pointer-events-none select-none">
                  {{ $t('admin-guest-rate-limit-suffix') }}
                </span>
              </div>
              <p class="text-xs text-tertiary">
                {{ $t('admin-guest-rate-limit-hint') }}
              </p>
            </div>
          </div>

          <div class="flex items-center justify-end gap-4">
            <div v-if="dirty" class="flex items-center gap-1.5 text-xs text-tertiary">
              <span class="inline-block w-1.5 h-1.5 rounded-full bg-status-warning"></span>
              {{ $t('admin-guest-unsaved') }}
            </div>
            <button
              @click="save"
              :disabled="saving || !dirty"
              class="px-4 py-2 bg-accent text-on-accent rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2 text-sm font-medium"
            >
              <svg v-if="saving" class="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24">
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                <path class="opacity-75" fill="currentColor"
                  d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
              {{ saving ? $t('admin-guest-saving') : $t('admin-guest-save') }}
            </button>
          </div>
        </div>
      </div>
    </div>

    <ConfirmModal
      :show="showLeaveConfirm"
      variant="warning"
      :title="$t('settings-unsaved-leave-title')"
      :message="$t('settings-unsaved-leave-message')"
      :confirm-label="$t('settings-unsaved-leave-confirm')"
      :cancel-label="$t('settings-unsaved-leave-cancel')"
      @confirm="confirmLeave"
      @close="cancelLeave"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import AlertMessage from '@/components/common/AlertMessage.vue';
import BaseDropdown, { type DropdownOption } from '@/components/common/BaseDropdown.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import { useToastStore } from '@/stores/toast';
import { useUnsavedChanges } from '@/composables/useUnsavedChanges';
import {
  adminGuestSettingsService,
  type AdminGuestSettings
} from '@/services/publicService';

const fluent = useFluent();
const t = (key: string) => fluent.$t(key);
const toast = useToastStore();

type ToggleKey =
  | 'guest_tickets_enabled'
  | 'guest_ticket_lookup_enabled'
  | 'guest_public_docs_enabled'
  | 'guest_kb_search_enabled'
  | 'guest_help_page_enabled';

// Settings are fetched through Pinia Colada so a revisit renders the
// form instantly from cache and revalidates silently. The form seeds
// from the cached data (see the watch below) only while clean, so a
// background refetch never clobbers in-progress edits.
const GUEST_SETTINGS_KEY = ['guest-settings'] as const;
const queryCache = useQueryCache();
const settingsQuery = useQuery({
  key: GUEST_SETTINGS_KEY,
  query: () => adminGuestSettingsService.get(),
});
const isFirstLoad = computed(
  () => settingsQuery.status.value === 'pending' && settingsQuery.data.value === undefined,
);
const loadError = computed(() =>
  settingsQuery.error.value ? t('admin-guest-error-load') : '',
);

const saving = ref(false);
// `settings` is the editable working copy; `pristine` is the last
// saved/loaded baseline used for dirty-tracking.
const settings = ref<AdminGuestSettings | null>(null);
const pristine = ref<AdminGuestSettings | null>(null);
const errorMessage = ref('');

const toggles = computed<Array<{ key: ToggleKey; label: string; description: string }>>(() => [
  {
    key: 'guest_tickets_enabled',
    label: t('admin-guest-toggle-tickets-label'),
    description: t('admin-guest-toggle-tickets-description')
  },
  {
    key: 'guest_ticket_lookup_enabled',
    label: t('admin-guest-toggle-lookup-label'),
    description: t('admin-guest-toggle-lookup-description')
  },
  {
    key: 'guest_public_docs_enabled',
    label: t('admin-guest-toggle-public-docs-label'),
    description: t('admin-guest-toggle-public-docs-description')
  },
  {
    key: 'guest_kb_search_enabled',
    label: t('admin-guest-toggle-kb-search-label'),
    description: t('admin-guest-toggle-kb-search-description')
  },
  {
    key: 'guest_help_page_enabled',
    label: t('admin-guest-toggle-help-label'),
    description: t('admin-guest-toggle-help-description')
  }
]);

// Priority is set by admin policy, never by the submitter. Existing rows
// with a null priority fall back to 'medium' in the UI — the next save
// writes a real value through.
const DEFAULT_PRIORITY = 'medium';

const priorityOptions = computed<DropdownOption[]>(() => [
  { value: 'low', label: t('admin-guest-priority-low') },
  { value: 'medium', label: t('admin-guest-priority-medium') },
  { value: 'high', label: t('admin-guest-priority-high') }
]);

const priorityValue = computed(
  () => settings.value?.guest_ticket_default_priority ?? DEFAULT_PRIORITY
);

function onPriorityChange(value: string | string[]) {
  if (!settings.value || Array.isArray(value)) return;
  settings.value.guest_ticket_default_priority = value;
}

// Intro textarea writes through a local ref so the null ↔ "" conversion
// only happens at save time. An empty or whitespace-only string is sent
// as null so the frontend form doesn't render an empty callout.
const introMessage = computed<string>({
  get: () => settings.value?.guest_ticket_intro_message ?? '',
  set: (v) => {
    if (settings.value) settings.value.guest_ticket_intro_message = v;
  }
});

const dirty = computed(() => {
  if (!settings.value || !pristine.value) return false;
  return JSON.stringify(settings.value) !== JSON.stringify(pristine.value);
});

// Prompt before navigating away (or closing the tab) with unsaved edits.
const { showLeaveConfirm, confirmLeave, cancelLeave } = useUnsavedChanges(dirty);

// Seed the editable form from the cached query. Skip while dirty so a
// silent background revalidation never overwrites edits in progress.
watch(
  settingsQuery.data,
  (data) => {
    if (!data || dirty.value) return;
    settings.value = { ...data };
    pristine.value = { ...data };
  },
  { immediate: true },
);

async function save() {
  if (!settings.value) return;
  saving.value = true;
  errorMessage.value = '';
  try {
    const data = await adminGuestSettingsService.update({
      guest_tickets_enabled: settings.value.guest_tickets_enabled,
      guest_public_docs_enabled: settings.value.guest_public_docs_enabled,
      guest_kb_search_enabled: settings.value.guest_kb_search_enabled,
      guest_ticket_lookup_enabled: settings.value.guest_ticket_lookup_enabled,
      guest_help_page_enabled: settings.value.guest_help_page_enabled,
      guest_ticket_default_priority: settings.value.guest_ticket_default_priority,
      guest_ticket_rate_limit_per_hour: settings.value.guest_ticket_rate_limit_per_hour,
      guest_ticket_email_verification: settings.value.guest_ticket_email_verification,
      guest_ticket_attachments_enabled: settings.value.guest_ticket_attachments_enabled,
      guest_ticket_intro_message:
        (settings.value.guest_ticket_intro_message ?? '').trim() || null
    });
    settings.value = data;
    pristine.value = { ...data };
    // Keep the cache in lockstep so a later revisit shows the saved
    // values without a network round-trip.
    queryCache.setQueryData(GUEST_SETTINGS_KEY, data);
    toast.success(t('admin-guest-saved'));
  } catch {
    errorMessage.value = t('admin-guest-error-save');
  } finally {
    saving.value = false;
  }
}

watch(
  () => settings.value?.guest_public_docs_enabled,
  (v) => {
    if (!v && settings.value?.guest_kb_search_enabled) {
      settings.value.guest_kb_search_enabled = false;
    }
  }
);
</script>
