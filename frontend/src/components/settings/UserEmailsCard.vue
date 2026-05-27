<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import userService from '@/services/userService';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import { extractErrorMessage } from '@/utils/errors';
import Spinner from '@/components/common/Spinner.vue';
import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

// Props
const props = withDefaults(defineProps<{
  userUuid: string;
  canEdit?: boolean;
}>(), {
  canEdit: false
});

// Emit events
const emit = defineEmits<{
  (e: 'success', message: string): void;
  (e: 'error', message: string): void;
}>();

// State
const userEmails = ref<any[]>([]);
const loading = ref(false);
const addingEmail = ref(false);
const newEmailAddress = ref('');
const showAddForm = ref(false);

// Fetch user emails
const fetchUserEmails = async () => {
  if (!props.userUuid) return;

  loading.value = true;
  try {
    const emails = await userService.getUserEmails(props.userUuid);
    userEmails.value = emails || [];
  } catch (error) {
    console.error(`Error fetching emails for user with UUID ${props.userUuid}:`, error);
    userEmails.value = [];
  } finally {
    loading.value = false;
  }
};

// Add new email
const addEmail = async () => {
  if (!newEmailAddress.value.trim()) {
    emit('error', t('settings-emails-error-required'));
    return;
  }

  // Basic email validation
  if (!newEmailAddress.value.includes('@') || !newEmailAddress.value.includes('.')) {
    emit('error', t('settings-emails-error-invalid-format'));
    return;
  }

  addingEmail.value = true;
  try {
    const addedEmail = await userService.addUserEmail(props.userUuid, newEmailAddress.value.trim());
    if (addedEmail) {
      emit('success', t('settings-emails-add-success'));
      newEmailAddress.value = '';
      showAddForm.value = false;
      await fetchUserEmails(); // Refresh list
    }
  } catch (error) {
    const message = extractErrorMessage(error, t('settings-emails-add-error'));
    emit('error', message);
  } finally {
    addingEmail.value = false;
  }
};

// Set email as primary
const setAsPrimary = async (emailId: number, emailAddress: string) => {
  try {
    await userService.updateUserEmail(props.userUuid, emailId, { is_primary: true });
    emit('success', t('settings-emails-set-primary-success', { email: emailAddress }));
    await fetchUserEmails(); // Refresh list
  } catch (error) {
    const message = extractErrorMessage(error, t('settings-emails-set-primary-error'));
    emit('error', message);
  }
};

// Delete email
const pendingDeleteEmail = ref<{ id: number; address: string } | null>(null);

const deleteEmail = (emailId: number, emailAddress: string) => {
  pendingDeleteEmail.value = { id: emailId, address: emailAddress };
};

const doDeleteEmail = async () => {
  const target = pendingDeleteEmail.value;
  pendingDeleteEmail.value = null;
  if (!target) return;
  try {
    await userService.deleteUserEmail(props.userUuid, target.id);
    emit('success', t('settings-emails-delete-success'));
    await fetchUserEmails(); // Refresh list
  } catch (error) {
    const message = extractErrorMessage(error, t('settings-emails-delete-error'));
    emit('error', message);
  }
};

// Cancel adding email
const cancelAdd = () => {
  showAddForm.value = false;
  newEmailAddress.value = '';
};

// Localized confirm-modal message for pending deletion
const confirmDeleteMessage = computed(() =>
  pendingDeleteEmail.value
    ? t('settings-emails-confirm-message', { email: pendingDeleteEmail.value.address })
    : '',
);

// Watch for userUuid changes
watch(() => props.userUuid, () => {
  fetchUserEmails();
}, { immediate: true });
</script>

<template>
  <SectionCard content-padding="p-4 sm:p-6">
    <template #title>{{ $t('settings-emails-section-title') }}</template>
    <template #headerActions>
      <Button
        v-if="canEdit && !showAddForm"
        size="sm"
        icon="add"
        @click="showAddForm = true"
      >
        {{ $t('settings-emails-add-button') }}
      </Button>
    </template>

    <!-- Add Email Form -->
    <div v-if="showAddForm && canEdit" class="mb-4 p-4 bg-surface-alt rounded-lg border border-subtle">
        <h3 class="text-sm font-medium text-primary mb-3">{{ $t('settings-emails-add-form-title') }}</h3>
        <div class="flex flex-col sm:flex-row gap-3">
          <FormInput
            v-model="newEmailAddress"
            type="email"
            class="flex-1"
            :placeholder="$t('settings-emails-add-placeholder')"
            @keyup.enter="addEmail"
          />
          <div class="flex gap-2">
            <Button :loading="addingEmail" @click="addEmail">
              {{ $t('settings-emails-add-submit') }}
            </Button>
            <Button variant="secondary" @click="cancelAdd">
              {{ $t('settings-emails-add-cancel') }}
            </Button>
          </div>
        </div>
      </div>

      <!-- Loading state -->
      <div v-if="loading" class="flex justify-center py-8 text-accent">
        <Spinner size="lg" />
      </div>

      <!-- Empty state -->
      <div v-else-if="userEmails.length === 0" class="text-tertiary text-sm py-4">
        {{ $t('settings-emails-empty') }}
      </div>

      <!-- Email list -->
      <div v-else class="flex flex-col gap-3">
        <div
          v-for="email in userEmails"
          :key="email.id"
          class="bg-surface-alt p-4 rounded-lg hover:bg-surface-hover/70 transition-colors"
        >
          <div class="flex items-start justify-between gap-4">
            <!-- Email info -->
            <div class="flex-1 min-w-0">
              <!-- Email address with badges -->
              <div class="flex items-center gap-2 flex-wrap mb-2">
                <span class="font-medium text-primary truncate">
                  {{ email.email }}
                </span>
                <span
                  v-if="email.is_primary"
                  class="text-xs px-2 py-0.5 rounded-full bg-accent/20 text-accent flex-shrink-0"
                >
                  {{ $t('settings-emails-primary-badge') }}
                </span>
              </div>

              <!-- Metadata -->
              <div class="flex items-center gap-2 text-sm">
                <span class="text-tertiary capitalize">
                  {{ email.email_type || $t('settings-emails-type-personal') }}
                </span>
                <span v-if="email.source" class="text-border-default">•</span>
                <span v-if="email.source" class="text-xs text-tertiary capitalize">
                  {{ email.source }}
                </span>
              </div>
            </div>

            <!-- Verified badge -->
            <div class="flex-shrink-0">
              <span
                class="text-xs px-2 py-1 rounded-full"
                :class="{
                  'text-status-success bg-status-success/20': email.is_verified,
                  'text-status-warning bg-status-warning/20': !email.is_verified
                }"
              >
                {{ email.is_verified ? $t('settings-emails-verified-badge') : $t('settings-emails-unverified-badge') }}
              </span>
            </div>
          </div>

          <!-- Edit actions (only when canEdit is true) -->
          <div v-if="canEdit && email.id !== 0 && !email.is_primary" class="mt-3 flex flex-wrap gap-2">
            <Button variant="secondary" size="sm" @click="setAsPrimary(email.id, email.email)">
              {{ $t('settings-emails-set-primary') }}
            </Button>
            <Button variant="ghost-danger" size="sm" @click="deleteEmail(email.id, email.email)">
              {{ $t('settings-emails-remove') }}
            </Button>
          </div>
        </div>
      </div>

    <ConfirmModal
      :show="pendingDeleteEmail !== null"
      variant="danger"
      :title="$t('settings-emails-confirm-title')"
      :message="confirmDeleteMessage"
      :confirm-label="$t('settings-emails-confirm-label')"
      @confirm="doDeleteEmail"
      @close="pendingDeleteEmail = null"
    />
  </SectionCard>
</template>
