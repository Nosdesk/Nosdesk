<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { useFluent } from 'fluent-vue';
import { useAuthStore } from '@/stores/auth';
import { usePasskeys } from '@/composables/usePasskeys';
import type { PasskeyInfo } from '@/services/passkeyService';
import userService from '@/services/userService';
import Icon from '@/components/common/Icon.vue';
import Spinner from '@/components/common/Spinner.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';
import Modal from '@/components/Modal.vue';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

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
    emit('success', t('settings-passkey-admin-delete-success'));
  } catch (err) {
    const axiosError = err as { response?: { data?: { message?: string } } };
    emit('error', axiosError.response?.data?.message || t('settings-passkey-admin-delete-error'));
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
      emit('error', t('settings-passkey-admin-load-error'));
    } finally {
      adminLoading.value = false;
    }
  } else {
    await loadPasskeys();
  }
});
</script>

<template>
  <SectionCard content-padding="p-4 sm:p-6">
    <template #title>{{ $t('settings-passkey-section-title') }}</template>

    <div>
      <!-- Admin viewing another user: read-only passkey list -->
      <template v-if="isManagingOtherUser">
        <div v-if="adminLoading" class="flex items-center justify-center py-8 text-accent">
          <Spinner size="lg" />
        </div>

        <div v-else-if="adminPasskeys.length === 0" class="py-2">
          <p class="text-sm text-secondary">{{ $t('settings-passkey-empty-title') }}</p>
          <p class="text-xs text-tertiary mt-0.5">{{ $t('settings-passkey-empty-admin-description') }}</p>
        </div>

        <div v-else class="flex flex-col gap-4">
          <div
            v-for="passkey in adminPasskeys"
            :key="passkey.id"
            class="flex items-center justify-between p-4 bg-surface-alt rounded-lg border border-subtle"
          >
            <div class="flex items-center gap-4">
              <div class="flex-shrink-0 w-10 h-10 rounded-full bg-accent/10 flex items-center justify-center">
                <Icon name="key" size="md" class="text-accent" />
              </div>
              <div>
                <div class="flex items-center gap-2">
                  <p class="font-medium text-primary">{{ passkey.name }}</p>
                  <span v-if="passkey.backup_eligible" class="inline-flex items-center gap-1 text-xs text-status-success">
                    <Icon name="check" size="xs" />
                    {{ $t('settings-passkey-synced-badge') }}
                  </span>
                </div>
                <p class="text-xs text-tertiary mt-0.5">
                  {{ passkey.last_used_at ? $t('settings-passkey-last-used', { date: formatAdminDate(passkey.last_used_at) ?? '' }) : $t('settings-passkey-never-used') }}
                </p>
              </div>
            </div>
            <button
              type="button"
              @click="openAdminDeleteModal(passkey)"
              class="p-2 text-tertiary hover:text-status-error hover:bg-status-error/10 rounded-lg transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              :title="$t('settings-passkey-delete-tooltip')"
              :aria-label="$t('settings-passkey-delete-tooltip')"
            >
              <Icon name="trash" />
            </button>
          </div>

          <div class="flex items-start gap-3 text-secondary">
            <span class="text-tertiary flex-shrink-0 mt-0.5 inline-flex">
              <Icon name="info" size="md" />
            </span>
            <p class="text-sm">{{ $t('settings-passkey-admin-info') }}</p>
          </div>
        </div>
      </template>

      <!-- Self mode: full passkey management -->
      <template v-else>
        <!-- Browser not supported warning -->
        <div v-if="!isSupported" class="bg-status-warning/10 border border-status-warning/20 rounded-lg p-4">
          <div class="flex items-start gap-3">
            <span class="text-status-warning flex-shrink-0 mt-0.5 inline-flex">
              <Icon name="warning" size="md" />
            </span>
            <div>
              <p class="text-status-warning font-medium">{{ $t('settings-passkey-unsupported-title') }}</p>
              <p class="text-sm text-tertiary mt-1">
                {{ $t('settings-passkey-unsupported-description') }}
              </p>
            </div>
          </div>
        </div>

        <!-- Loading state -->
        <div v-else-if="loading" class="flex items-center justify-center py-8 text-accent">
          <Spinner size="lg" />
        </div>

        <!-- No passkeys -->
        <div v-else-if="!hasPasskeys" class="flex items-center justify-between gap-4 py-1">
          <div>
            <p class="text-sm text-secondary">{{ $t('settings-passkey-empty-title') }}</p>
            <p class="text-xs text-tertiary mt-0.5">{{ $t('settings-passkey-empty-self-description') }}</p>
          </div>
          <Button class="flex-shrink-0" @click="showAddModal = true">
            {{ $t('settings-passkey-add-button') }}
          </Button>
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
                <Icon name="key" size="md" class="text-accent" />
              </div>
              <div>
                <div class="flex items-center gap-2">
                  <p class="font-medium text-primary">{{ passkey.name }}</p>
                  <span v-if="passkey.backup_eligible" class="inline-flex items-center gap-1 text-xs text-status-success">
                    <Icon name="check" size="xs" />
                    {{ $t('settings-passkey-synced-badge') }}
                  </span>
                </div>
                <p class="text-xs text-tertiary mt-0.5">
                  {{ passkey.last_used_at ? $t('settings-passkey-last-used', { date: formatDate(passkey.last_used_at) }) : $t('settings-passkey-never-used') }}
                </p>
              </div>
            </div>
            <div class="flex items-center gap-2">
              <button
                type="button"
                @click="openRenameModal(passkey)"
                class="p-2 text-tertiary hover:text-primary hover:bg-surface rounded-lg transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                :title="$t('settings-passkey-rename-tooltip')"
                :aria-label="$t('settings-passkey-rename-tooltip')"
              >
                <Icon name="rename" />
              </button>
              <button
                type="button"
                @click="openDeleteModal(passkey)"
                class="p-2 text-tertiary hover:text-status-error hover:bg-status-error/10 rounded-lg transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                :title="$t('settings-passkey-delete-tooltip')"
                :aria-label="$t('settings-passkey-delete-tooltip')"
              >
                <Icon name="trash" />
              </button>
            </div>
          </div>

          <!-- Add Passkey button at bottom of list -->
          <button
            v-if="canAddPasskey"
            @click="showAddModal = true"
            class="flex items-center justify-center gap-2 p-4 border-2 border-dashed border-subtle hover:border-accent rounded-lg text-secondary hover:text-accent transition-colors"
          >
            <Icon name="add" size="md" />
            <span class="font-medium">{{ $t('settings-passkey-add-another-button') }}</span>
          </button>
        </div>
      </template>
    </div>
  </SectionCard>

  <!-- Add Passkey Modal -->
  <Modal :show="showAddModal" :title="$t('settings-passkey-add-modal-title')" size="sm" @close="closeModals">
    <p class="text-sm text-tertiary mb-4">
      {{ $t('settings-passkey-add-modal-description') }}
    </p>
    <FormInput
      v-model="newPasskeyName"
      :label="$t('settings-passkey-add-modal-name-label')"
      :placeholder="$t('settings-passkey-add-modal-name-placeholder')"
      maxlength="100"
      @keyup.enter="handleAddPasskey"
    />
    <template #footer>
      <div class="flex justify-end gap-3">
        <Button variant="ghost" @click="closeModals">
          {{ $t('settings-passkey-modal-cancel') }}
        </Button>
        <Button :loading="registering" @click="handleAddPasskey">
          {{ $t('settings-passkey-add-modal-create') }}
        </Button>
      </div>
    </template>
  </Modal>

  <!-- Rename Modal -->
  <Modal :show="showRenameModal" :title="$t('settings-passkey-rename-modal-title')" size="sm" @close="closeModals">
    <FormInput
      v-model="renameValue"
      :label="$t('settings-passkey-rename-modal-name-label')"
      :placeholder="$t('settings-passkey-rename-modal-placeholder')"
      maxlength="100"
      @keyup.enter="handleRenamePasskey"
    />
    <template #footer>
      <div class="flex justify-end gap-3">
        <Button variant="ghost" @click="closeModals">
          {{ $t('settings-passkey-modal-cancel') }}
        </Button>
        <Button :disabled="loading || !renameValue.trim()" @click="handleRenamePasskey">
          {{ $t('settings-passkey-rename-modal-save') }}
        </Button>
      </div>
    </template>
  </Modal>

  <!-- Delete Modal -->
  <Modal :show="showDeleteModal" :title="$t('settings-passkey-delete-modal-title')" size="sm" @close="closeModals">
    <p class="text-sm text-tertiary mb-4">
      {{ $t('settings-passkey-delete-modal-confirm-prefix') }} <strong class="text-primary">{{ selectedPasskey?.name }}</strong>{{ $t('settings-passkey-delete-modal-confirm-suffix') }}
    </p>
    <FormInput
      v-model="deletePassword"
      type="password"
      :label="$t('settings-passkey-delete-modal-password-label')"
      :placeholder="$t('settings-passkey-delete-modal-password-placeholder')"
      autocomplete="current-password"
    />
    <template #footer>
      <div class="flex justify-end gap-3">
        <Button variant="ghost" @click="closeModals">
          {{ $t('settings-passkey-modal-cancel') }}
        </Button>
        <Button variant="danger" :disabled="loading || !deletePassword" @click="handleDeletePasskey">
          {{ $t('settings-passkey-delete-modal-confirm') }}
        </Button>
      </div>
    </template>
  </Modal>

  <!-- Admin Delete Passkey Modal -->
  <Modal :show="showAdminDeleteModal" :title="$t('settings-passkey-delete-modal-title')" size="sm" @close="closeModals">
    <p class="text-sm text-tertiary">
      {{ $t('settings-passkey-admin-delete-modal-confirm-prefix') }} <strong class="text-primary">{{ adminDeleteTarget?.name }}</strong>{{ $t('settings-passkey-admin-delete-modal-confirm-suffix') }}
    </p>
    <template #footer>
      <div class="flex justify-end gap-3">
        <Button variant="ghost" @click="closeModals">
          {{ $t('settings-passkey-modal-cancel') }}
        </Button>
        <Button variant="danger" :loading="adminDeleting" @click="handleAdminDeletePasskey">
          {{ $t('settings-passkey-delete-modal-confirm') }}
        </Button>
      </div>
    </template>
  </Modal>
</template>