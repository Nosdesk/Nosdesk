<template>
  <PublicLayout :content-class="contentWidth">
    <!-- No loader: the settings call is brief and the form itself is the
         ideal optimistic state. The disabled notice only renders once
         settings resolve and the flag is confirmed off. -->
    <FeatureDisabledNotice
      v-if="!loading && !enabled"
      :title="t('guest-submit-disabled-title')"
      :message="t('guest-submit-disabled-message')"
    />

    <!-- Success state: awaiting email verification -->
    <div
      v-else-if="success?.verification_required"
      class="bg-surface border border-default rounded-xl shadow-sm p-6 sm:p-8 flex flex-col items-center gap-5 text-center"
    >
      <div class="w-12 h-12 rounded-full bg-accent-muted flex items-center justify-center">
        <Icon name="email" size="lg" class="text-accent" />
      </div>
      <div class="flex flex-col gap-2">
        <h1 class="text-xl font-semibold text-primary">{{ t('guest-submit-verify-title') }}</h1>
        <p class="text-sm text-secondary">
          {{ t('guest-submit-verify-message-prefix') }}
          <span class="text-primary font-medium">{{ submittedEmail }}</span>
          {{ t('guest-submit-verify-message-suffix') }}
        </p>
      </div>
      <p class="text-xs text-tertiary">
        {{ t('guest-submit-verify-spam-hint') }}
      </p>
      <Button variant="secondary" @click="submitAnother">
        {{ t('guest-submit-another') }}
      </Button>
    </div>

    <!-- Success state: verification not required -->
    <div
      v-else-if="success"
      class="bg-surface border border-default rounded-xl shadow-sm p-6 sm:p-8 flex flex-col gap-5"
    >
      <div class="flex items-start gap-4">
        <div class="shrink-0 w-10 h-10 rounded-full bg-status-success-muted flex items-center justify-center">
          <Icon name="check" size="md" class="text-status-success" />
        </div>
        <div class="flex-1 min-w-0 flex flex-col gap-1">
          <h1 class="text-xl font-semibold text-primary">{{ t('guest-submit-success-title') }}</h1>
          <p class="text-sm text-secondary">
            <template v-if="success.email_sent">
              {{ t('guest-submit-success-email-prefix') }}
              <span class="text-primary font-medium">{{ submittedEmail }}</span>
              {{ t('guest-submit-success-email-suffix') }}
            </template>
            <template v-else>
              {{ t('guest-submit-success-no-email') }}
            </template>
            {{ t('guest-submit-success-reference-prefix') }}
            <span class="text-primary font-mono font-medium">#{{ success.ticket_id }}</span>.
          </p>
        </div>
      </div>

      <div
        v-if="lookupEnabled && success.status_url"
        class="rounded-lg border border-default bg-surface-alt p-4 flex flex-col gap-2"
      >
        <div class="text-xs font-medium text-tertiary uppercase tracking-wide">
          {{ t('guest-submit-track-heading') }}
        </div>
        <div class="flex flex-col sm:flex-row sm:items-center gap-2">
          <code class="flex-1 min-w-0 text-xs text-primary font-mono break-all">{{ statusAbsolute }}</code>
          <Button variant="secondary" size="sm" :icon="copied ? 'check' : 'copy'" @click="copyLink">
            {{ copied ? t('guest-submit-copied') : t('guest-submit-copy') }}
          </Button>
        </div>
        <p class="text-xs text-tertiary">
          {{ t('guest-submit-track-hint') }}
        </p>
      </div>

      <div class="flex flex-col sm:flex-row gap-2">
        <RouterLink
          v-if="lookupEnabled && success.status_url"
          :to="success.status_url"
          class="inline-flex items-center justify-center px-4 py-2 rounded-lg text-sm font-medium text-on-accent bg-accent hover:opacity-90 transition-colors"
        >
          {{ t('guest-submit-view-status') }}
        </RouterLink>
        <Button variant="secondary" @click="submitAnother">
          {{ t('guest-submit-another-short') }}
        </Button>
      </div>
    </div>

    <!-- Form state (also rendered optimistically while settings are loading) -->
    <template v-else>
      <div class="flex flex-col gap-1 text-center">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ t('guest-submit-heading') }}</h1>
        <p class="text-sm text-secondary">{{ t('guest-submit-tagline') }}</p>
      </div>

      <!-- Admin-configured intro message. Plain text only; `whitespace-pre-line`
           preserves line breaks without opening an HTML/XSS surface. -->
      <div
        v-if="introMessage"
        class="rounded-lg border border-status-info/30 bg-status-info-muted p-4 flex items-start gap-3"
      >
        <Icon name="info" size="md" class="text-status-info shrink-0 mt-0.5" />
        <p class="text-sm text-secondary whitespace-pre-line">{{ introMessage }}</p>
      </div>

      <form
        @submit.prevent="submit"
        novalidate
        class="bg-surface border border-default rounded-xl shadow-sm p-5 sm:p-6 flex flex-col gap-4"
      >
        <!-- Honeypot: invisible to humans (sr-only + aria-hidden + tabindex=-1
             + off-screen positioning + autocomplete=off). Bots that auto-fill
             every field they find will populate this; the backend rejects
             any submission where it's non-empty. Do NOT add a label or
             placeholder that would hint at its purpose. -->
        <div aria-hidden="true" class="sr-only" style="position: absolute; left: -9999px;">
          <label>
            {{ t('guest-submit-honeypot-label') }}
            <input
              type="text"
              name="website"
              tabindex="-1"
              autocomplete="off"
              v-model="form.website"
            />
          </label>
        </div>

        <AlertMessage v-if="error" type="error" :message="error" />

        <FormInput
          v-model="form.name"
          :label="t('guest-submit-field-name')"
          :placeholder="t('guest-submit-field-name-placeholder')"
          :error="fieldErrors.name ?? undefined"
          required
          maxlength="120"
          autocomplete="name"
          @blur="validateField('name')"
          @input="fieldErrors.name && validateField('name')"
        />

        <FormInput
          v-model="form.email"
          type="email"
          :label="t('guest-submit-field-email')"
          :placeholder="t('guest-submit-field-email-placeholder')"
          :error="fieldErrors.email ?? undefined"
          required
          autocomplete="email"
          @blur="validateField('email')"
          @input="fieldErrors.email && validateField('email')"
        />

        <FormInput
          v-model="form.title"
          :label="t('guest-submit-field-title')"
          :placeholder="t('guest-submit-field-title-placeholder')"
          :error="fieldErrors.title ?? undefined"
          required
          maxlength="255"
          @blur="validateField('title')"
          @input="fieldErrors.title && validateField('title')"
        />

        <FormTextarea
          v-model="form.description"
          :label="t('guest-submit-field-description')"
          :placeholder="t('guest-submit-field-description-placeholder')"
          :error="fieldErrors.description ?? undefined"
          :description="t('guest-submit-description-counter', { count: form.description.length })"
          required
          :rows="5"
          resize="vertical"
          maxlength="10000"
          @blur="validateField('description')"
          @input="fieldErrors.description && validateField('description')"
        />

        <!-- Attachments (only rendered when the admin has enabled them) -->
        <div v-if="attachmentsEnabled" class="flex flex-col gap-2">
          <div class="flex items-center justify-between gap-2">
            <span class="text-xs font-medium text-tertiary uppercase tracking-wide">
              {{ t('guest-submit-attachments-label') }}
              <span class="normal-case font-normal">{{ t('guest-submit-attachments-optional') }}</span>
            </span>
            <span class="text-xs text-tertiary">
              {{ t('guest-submit-attachments-counter', { count: attachments.length, max: MAX_FILES }) }}
            </span>
          </div>

          <label
            v-if="attachments.length < MAX_FILES"
            class="flex flex-col items-center justify-center gap-1 p-4 border border-dashed rounded-lg cursor-pointer transition-colors"
            :class="[
              dragActive ? 'border-accent bg-surface-alt' : 'border-default hover:border-accent hover:bg-surface-alt',
              uploading ? 'opacity-50 cursor-not-allowed' : '',
            ]"
            @dragover.prevent="onDragOver"
            @dragenter.prevent="onDragOver"
            @dragleave="onDragLeave"
            @drop.prevent="onDrop"
          >
            <input
              type="file"
              class="sr-only"
              multiple
              :accept="ACCEPT_TYPES"
              :disabled="uploading"
              @change="onFilePick"
            />
            <Icon name="paperclip" size="md" class="text-tertiary" />
            <span class="text-xs text-secondary">
              {{ uploading ? t('guest-submit-attachments-uploading') : t('guest-submit-attachments-pick') }}
            </span>
            <span class="text-[11px] text-tertiary">
              {{ t('guest-submit-attachments-hint', { size: MAX_SIZE_MB }) }}
            </span>
          </label>

          <ul
            v-if="attachments.length"
            class="flex flex-col gap-1.5 rounded-lg border border-default bg-surface-alt p-2"
          >
            <li
              v-for="att in attachments"
              :key="att.id"
              class="flex items-center gap-3 px-2 py-1.5 rounded-md bg-surface border border-default"
            >
              <Icon name="paperclip" class="shrink-0 text-tertiary" />
              <div class="flex-1 min-w-0 flex flex-col">
                <span class="text-xs text-primary truncate">{{ att.name }}</span>
                <span class="text-[11px] text-tertiary">{{ formatSize(att.size) }}</span>
              </div>
              <Button
                variant="ghost-danger"
                size="sm"
                icon="close"
                class="!px-2"
                :aria-label="t('guest-submit-attachments-remove-aria', { name: att.name })"
                @click="removeAttachment(att.id)"
              />
            </li>
          </ul>

          <p v-if="attachmentError" class="text-xs text-status-error">{{ attachmentError }}</p>
        </div>

        <div class="flex justify-end">
          <Button type="submit" :loading="submitting">
            {{ submitting ? t('guest-submit-submitting') : t('guest-submit-submit') }}
          </Button>
        </div>
      </form>

      <p class="text-center text-sm text-tertiary">
        {{ t('guest-submit-have-account') }}
        <RouterLink to="/login" class="text-accent hover:opacity-90 font-medium">{{ t('guest-submit-sign-in') }}</RouterLink>
      </p>
    </template>
  </PublicLayout>
</template>

<script setup lang="ts">
import { ref, reactive, computed, onMounted } from 'vue';
import { RouterLink } from 'vue-router';
import { useFluent } from 'fluent-vue';
import PublicLayout from './PublicLayout.vue';
import FeatureDisabledNotice from './FeatureDisabledNotice.vue';
import Icon from '@/components/common/Icon.vue';
import Button from '@/components/common/Button.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import FormInput from '@/components/common/FormInput.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import { usePublicSettingsStore } from '@nosdesk/core/stores/publicSettings';
import {
  publicService,
  type GuestAttachmentUpload,
  type SubmitGuestTicketResponse
} from '@nosdesk/core/services/publicService';
import axios from 'axios';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

// Keep in sync with backend validation (utils/file_validation.rs).
const MAX_FILES = 5;
const MAX_SIZE_MB = 10;
const MAX_SIZE_BYTES = MAX_SIZE_MB * 1024 * 1024;
const ACCEPT_TYPES =
  '.jpg,.jpeg,.png,.gif,.webp,.pdf,.txt,.log,image/jpeg,image/png,image/gif,image/webp,application/pdf,text/plain';

// Form column is slightly wider than the default auth-page max-w-md to
// give the description textarea room to breathe across 4+ inputs. The
// disabled / success states still use the default narrower column.
const FORM_WIDTH = 'max-w-lg mx-auto w-full';
const NARROW_WIDTH = 'max-w-md mx-auto w-full';

const store = usePublicSettingsStore();
const loading = ref(true);
const submitting = ref(false);
const error = ref<string | null>(null);
const success = ref<SubmitGuestTicketResponse | null>(null);
const submittedEmail = ref('');
const copied = ref(false);
const attachments = ref<GuestAttachmentUpload[]>([]);
const uploading = ref(false);
const attachmentError = ref('');
const dragActive = ref(false);

// Priority is deliberately NOT surfaced to the submitter, every helpdesk
// that has tried it discovers everyone marks "high," which renders the
// field useless for triage. Priority is an admin-configured default
// (site_settings.guest_ticket_default_priority) and techs re-triage after
// the ticket lands.
const form = reactive({
  name: '',
  email: '',
  title: '',
  description: '',
  // Honeypot, always empty when a human submits. See the hidden input in
  // the template for the details on why we pose this as "website".
  website: ''
});

type FieldKey = 'name' | 'email' | 'title' | 'description';
const fieldErrors = reactive<Record<FieldKey, string | null>>({
  name: null,
  email: null,
  title: null,
  description: null
});

const enabled = computed(() => store.settings?.guest_tickets_enabled === true);
const lookupEnabled = computed(() => store.settings?.guest_ticket_lookup_enabled === true);
const attachmentsEnabled = computed(
  () => store.settings?.guest_ticket_attachments_enabled === true
);
const introMessage = computed(() => store.settings?.guest_ticket_intro_message ?? '');
const statusAbsolute = computed(() =>
  success.value?.status_url ? `${window.location.origin}${success.value.status_url}` : ''
);

const contentWidth = computed(() => {
  if (loading.value || !enabled.value || success.value) return NARROW_WIDTH;
  return FORM_WIDTH;
});

onMounted(async () => {
  await store.load();
  loading.value = false;
});

// Per-field validation. Trims before checking so a whitespace-only value
// fails the same way the backend would. Run on blur and, once an error is
// showing, on every keystroke so it clears the moment the input is fixed.
function fieldError(field: FieldKey): string | null {
  switch (field) {
    case 'name':
      return form.name.trim().length < 1 ? t('guest-submit-error-name') : null;
    case 'email':
      return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(form.email.trim())
        ? null
        : t('guest-submit-error-email');
    case 'title':
      return form.title.trim().length < 1 ? t('guest-submit-error-title') : null;
    case 'description':
      return form.description.trim().length < 1 ? t('guest-submit-error-description') : null;
  }
}

function validateField(field: FieldKey) {
  fieldErrors[field] = fieldError(field);
}

function validate(): boolean {
  (['name', 'email', 'title', 'description'] as FieldKey[]).forEach(validateField);
  return !Object.values(fieldErrors).some(Boolean);
}

async function submit() {
  error.value = null;
  if (!validate()) return;
  if (uploading.value) {
    error.value = t('guest-submit-error-uploads-pending');
    return;
  }
  submitting.value = true;
  try {
    const response = await publicService.submitTicket({
      name: form.name.trim(),
      email: form.email.trim(),
      title: form.title.trim(),
      description: form.description.trim(),
      website: form.website,
      attachment_ids: attachments.value.map((a) => a.id)
    });
    submittedEmail.value = form.email.trim();
    success.value = response;
  } catch (e: unknown) {
    if (axios.isAxiosError(e)) {
      if (e.response?.status === 429) {
        error.value = t('guest-submit-error-rate-limited');
      } else if (e.response?.status === 403) {
        error.value = t('guest-submit-error-disabled');
        await store.load(true);
      } else if (e.response?.status === 409) {
        error.value = t('guest-submit-error-account-exists');
      } else {
        const data = e.response?.data as { error?: string } | undefined;
        error.value = data?.error ?? t('guest-submit-error-generic');
      }
    } else {
      error.value = t('guest-submit-error-network');
    }
  } finally {
    submitting.value = false;
  }
}

async function copyLink() {
  try {
    await navigator.clipboard.writeText(statusAbsolute.value);
    copied.value = true;
    window.setTimeout(() => (copied.value = false), 2000);
  } catch {
    // clipboard unavailable
  }
}

function submitAnother() {
  success.value = null;
  form.title = '';
  form.description = '';
  attachments.value = [];
  attachmentError.value = '';
  window.scrollTo({ top: 0, behavior: 'smooth' });
}

function formatSize(bytes: number) {
  if (bytes < 1024) return t('guest-submit-size-bytes', { bytes });
  if (bytes < 1024 * 1024) return t('guest-submit-size-kb', { value: (bytes / 1024).toFixed(1) });
  return t('guest-submit-size-mb', { value: (bytes / (1024 * 1024)).toFixed(1) });
}

function attachmentErrorMessage(e: unknown, file: File): string {
  if (axios.isAxiosError(e)) {
    const data = e.response?.data as { error?: string } | undefined;
    if (e.response?.status === 429) return t('guest-submit-attach-error-rate-limited');
    if (e.response?.status === 413)
      return t('guest-submit-attach-error-too-large-server', { name: file.name });
    if (e.response?.status === 403) return t('guest-submit-attach-error-disabled');
    return data?.error ?? t('guest-submit-attach-error-generic');
  }
  return t('guest-submit-attach-error-network');
}

// Upload a batch of files in sequence. Used by both the file picker
// (multi-select) and drag-and-drop. Each file is checked against the
// client-side count/size limits before hitting the upload endpoint; the
// backend re-validates everything (MIME, magic bytes, rate limit).
async function handleFiles(files: File[]) {
  attachmentError.value = '';
  for (const file of files) {
    if (attachments.value.length >= MAX_FILES) {
      attachmentError.value = t('guest-submit-attach-error-max', { max: MAX_FILES });
      break;
    }
    if (file.size > MAX_SIZE_BYTES) {
      attachmentError.value = t('guest-submit-attach-error-too-large', { name: file.name, size: MAX_SIZE_MB });
      continue;
    }
    uploading.value = true;
    try {
      const uploaded = await publicService.uploadAttachment(file);
      attachments.value = [...attachments.value, uploaded];
    } catch (e: unknown) {
      attachmentError.value = attachmentErrorMessage(e, file);
    } finally {
      uploading.value = false;
    }
  }
}

function onFilePick(event: Event) {
  const input = event.target as HTMLInputElement;
  const files = Array.from(input.files ?? []);
  input.value = ''; // reset so the same file can be re-picked
  if (files.length) void handleFiles(files);
}

function onDragOver() {
  if (!uploading.value && attachments.value.length < MAX_FILES) dragActive.value = true;
}

function onDragLeave() {
  dragActive.value = false;
}

function onDrop(event: DragEvent) {
  dragActive.value = false;
  if (uploading.value) return;
  const files = Array.from(event.dataTransfer?.files ?? []);
  if (files.length) void handleFiles(files);
}

function removeAttachment(id: number) {
  attachments.value = attachments.value.filter((a) => a.id !== id);
}
</script>
