<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { useAuthStore } from '@/stores/auth';
import { usePasskeys } from '@/composables/usePasskeys';
import type { PasskeyInfo } from '@/services/passkeyService';
import userService from '@/services/userService';

const props = defineProps<{
  targetUserUuid?: string;
}>();

// Emits for notifications
const emit = defineEmits<{
  (e: 'success', message: string): void;
  (e: 'error', message: string): void;
}>();

const authStore = useAuthStore();

const isManagingOtherUser = computed(() => {
  return !!props.targetUserUuid && props.targetUserUuid !== authStore.user?.uuid;
});

// Use passkeys composable (for self mode)
const {
  loading,
  registering,
  passkeys,
  error,
  successMessage,
  isSupported,
  hasPasskeys,
  canAddPasskey,
  loadPasskeys,
  registerPasskey,
  renamePasskey,
  deletePasskey,
  formatDate,
  clearMessages,
} = usePasskeys();

// Admin mode state
const adminPasskeys = ref<PasskeyInfo[]>([]);
const adminLoading = ref(false);
const showAdminDeleteModal = ref(false);
const adminDeleteTarget = ref<PasskeyInfo | null>(null);
const adminDeleting = ref(false);

// Local state for modals
const showAddModal = ref(false);
const showDeleteModal = ref(false);
const showRenameModal = ref(false);
const newPasskeyName = ref('');
const selectedPasskey = ref<PasskeyInfo | null>(null);
const deletePassword = ref('');
const renameValue = ref('');

// Watch for messages and emit them
const emitMessages = () => {
  if (successMessage.value) {
    emit('success', successMessage.value);
    clearMessages();
  }
  if (error.value) {
    emit('error', error.value);
    clearMessages();
  }
};

// Add passkey
const handleAddPasskey = async () => {
  const success = await registerPasskey(newPasskeyName.value || undefined);
  emitMessages();
  if (success) {
    showAddModal.value = false;
    newPasskeyName.value = '';
  }
};

// Open rename modal
const openRenameModal = (passkey: PasskeyInfo) => {
  selectedPasskey.value = passkey;
  renameValue.value = passkey.name;
  showRenameModal.value = true;
};

// Rename passkey
const handleRenamePasskey = async () => {
  if (!selectedPasskey.value) return;
  const success = await renamePasskey(selectedPasskey.value.id, renameValue.value);
  emitMessages();
  if (success) {
    showRenameModal.value = false;
    selectedPasskey.value = null;
    renameValue.value = '';
  }
};

// Open delete modal
const openDeleteModal = (passkey: PasskeyInfo) => {
  selectedPasskey.value = passkey;
  deletePassword.value = '';
  showDeleteModal.value = true;
};

// Delete passkey
const handleDeletePasskey = async () => {
  if (!selectedPasskey.value) return;
  const success = await deletePasskey(selectedPasskey.value.id, deletePassword.value);
  emitMessages();
  if (success) {
    showDeleteModal.value = false;
    selectedPasskey.value = null;
    deletePassword.value = '';
  }
};

// Admin: delete a passkey for the target user
const openAdminDeleteModal = (passkey: PasskeyInfo) => {
  adminDeleteTarget.value = passkey;
  showAdminDeleteModal.value = true;
};

const handleAdminDeletePasskey = async () => {
  if (!adminDeleteTarget.value || !props.targetUserUuid) return;
  adminDeleting.value = true;
  try {
    await userService.adminDeleteUserPasskey(props.targetUserUuid, adminDeleteTarget.value.id);
    adminPasskeys.value = adminPasskeys.value.filter(p => p.id !== adminDeleteTarget.value!.id);
    showAdminDeleteModal.value = false;
    adminDeleteTarget.value = null;
    emit('success', 'Passkey has been deleted');
  } catch (err) {
    const axiosError = err as { response?: { data?: { message?: string } } };
    emit('error', axiosError.response?.data?.message || 'Failed to delete passkey');
  } finally {
    adminDeleting.value = false;
  }
};

// Close modals
const closeModals = () => {
  showAddModal.value = false;
  showDeleteModal.value = false;
  showRenameModal.value = false;
  showAdminDeleteModal.value = false;
  selectedPasskey.value = null;
  adminDeleteTarget.value = null;
  newPasskeyName.value = '';
  deletePassword.value = '';
  renameValue.value = '';
};

// Format date helper for admin mode
const formatAdminDate = (dateStr: string | null) => {
  if (!dateStr) return null;
  try {
    return new Date(dateStr).toLocaleDateString('en-US', { year: 'numeric', month: 'short', day: 'numeric' });
  } catch {
    return dateStr;
  }
};

// Load passkeys on mount
onMounted(async () => {
  if (isManagingOtherUser.value && props.targetUserUuid) {
    adminLoading.value = true;
    try {
      const info = await userService.getUserSecurityInfo(props.targetUserUuid);
      adminPasskeys.value = info.passkeys.map(p => ({
        id: p.id,
        name: p.name,
        created_at: p.created_at,
        last_used_at: p.last_used_at,
        transports: p.transports,
        backup_eligible: p.backup_eligible,
      }));
    } catch {
      emit('error', 'Failed to load passkeys for this user');
    } finally {
      adminLoading.value = false;
    }
  } else {
    await loadPasskeys();
  }
});
</script>

<template>
  <div class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden">
    <div class="px-4 py-3 bg-surface-alt border-b border-default">
      <h2 class="text-lg font-medium text-primary">Passkeys</h2>
      <p class="text-sm text-tertiary mt-1">
        {{ isManagingOtherUser ? "Passkeys registered to this user's account" : "Sign in securely without a password using biometrics or security keys" }}
      </p>
    </div>

    <div class="p-4">
      <!-- Admin viewing another user: read-only passkey list -->
      <template v-if="isManagingOtherUser">
        <div v-if="adminLoading" class="flex items-center justify-center py-8">
          <div class="animate-spin h-8 w-8 border-2 border-accent border-t-transparent rounded-full"></div>
        </div>

        <div v-else-if="adminPasskeys.length === 0" class="py-2">
          <p class="text-sm text-secondary">No passkeys registered</p>
          <p class="text-xs text-tertiary mt-0.5">This user has not registered any passkeys.</p>
        </div>

        <div v-else class="flex flex-col gap-4">
          <div
            v-for="passkey in adminPasskeys"
            :key="passkey.id"
            class="flex items-center justify-between p-4 bg-surface-alt rounded-lg border border-subtle"
          >
            <div class="flex items-center gap-4">
              <div class="flex-shrink-0 w-10 h-10 rounded-full bg-accent/10 flex items-center justify-center">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-accent" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z" />
                </svg>
              </div>
              <div>
                <div class="flex items-center gap-2">
                  <p class="font-medium text-primary">{{ passkey.name }}</p>
                  <span v-if="passkey.backup_eligible" class="inline-flex items-center gap-1 text-xs text-status-success">
                    <svg xmlns="http://www.w3.org/2000/svg" class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
                    </svg>
                    Synced
                  </span>
                </div>
                <p class="text-xs text-tertiary mt-0.5">
                  {{ passkey.last_used_at ? `Last used ${formatAdminDate(passkey.last_used_at)}` : 'Never used' }}
                </p>
              </div>
            </div>
            <button
              @click="openAdminDeleteModal(passkey)"
              class="p-2 text-tertiary hover:text-status-error hover:bg-status-error/10 rounded-lg transition-colors"
              title="Delete passkey"
            >
              <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
              </svg>
            </button>
          </div>

          <div class="flex items-start gap-3 text-secondary">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-tertiary flex-shrink-0 mt-0.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <p class="text-sm">Passkey registration requires the account owner's biometrics or security key.</p>
          </div>
        </div>
      </template>

      <!-- Self mode: full passkey management -->
      <template v-else>
        <!-- Browser not supported warning -->
        <div v-if="!isSupported" class="bg-status-warning/10 border border-status-warning/20 rounded-lg p-4">
          <div class="flex items-start gap-3">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-status-warning flex-shrink-0 mt-0.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
            </svg>
            <div>
              <p class="text-status-warning font-medium">Browser Not Supported</p>
              <p class="text-sm text-tertiary mt-1">
                Your browser does not support passkeys (WebAuthn). Please use a modern browser like Chrome, Safari, Firefox, or Edge.
              </p>
            </div>
          </div>
        </div>

        <!-- Loading state -->
        <div v-else-if="loading" class="flex items-center justify-center py-8">
          <div class="animate-spin h-8 w-8 border-2 border-accent border-t-transparent rounded-full"></div>
        </div>

        <!-- No passkeys -->
        <div v-else-if="!hasPasskeys" class="flex items-center justify-between gap-4 py-1">
          <div>
            <p class="text-sm text-secondary">No passkeys registered</p>
            <p class="text-xs text-tertiary mt-0.5">Sign in with biometrics or a security key instead of a password</p>
          </div>
          <button
            @click="showAddModal = true"
            class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 focus:outline-none focus:ring-2 focus:ring-accent text-sm font-medium whitespace-nowrap flex-shrink-0"
          >
            Add Passkey
          </button>
        </div>

        <!-- Passkey list -->
        <div v-else class="flex flex-col gap-4">
          <div
            v-for="passkey in passkeys"
            :key="passkey.id"
            class="flex items-center justify-between p-4 bg-surface-alt rounded-lg border border-subtle hover:border-default transition-colors"
          >
            <div class="flex items-center gap-4">
              <div class="flex-shrink-0 w-10 h-10 rounded-full bg-accent/10 flex items-center justify-center">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-accent" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z" />
                </svg>
              </div>
              <div>
                <div class="flex items-center gap-2">
                  <p class="font-medium text-primary">{{ passkey.name }}</p>
                  <span v-if="passkey.backup_eligible" class="inline-flex items-center gap-1 text-xs text-status-success">
                    <svg xmlns="http://www.w3.org/2000/svg" class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
                    </svg>
                    Synced
                  </span>
                </div>
                <p class="text-xs text-tertiary mt-0.5">
                  {{ passkey.last_used_at ? `Last used ${formatDate(passkey.last_used_at)}` : 'Never used' }}
                </p>
              </div>
            </div>
            <div class="flex items-center gap-2">
              <button
                @click="openRenameModal(passkey)"
                class="p-2 text-tertiary hover:text-primary hover:bg-surface rounded-lg transition-colors"
                title="Rename passkey"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z" />
                </svg>
              </button>
              <button
                @click="openDeleteModal(passkey)"
                class="p-2 text-tertiary hover:text-status-error hover:bg-status-error/10 rounded-lg transition-colors"
                title="Delete passkey"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                </svg>
              </button>
            </div>
          </div>

          <!-- Add Passkey button at bottom of list -->
          <button
            v-if="canAddPasskey"
            @click="showAddModal = true"
            class="flex items-center justify-center gap-2 p-4 border-2 border-dashed border-subtle hover:border-accent rounded-lg text-secondary hover:text-accent transition-colors"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
            </svg>
            <span class="font-medium">Add Another Passkey</span>
          </button>
        </div>
      </template>
    </div>
  </div>

  <!-- Add Passkey Modal -->
  <Teleport to="body">
    <div v-if="showAddModal" class="fixed inset-0 z-50 flex items-center justify-center">
      <div class="absolute inset-0 bg-black/50" @click="closeModals"></div>
      <div class="relative bg-surface rounded-xl border border-default shadow-xl max-w-md w-full mx-4 p-6">
        <h3 class="text-lg font-medium text-primary mb-4">Add Passkey</h3>
        <p class="text-sm text-tertiary mb-4">
          Give your passkey a name to help you identify it later. Your device will prompt you to create the passkey.
        </p>
        <div class="mb-6">
          <label class="text-xs font-medium text-tertiary uppercase tracking-wide block mb-1.5">Passkey Name (optional)</label>
          <input
            v-model="newPasskeyName"
            type="text"
            class="w-full px-4 py-2 bg-surface-alt text-primary rounded-lg border border-subtle focus:ring-2 focus:ring-accent focus:outline-none"
            placeholder="e.g., MacBook Pro, iPhone"
            maxlength="100"
          />
        </div>
        <div class="flex justify-end gap-3">
          <button
            @click="closeModals"
            class="px-4 py-2 text-tertiary hover:text-primary rounded-lg hover:bg-surface-alt transition-colors"
          >
            Cancel
          </button>
          <button
            @click="handleAddPasskey"
            :disabled="registering"
            class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 focus:outline-none focus:ring-2 focus:ring-accent disabled:opacity-50 flex items-center gap-2"
          >
            <span v-if="registering" class="animate-spin h-4 w-4 border-2 border-white border-t-transparent rounded-full"></span>
            {{ registering ? 'Creating...' : 'Create Passkey' }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>

  <!-- Rename Modal -->
  <Teleport to="body">
    <div v-if="showRenameModal" class="fixed inset-0 z-50 flex items-center justify-center">
      <div class="absolute inset-0 bg-black/50" @click="closeModals"></div>
      <div class="relative bg-surface rounded-xl border border-default shadow-xl max-w-md w-full mx-4 p-6">
        <h3 class="text-lg font-medium text-primary mb-4">Rename Passkey</h3>
        <div class="mb-6">
          <label class="text-xs font-medium text-tertiary uppercase tracking-wide block mb-1.5">Passkey Name</label>
          <input
            v-model="renameValue"
            type="text"
            class="w-full px-4 py-2 bg-surface-alt text-primary rounded-lg border border-subtle focus:ring-2 focus:ring-accent focus:outline-none"
            placeholder="Enter new name"
            maxlength="100"
          />
        </div>
        <div class="flex justify-end gap-3">
          <button
            @click="closeModals"
            class="px-4 py-2 text-tertiary hover:text-primary rounded-lg hover:bg-surface-alt transition-colors"
          >
            Cancel
          </button>
          <button
            @click="handleRenamePasskey"
            :disabled="loading || !renameValue.trim()"
            class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 focus:outline-none focus:ring-2 focus:ring-accent disabled:opacity-50"
          >
            Save
          </button>
        </div>
      </div>
    </div>
  </Teleport>

  <!-- Delete Modal -->
  <Teleport to="body">
    <div v-if="showDeleteModal" class="fixed inset-0 z-50 flex items-center justify-center">
      <div class="absolute inset-0 bg-black/50" @click="closeModals"></div>
      <div class="relative bg-surface rounded-xl border border-default shadow-xl max-w-md w-full mx-4 p-6">
        <h3 class="text-lg font-medium text-primary mb-4">Delete Passkey</h3>
        <p class="text-sm text-tertiary mb-4">
          Are you sure you want to delete <strong class="text-primary">{{ selectedPasskey?.name }}</strong>?
          You will no longer be able to use this passkey to sign in.
        </p>
        <div class="mb-6">
          <label class="text-xs font-medium text-tertiary uppercase tracking-wide block mb-1.5">Enter your password to confirm</label>
          <input
            v-model="deletePassword"
            type="password"
            class="w-full px-4 py-2 bg-surface-alt text-primary rounded-lg border border-subtle focus:ring-2 focus:ring-accent focus:outline-none"
            placeholder="Your password"
            autocomplete="current-password"
          />
        </div>
        <div class="flex justify-end gap-3">
          <button
            @click="closeModals"
            class="px-4 py-2 text-tertiary hover:text-primary rounded-lg hover:bg-surface-alt transition-colors"
          >
            Cancel
          </button>
          <button
            @click="handleDeletePasskey"
            :disabled="loading || !deletePassword"
            class="px-4 py-2 bg-status-error text-white rounded-lg hover:opacity-90 focus:outline-none focus:ring-2 focus:ring-status-error disabled:opacity-50"
          >
            Delete Passkey
          </button>
        </div>
      </div>
    </div>
  </Teleport>

  <!-- Admin Delete Passkey Modal -->
  <Teleport to="body">
    <div v-if="showAdminDeleteModal" class="fixed inset-0 z-50 flex items-center justify-center">
      <div class="absolute inset-0 bg-black/50" @click="closeModals"></div>
      <div class="relative bg-surface rounded-xl border border-default shadow-xl max-w-md w-full mx-4 p-6">
        <h3 class="text-lg font-medium text-primary mb-4">Delete Passkey</h3>
        <p class="text-sm text-tertiary mb-6">
          Are you sure you want to delete <strong class="text-primary">{{ adminDeleteTarget?.name }}</strong>?
          This user will no longer be able to sign in with this passkey.
        </p>
        <div class="flex justify-end gap-3">
          <button
            @click="closeModals"
            class="px-4 py-2 text-tertiary hover:text-primary rounded-lg hover:bg-surface-alt transition-colors"
          >
            Cancel
          </button>
          <button
            @click="handleAdminDeletePasskey"
            :disabled="adminDeleting"
            class="px-4 py-2 bg-status-error text-white rounded-lg hover:opacity-90 focus:outline-none focus:ring-2 focus:ring-status-error disabled:opacity-50 flex items-center gap-2"
          >
            <span v-if="adminDeleting" class="animate-spin h-4 w-4 border-2 border-white border-t-transparent rounded-full"></span>
            {{ adminDeleting ? 'Deleting...' : 'Delete Passkey' }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>