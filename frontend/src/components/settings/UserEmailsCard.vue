<script setup lang="ts">
import { ref, watch } from 'vue';
import userService from '@/services/userService';

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
    emit('error', 'Email address is required');
    return;
  }

  // Basic email validation
  if (!newEmailAddress.value.includes('@') || !newEmailAddress.value.includes('.')) {
    emit('error', 'Invalid email format');
    return;
  }

  addingEmail.value = true;
  try {
    const addedEmail = await userService.addUserEmail(props.userUuid, newEmailAddress.value.trim());
    if (addedEmail) {
      emit('success', 'Email address added successfully');
      newEmailAddress.value = '';
      showAddForm.value = false;
      await fetchUserEmails(); // Refresh list
    }
  } catch (error: any) {
    const message = error.response?.data?.message || 'Failed to add email address';
    emit('error', message);
  } finally {
    addingEmail.value = false;
  }
};

// Set email as primary
const setAsPrimary = async (emailId: number, emailAddress: string) => {
  try {
    await userService.updateUserEmail(props.userUuid, emailId, { is_primary: true });
    emit('success', `Set ${emailAddress} as primary email`);
    await fetchUserEmails(); // Refresh list
  } catch (error: any) {
    const message = error.response?.data?.message || 'Failed to set email as primary';
    emit('error', message);
  }
};

// Delete email
const deleteEmail = async (emailId: number, emailAddress: string) => {
  if (!confirm(`Are you sure you want to remove ${emailAddress}?`)) {
    return;
  }

  try {
    await userService.deleteUserEmail(props.userUuid, emailId);
    emit('success', 'Email address removed successfully');
    await fetchUserEmails(); // Refresh list
  } catch (error: any) {
    const message = error.response?.data?.message || 'Failed to delete email address';
    emit('error', message);
  }
};

// Cancel adding email
const cancelAdd = () => {
  showAddForm.value = false;
  newEmailAddress.value = '';
};

// Watch for userUuid changes
watch(() => props.userUuid, () => {
  fetchUserEmails();
}, { immediate: true });
</script>

<template>
  <div class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden">
    <!-- Header -->
    <div class="px-4 py-3 bg-surface-alt border-b border-default flex items-center justify-between">
      <h2 class="text-lg font-medium text-primary">Email Addresses</h2>
      <button
        v-if="canEdit && !showAddForm"
        @click="showAddForm = true"
        class="px-3 py-1.5 bg-accent text-white rounded-lg hover:opacity-90 transition-colors text-sm flex items-center gap-2"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
        </svg>
        Add Email
      </button>
    </div>

    <!-- Content -->
    <div class="p-4">
      <!-- Add Email Form -->
      <div v-if="showAddForm && canEdit" class="mb-4 p-4 bg-surface-alt rounded-lg border border-subtle">
        <h3 class="text-sm font-medium text-primary mb-3">Add New Email Address</h3>
        <div class="flex flex-col sm:flex-row gap-3">
          <input
            v-model="newEmailAddress"
            type="email"
            placeholder="email@example.com"
            class="flex-1 px-4 py-2.5 bg-surface-alt rounded-lg border border-subtle text-primary focus:ring-2 focus:ring-accent focus:outline-none"
            @keyup.enter="addEmail"
          />
          <div class="flex gap-2">
            <button
              @click="addEmail"
              :disabled="addingEmail"
              class="px-4 py-2.5 bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {{ addingEmail ? 'Adding...' : 'Add' }}
            </button>
            <button
              @click="cancelAdd"
              class="px-4 py-2.5 bg-surface-hover text-primary rounded-lg hover:bg-surface transition-colors"
            >
              Cancel
            </button>
          </div>
        </div>
      </div>

      <!-- Loading state -->
      <div v-if="loading" class="flex justify-center py-8">
        <div class="animate-spin h-8 w-8 border-4 border-accent border-t-transparent rounded-full"></div>
      </div>

      <!-- Empty state -->
      <div v-else-if="userEmails.length === 0" class="text-tertiary text-sm py-4">
        No email addresses found
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
                  Primary
                </span>
              </div>

              <!-- Metadata -->
              <div class="flex items-center gap-2 text-sm">
                <span class="text-tertiary capitalize">
                  {{ email.email_type || 'personal' }}
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
                {{ email.is_verified ? 'Verified' : 'Unverified' }}
              </span>
            </div>
          </div>

          <!-- Edit actions (only when canEdit is true) -->
          <div v-if="canEdit && email.id !== 0 && !email.is_primary" class="mt-3 flex flex-wrap gap-2">
            <button
              @click="setAsPrimary(email.id, email.email)"
              class="text-xs px-3 py-1.5 bg-accent/20 text-accent rounded-lg hover:bg-accent/30 transition-colors"
            >
              Set as Primary
            </button>
            <button
              @click="deleteEmail(email.id, email.email)"
              class="text-xs px-3 py-1.5 bg-status-error/20 text-status-error rounded-lg hover:bg-status-error/30 transition-colors"
            >
              Remove
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
