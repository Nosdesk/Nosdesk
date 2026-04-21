<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-6xl">
      <div class="flex flex-col gap-2">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">Guest Access</h1>
        <p class="text-secondary">
          Control what unauthenticated visitors can see and submit. All features are disabled by default.
        </p>
      </div>

      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <LoadingSpinner v-if="loading" text="Loading guest settings..." />

      <div v-else-if="settings" class="flex flex-col gap-6">
        <!-- Feature toggles -->
        <div class="bg-surface border border-default rounded-xl p-6 hover:border-strong transition-colors flex flex-col gap-4">
          <h2 class="text-lg font-semibold text-primary">Public Features</h2>

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
            <h2 class="text-lg font-semibold text-primary">Guest Ticket Submissions</h2>
            <p class="text-sm text-secondary">Behavior for tickets submitted through the public form.</p>
          </div>

          <ToggleSwitch
            label="Require email confirmation"
            description="Hold submissions until the requester confirms via email. Also gives them portal access."
            :model-value="settings.guest_ticket_email_verification"
            @update:model-value="settings!.guest_ticket_email_verification = $event"
          />

          <ToggleSwitch
            label="Allow attachments"
            description="Submitters can attach images, PDFs, and text/log files (≤10MB each, up to 5 per ticket)."
            :model-value="settings.guest_ticket_attachments_enabled"
            @update:model-value="settings!.guest_ticket_attachments_enabled = $event"
          />

          <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
            <!-- Default priority -->
            <div class="flex flex-col gap-2">
              <label id="guest-default-priority-label" class="text-sm font-medium text-primary">
                Default priority
              </label>
              <BaseDropdown
                :model-value="priorityValue"
                :options="priorityOptions"
                aria-labelledby="guest-default-priority-label"
                @update:model-value="onPriorityChange"
              />
              <p class="text-xs text-tertiary">
                Applied to every guest submission. Techs can re-triage after.
              </p>
            </div>

            <!-- Intro message (full-width across both columns) -->
            <div class="flex flex-col gap-2 md:col-span-2">
              <label for="guest-intro-message" class="text-sm font-medium text-primary">
                Intro message
                <span class="text-tertiary font-normal">(optional)</span>
              </label>
              <textarea
                id="guest-intro-message"
                v-model="introMessage"
                rows="3"
                maxlength="500"
                placeholder="e.g. For urgent outages call 555-1234. Check our docs first at /docs."
                class="w-full bg-surface-alt border border-default rounded-lg px-3 py-2.5 text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent resize-y transition-colors"
              ></textarea>
              <div class="flex items-center justify-between gap-2">
                <p class="text-xs text-tertiary">
                  Shown above the public submit form. Plain text — line breaks preserved.
                </p>
                <p class="text-xs text-tertiary shrink-0">
                  {{ introMessage.length }} / 500
                </p>
              </div>
            </div>

            <!-- Rate limit -->
            <div class="flex flex-col gap-2">
              <label for="guest-rate-limit" class="text-sm font-medium text-primary">
                Rate limit
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
                  per IP / hour
                </span>
              </div>
              <p class="text-xs text-tertiary">
                Lower this if you see spam from shared IPs.
              </p>
            </div>
          </div>

          <div class="flex items-center justify-end gap-4">
            <div v-if="dirty" class="flex items-center gap-1.5 text-xs text-tertiary">
              <span class="inline-block w-1.5 h-1.5 rounded-full bg-status-warning"></span>
              Unsaved changes
            </div>
            <button
              @click="save"
              :disabled="saving || !dirty"
              class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2 text-sm font-medium"
            >
              <svg v-if="saving" class="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24">
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                <path class="opacity-75" fill="currentColor"
                  d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
              {{ saving ? 'Saving...' : 'Save Settings' }}
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import BaseDropdown, { type DropdownOption } from '@/components/common/BaseDropdown.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import {
  adminGuestSettingsService,
  type AdminGuestSettings
} from '@/services/publicService';

type ToggleKey =
  | 'guest_tickets_enabled'
  | 'guest_ticket_lookup_enabled'
  | 'guest_public_docs_enabled'
  | 'guest_kb_search_enabled'
  | 'guest_help_page_enabled';

const loading = ref(true);
const saving = ref(false);
const settings = ref<AdminGuestSettings | null>(null);
const pristine = ref<AdminGuestSettings | null>(null);
const errorMessage = ref('');
const successMessage = ref('');

const toggles: Array<{ key: ToggleKey; label: string; description: string }> = [
  {
    key: 'guest_tickets_enabled',
    label: 'Accept guest ticket submissions',
    description: 'Shows a public ticket form at /submit-ticket.'
  },
  {
    key: 'guest_ticket_lookup_enabled',
    label: 'Guest ticket status lookup',
    description: 'Lets guests check status via a private link returned on submit.'
  },
  {
    key: 'guest_public_docs_enabled',
    label: 'Public documentation',
    description: "Exposes pages marked 'public' at /docs without requiring login."
  },
  {
    key: 'guest_kb_search_enabled',
    label: 'Public knowledge base search',
    description: "Search over public documentation. Requires 'Public documentation' on."
  },
  {
    key: 'guest_help_page_enabled',
    label: 'Self-service help page',
    description: 'Static /help page with links to password reset and ticket submission.'
  }
];

// Priority is set by admin policy, never by the submitter. Existing rows
// with a null priority fall back to 'medium' in the UI — the next save
// writes a real value through.
const DEFAULT_PRIORITY = 'medium';

const priorityOptions: DropdownOption[] = [
  { value: 'low', label: 'Low' },
  { value: 'medium', label: 'Medium' },
  { value: 'high', label: 'High' }
];

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

onMounted(async () => {
  try {
    const data = await adminGuestSettingsService.get();
    settings.value = data;
    pristine.value = { ...data };
  } catch {
    errorMessage.value = 'Failed to load guest settings';
  } finally {
    loading.value = false;
  }
});

async function save() {
  if (!settings.value) return;
  saving.value = true;
  errorMessage.value = '';
  successMessage.value = '';
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
    successMessage.value = 'Guest access settings saved';
    setTimeout(() => (successMessage.value = ''), 3000);
  } catch {
    errorMessage.value = 'Failed to save guest settings';
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
