<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import BackButton from '@/components/common/BackButton.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Modal from '@/components/Modal.vue';
import apiTokenService from '@/services/apiTokenService';
import userService from '@/services/userService';
import { formatDistanceToNow } from 'date-fns';
import type { ApiToken, ApiTokenCreated, CreateApiTokenRequest } from '@/types/apiToken';
import type { User } from '@/types/user';

// State
const isLoading = ref(false);
const isSaving = ref(false);
const errorMessage = ref('');
const successMessage = ref('');
const tokens = ref<ApiToken[]>([]);
const users = ref<User[]>([]);

// Modal states
const showCreateModal = ref(false);
const showRevokeConfirm = ref(false);
const showTokenCreated = ref(false);
const tokenToRevoke = ref<ApiToken | null>(null);
const createdToken = ref<ApiTokenCreated | null>(null);
const copiedToken = ref(false);

// Form state
const tokenForm = ref<CreateApiTokenRequest>({
  name: '',
  user_uuid: '',
  expires_in_days: 90,
  scopes: ['full']
});
const noExpiration = ref(false);

// Computed - active (non-revoked) tokens
const activeTokens = computed(() =>
  tokens.value.filter(t => !t.revoked_at)
);

const revokedTokens = computed(() =>
  tokens.value.filter(t => t.revoked_at)
);

// Format date helper
const formatDate = (dateStr: string | null) => {
  if (!dateStr) return 'Never';
  try {
    return formatDistanceToNow(new Date(dateStr), { addSuffix: true });
  } catch {
    return dateStr;
  }
};

// Load tokens
const loadTokens = async () => {
  isLoading.value = true;
  errorMessage.value = '';

  try {
    const result = await apiTokenService.listTokens();
    tokens.value = Array.isArray(result) ? result : [];
  } catch (error) {
    console.error('Failed to load tokens:', error);
    const axiosError = error as { response?: { data?: string } };
    errorMessage.value = axiosError.response?.data || 'Failed to load API tokens';
    tokens.value = [];
  } finally {
    isLoading.value = false;
  }
};

// Load users for the dropdown
const loadUsers = async () => {
  try {
    const result = await userService.getAllUsers();
    users.value = result;
  } catch (error) {
    console.error('Failed to load users:', error);
  }
};

// Open create token modal
const openCreateModal = () => {
  tokenForm.value = {
    name: '',
    user_uuid: '',
    expires_in_days: 90,
    scopes: ['full']
  };
  noExpiration.value = false;
  showCreateModal.value = true;
};

// Create token
const createToken = async () => {
  if (!tokenForm.value.name.trim()) {
    errorMessage.value = 'Token name is required';
    return;
  }
  if (!tokenForm.value.user_uuid) {
    errorMessage.value = 'Please select a user';
    return;
  }

  isSaving.value = true;
  errorMessage.value = '';

  try {
    const request: CreateApiTokenRequest = {
      name: tokenForm.value.name.trim(),
      user_uuid: tokenForm.value.user_uuid,
      expires_in_days: noExpiration.value ? null : tokenForm.value.expires_in_days,
      scopes: ['full']
    };

    const result = await apiTokenService.createToken(request);
    createdToken.value = result;
    showCreateModal.value = false;
    showTokenCreated.value = true;
    copiedToken.value = false;
    await loadTokens();
  } catch (error) {
    const axiosError = error as { response?: { data?: string } };
    errorMessage.value = axiosError.response?.data || 'Failed to create token';
  } finally {
    isSaving.value = false;
  }
};

// Copy token to clipboard
const copyToken = async () => {
  if (!createdToken.value?.token) return;

  try {
    await navigator.clipboard.writeText(createdToken.value.token);
    copiedToken.value = true;
    setTimeout(() => copiedToken.value = false, 2000);
  } catch (error) {
    console.error('Failed to copy token:', error);
  }
};

// Confirm revoke
const confirmRevoke = (token: ApiToken) => {
  tokenToRevoke.value = token;
  showRevokeConfirm.value = true;
};

// Revoke token
const revokeToken = async () => {
  if (!tokenToRevoke.value) return;

  isSaving.value = true;
  errorMessage.value = '';

  try {
    await apiTokenService.revokeToken(tokenToRevoke.value.uuid);
    successMessage.value = 'Token revoked successfully';
    showRevokeConfirm.value = false;
    tokenToRevoke.value = null;
    await loadTokens();

    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    const axiosError = error as { response?: { data?: string } };
    errorMessage.value = axiosError.response?.data || 'Failed to revoke token';
  } finally {
    isSaving.value = false;
  }
};

onMounted(() => {
  loadTokens();
  loadUsers();
});
</script>

<template>
  <div class="flex-1">
    <!-- Navigation and actions bar -->
    <div class="pt-4 px-4 sm:px-6 flex flex-col sm:flex-row justify-between items-start sm:items-center gap-3 sm:gap-4">
      <BackButton fallbackRoute="/admin/settings" label="Back to Administration" />
      <button
        @click="openCreateModal"
        class="px-3 py-1.5 bg-accent text-white rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors flex items-center gap-1.5"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6m0 0v6m0-6h6m-6 0H6" />
        </svg>
        <span class="hidden xs:inline">Create Token</span>
        <span class="xs:hidden">Create</span>
      </button>
    </div>

    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-2">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">API Tokens</h1>
        <p class="text-secondary text-sm sm:text-base mt-1">Manage API tokens for programmatic access</p>
      </div>

      <!-- Success message -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

      <!-- Error message -->
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Loading state -->
      <LoadingSpinner v-if="isLoading" text="Loading tokens..." />

      <!-- Tokens list -->
      <div v-else class="flex flex-col gap-4">
        <!-- Active tokens -->
        <div v-if="activeTokens.length > 0" class="flex flex-col gap-2 sm:gap-3">
          <h2 class="text-sm font-medium text-secondary uppercase tracking-wide">Active Tokens</h2>
          <div
            v-for="token in activeTokens"
            :key="token.uuid"
            class="bg-surface border border-default rounded-lg sm:rounded-xl"
          >
            <div class="p-3 sm:p-4 flex items-start gap-3 sm:gap-4">
              <!-- Key icon -->
              <div class="w-8 h-8 sm:w-10 sm:h-10 rounded-lg bg-accent/10 flex items-center justify-center flex-shrink-0">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 sm:h-5 sm:w-5 text-accent" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z" />
                </svg>
              </div>

              <!-- Token info -->
              <div class="flex-1 min-w-0">
                <div class="flex flex-col sm:flex-row sm:items-center gap-1 sm:gap-2">
                  <h3 class="font-medium text-primary text-sm sm:text-base truncate">{{ token.name }}</h3>
                  <code class="px-1.5 py-0.5 text-xs bg-surface-alt text-secondary rounded font-mono">{{ token.token_prefix }}...</code>
                </div>
                <div class="flex flex-wrap items-center gap-2 mt-1 text-xs text-secondary">
                  <span>User: {{ token.user_name }}</span>
                  <span class="text-tertiary">|</span>
                  <span>Created {{ formatDate(token.created_at) }}</span>
                  <span class="text-tertiary">|</span>
                  <span :class="token.expires_at ? '' : 'text-status-warning'">
                    {{ token.expires_at ? `Expires ${formatDate(token.expires_at)}` : 'No expiration' }}
                  </span>
                </div>
                <div class="text-xs text-tertiary mt-1">
                  Last used: {{ token.last_used_at ? formatDate(token.last_used_at) : 'Never' }}
                </div>
              </div>

              <!-- Actions -->
              <div class="flex-shrink-0">
                <button
                  @click="confirmRevoke(token)"
                  class="p-1.5 sm:p-2 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-md sm:rounded-lg transition-colors"
                  title="Revoke token"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M18.364 18.364A9 9 0 005.636 5.636m12.728 12.728A9 9 0 015.636 5.636m12.728 12.728L5.636 5.636" />
                  </svg>
                </button>
              </div>
            </div>
          </div>
        </div>

        <!-- Revoked tokens -->
        <div v-if="revokedTokens.length > 0" class="flex flex-col gap-2 sm:gap-3 mt-4">
          <h2 class="text-sm font-medium text-secondary uppercase tracking-wide">Revoked Tokens</h2>
          <div
            v-for="token in revokedTokens"
            :key="token.uuid"
            class="bg-surface border border-default rounded-lg sm:rounded-xl opacity-60"
          >
            <div class="p-3 sm:p-4 flex items-start gap-3 sm:gap-4">
              <!-- Key icon (strikethrough) -->
              <div class="w-8 h-8 sm:w-10 sm:h-10 rounded-lg bg-surface-alt flex items-center justify-center flex-shrink-0">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 sm:h-5 sm:w-5 text-secondary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z" />
                </svg>
              </div>

              <!-- Token info -->
              <div class="flex-1 min-w-0">
                <div class="flex flex-col sm:flex-row sm:items-center gap-1 sm:gap-2">
                  <h3 class="font-medium text-secondary text-sm sm:text-base truncate line-through">{{ token.name }}</h3>
                  <code class="px-1.5 py-0.5 text-xs bg-surface-alt text-tertiary rounded font-mono">{{ token.token_prefix }}...</code>
                </div>
                <div class="flex flex-wrap items-center gap-2 mt-1 text-xs text-tertiary">
                  <span>User: {{ token.user_name }}</span>
                  <span>|</span>
                  <span class="text-status-error">Revoked {{ formatDate(token.revoked_at) }}</span>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Empty state -->
        <EmptyState
          v-if="tokens.length === 0 && !isLoading"
          icon="key"
          title="No API tokens"
          description="Create an API token to enable programmatic access to the API"
          action-label="Create Token"
          variant="card"
          @action="openCreateModal"
        />
      </div>
    </div>

    <!-- Create Token Modal -->
    <Modal
      :show="showCreateModal"
      title="Create API Token"
      size="sm"
      @close="showCreateModal = false"
    >
      <form @submit.prevent="createToken" class="flex flex-col gap-4">
        <!-- Name -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">Token Name</label>
          <input
            v-model="tokenForm.name"
            type="text"
            placeholder="e.g., CI/CD Pipeline"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            required
          />
          <p class="text-xs text-tertiary mt-1">A descriptive name to identify this token</p>
        </div>

        <!-- User selection -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">User (acts as)</label>
          <select
            v-model="tokenForm.user_uuid"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            required
          >
            <option value="" disabled>Select a user...</option>
            <option v-for="user in users" :key="user.uuid" :value="user.uuid">
              {{ user.name }} ({{ user.role }})
            </option>
          </select>
          <p class="text-xs text-tertiary mt-1">The token will have the same permissions as this user</p>
        </div>

        <!-- Expiration -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">Expiration</label>
          <div class="flex items-center gap-2 mb-2">
            <input
              type="checkbox"
              id="no-expiration"
              v-model="noExpiration"
              class="rounded border-default text-accent focus:ring-accent"
            />
            <label for="no-expiration" class="text-sm text-secondary">No expiration</label>
          </div>
          <div v-if="!noExpiration" class="flex items-center gap-2">
            <input
              v-model.number="tokenForm.expires_in_days"
              type="number"
              min="1"
              max="365"
              class="w-24 px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            />
            <span class="text-sm text-secondary">days</span>
          </div>
          <p v-if="!noExpiration" class="text-xs text-tertiary mt-1">Token will expire after {{ tokenForm.expires_in_days }} days</p>
          <p v-else class="text-xs text-status-warning mt-1">Tokens without expiration are less secure</p>
        </div>

        <!-- Actions -->
        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="showCreateModal = false"
            class="px-4 py-2 text-sm text-secondary hover:text-primary transition-colors"
          >
            Cancel
          </button>
          <button
            type="submit"
            :disabled="isSaving"
            class="px-4 py-2 bg-accent text-white rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors disabled:opacity-50"
          >
            {{ isSaving ? 'Creating...' : 'Create Token' }}
          </button>
        </div>
      </form>
    </Modal>

    <!-- Token Created Modal -->
    <Modal
      :show="showTokenCreated"
      title="Token Created"
      size="sm"
      @close="showTokenCreated = false"
    >
      <div class="flex flex-col gap-4">
        <div class="flex items-center gap-2 p-3 bg-status-warning/10 border border-status-warning/20 rounded-lg">
          <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-status-warning flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
          </svg>
          <p class="text-sm text-status-warning">Copy this token now - it won't be shown again!</p>
        </div>

        <div class="relative">
          <code class="block w-full p-3 bg-surface-alt border border-default rounded-lg text-primary font-mono text-sm break-all">
            {{ createdToken?.token }}
          </code>
          <button
            @click="copyToken"
            class="absolute top-2 right-2 p-1.5 text-secondary hover:text-primary hover:bg-surface-hover rounded transition-colors"
            :title="copiedToken ? 'Copied!' : 'Copy to clipboard'"
          >
            <svg v-if="!copiedToken" xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
            </svg>
            <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 text-status-success" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
            </svg>
          </button>
        </div>

        <p class="text-xs text-tertiary">
          Use this token with the <code class="px-1 py-0.5 bg-surface-alt rounded">Authorization: Bearer &lt;token&gt;</code> header
        </p>

        <div class="flex justify-end pt-2">
          <button
            @click="showTokenCreated = false"
            class="px-4 py-2 bg-accent text-white rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors"
          >
            Done
          </button>
        </div>
      </div>
    </Modal>

    <!-- Revoke Confirmation Modal -->
    <Modal
      :show="showRevokeConfirm"
      title="Revoke Token"
      size="sm"
      @close="showRevokeConfirm = false"
    >
      <div class="flex flex-col gap-4">
        <p class="text-secondary">
          Are you sure you want to revoke the token <strong class="text-primary">{{ tokenToRevoke?.name }}</strong>?
        </p>
        <p class="text-sm text-status-error">
          This action cannot be undone. Any systems using this token will lose access.
        </p>

        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="showRevokeConfirm = false"
            class="px-4 py-2 text-sm text-secondary hover:text-primary transition-colors"
          >
            Cancel
          </button>
          <button
            @click="revokeToken"
            :disabled="isSaving"
            class="px-4 py-2 bg-status-error text-white rounded-lg text-sm hover:bg-status-error/90 font-medium transition-colors disabled:opacity-50"
          >
            {{ isSaving ? 'Revoking...' : 'Revoke Token' }}
          </button>
        </div>
      </div>
    </Modal>
  </div>
</template>
