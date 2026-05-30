<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';

import AlertMessage from '@/components/common/AlertMessage.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import Icon from '@/components/common/Icon.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import FormNumber from '@/components/common/FormNumber.vue';
import Modal from '@/components/Modal.vue';
import apiTokenService from '@/services/apiTokenService';
import userService from '@/services/userService';
import { formatDistanceToNow } from 'date-fns';
import type { ApiToken, ApiTokenCreated, CreateApiTokenRequest } from '@/types/apiToken';
import type { User } from '@/types/user';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

// Token list is cached by Pinia Colada keyed here, so navigating away
// and back renders it instantly from cache and revalidates in the
// background — no cold refetch, no blank flash. A skeleton shows only
// on the genuine first load (empty cache); see `isFirstLoad`.
const API_TOKENS_KEY = ['api-tokens'] as const;
const queryCache = useQueryCache();
const tokensQuery = useQuery({
  key: API_TOKENS_KEY,
  query: () => apiTokenService.listTokens(),
});
const tokens = computed<ApiToken[]>(() =>
  Array.isArray(tokensQuery.data.value) ? tokensQuery.data.value : [],
);
// First-ever load (no cached data yet) gates the skeleton; a background
// refetch on remount keeps the cached list on screen instead.
const isFirstLoad = computed(
  () => tokensQuery.status.value === 'pending' && tokensQuery.data.value === undefined,
);
const loadError = computed(() =>
  tokensQuery.error.value ? t('admin-api-tokens-error-load') : '',
);

// Mutation feedback (create / revoke) stays in local refs.
const isSaving = ref(false);
const errorMessage = ref('');
const successMessage = ref('');
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
  if (!dateStr) return t('admin-api-tokens-last-used-never');
  try {
    return formatDistanceToNow(new Date(dateStr), { addSuffix: true });
  } catch {
    return dateStr;
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
    errorMessage.value = t('admin-api-tokens-error-name-required');
    return;
  }
  if (!tokenForm.value.user_uuid) {
    errorMessage.value = t('admin-api-tokens-error-user-required');
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
    await queryCache.invalidateQueries({ key: API_TOKENS_KEY });
  } catch (error) {
    const axiosError = error as { response?: { data?: string } };
    errorMessage.value = axiosError.response?.data || t('admin-api-tokens-error-create');
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
    successMessage.value = t('admin-api-tokens-revoke-success');
    showRevokeConfirm.value = false;
    tokenToRevoke.value = null;
    await queryCache.invalidateQueries({ key: API_TOKENS_KEY });

    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    const axiosError = error as { response?: { data?: string } };
    errorMessage.value = axiosError.response?.data || t('admin-api-tokens-error-revoke');
  } finally {
    isSaving.value = false;
  }
};

onMounted(() => {
  // The token list auto-fetches via useQuery; only the dropdown's
  // user list needs an explicit load.
  loadUsers();
});
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-2 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
        <div>
          <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('admin-api-tokens-title') }}</h1>
          <p class="text-secondary text-sm sm:text-base mt-1">{{ $t('admin-api-tokens-description') }}</p>
        </div>
        <button
          @click="openCreateModal"
          class="px-3 py-1.5 bg-accent text-white rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors flex items-center gap-1.5 self-start sm:self-auto"
        >
          <Icon name="add" />
          <span class="hidden xs:inline">{{ $t('admin-api-tokens-create') }}</span>
          <span class="xs:hidden">{{ $t('admin-api-tokens-create-short') }}</span>
        </button>
      </div>

      <!-- Success message -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

      <!-- Error message (mutation failures) -->
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Load error (initial fetch failed with no cached data) -->
      <AlertMessage v-if="loadError && tokens.length === 0" type="error" :message="loadError" />

      <!-- First-load skeleton: mirrors the token-row layout so the
           shell doesn't shift when data arrives. Only shown on a cold
           cache; remounts render cached rows instantly and revalidate
           silently in the background. -->
      <Skeleton
        v-if="isFirstLoad"
        :label="$t('admin-api-tokens-loading')"
        class="flex flex-col gap-2 sm:gap-3"
      >
        <div
          v-for="n in 3"
          :key="n"
          class="bg-surface border border-default rounded-lg sm:rounded-xl p-3 sm:p-4 flex items-start gap-3 sm:gap-4"
        >
          <SkeletonBar class="w-8 h-8 sm:w-10 sm:h-10 rounded-lg shrink-0" />
          <div class="flex-1 flex flex-col gap-2">
            <SkeletonBar class="h-4 w-40 max-w-full" />
            <SkeletonBar class="h-3 w-3/4" />
          </div>
        </div>
      </Skeleton>

      <!-- Tokens list -->
      <div v-else class="flex flex-col gap-4">
        <!-- Active tokens -->
        <div v-if="activeTokens.length > 0" class="flex flex-col gap-2 sm:gap-3">
          <h2 class="text-sm font-medium text-secondary uppercase tracking-wide">{{ $t('admin-api-tokens-active-heading') }}</h2>
          <div
            v-for="token in activeTokens"
            :key="token.uuid"
            class="bg-surface border border-default rounded-lg sm:rounded-xl"
          >
            <div class="p-3 sm:p-4 flex items-start gap-3 sm:gap-4">
              <!-- Key icon -->
              <div class="w-8 h-8 sm:w-10 sm:h-10 rounded-lg bg-accent/10 flex items-center justify-center flex-shrink-0">
                <Icon name="key" class="sm:h-5 sm:w-5 text-accent" />
              </div>

              <!-- Token info -->
              <div class="flex-1 min-w-0">
                <div class="flex flex-col sm:flex-row sm:items-center gap-1 sm:gap-2">
                  <h3 class="font-medium text-primary text-sm sm:text-base truncate">{{ token.name }}</h3>
                  <code class="px-1.5 py-0.5 text-xs bg-surface-alt text-secondary rounded font-mono">{{ token.token_prefix }}...</code>
                </div>
                <div class="flex flex-wrap items-center gap-2 mt-1 text-xs text-secondary">
                  <span>{{ $t('admin-api-tokens-user-prefix') }} {{ token.user_name }}</span>
                  <span class="text-tertiary">|</span>
                  <span>{{ t('admin-api-tokens-created-prefix', { when: formatDate(token.created_at) }) }}</span>
                  <span class="text-tertiary">|</span>
                  <span :class="token.expires_at ? '' : 'text-status-warning'">
                    {{ token.expires_at ? t('admin-api-tokens-expires-prefix', { when: formatDate(token.expires_at) }) : t('admin-api-tokens-no-expiration') }}
                  </span>
                </div>
                <div class="text-xs text-tertiary mt-1">
                  {{ $t('admin-api-tokens-last-used-label') }} {{ token.last_used_at ? formatDate(token.last_used_at) : $t('admin-api-tokens-last-used-never') }}
                </div>
              </div>

              <!-- Actions -->
              <div class="flex-shrink-0">
                <button
                  @click="confirmRevoke(token)"
                  class="p-1.5 sm:p-2 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-md sm:rounded-lg transition-colors"
                  :title="$t('admin-api-tokens-revoke-title')"
                >
                  <Icon name="close" />
                </button>
              </div>
            </div>
          </div>
        </div>

        <!-- Revoked tokens -->
        <div v-if="revokedTokens.length > 0" class="flex flex-col gap-2 sm:gap-3 mt-4">
          <h2 class="text-sm font-medium text-secondary uppercase tracking-wide">{{ $t('admin-api-tokens-revoked-heading') }}</h2>
          <div
            v-for="token in revokedTokens"
            :key="token.uuid"
            class="bg-surface border border-default rounded-lg sm:rounded-xl opacity-60"
          >
            <div class="p-3 sm:p-4 flex items-start gap-3 sm:gap-4">
              <!-- Key icon (strikethrough) -->
              <div class="w-8 h-8 sm:w-10 sm:h-10 rounded-lg bg-surface-alt flex items-center justify-center flex-shrink-0">
                <Icon name="key" class="sm:h-5 sm:w-5 text-secondary" />
              </div>

              <!-- Token info -->
              <div class="flex-1 min-w-0">
                <div class="flex flex-col sm:flex-row sm:items-center gap-1 sm:gap-2">
                  <h3 class="font-medium text-secondary text-sm sm:text-base truncate line-through">{{ token.name }}</h3>
                  <code class="px-1.5 py-0.5 text-xs bg-surface-alt text-tertiary rounded font-mono">{{ token.token_prefix }}...</code>
                </div>
                <div class="flex flex-wrap items-center gap-2 mt-1 text-xs text-tertiary">
                  <span>{{ $t('admin-api-tokens-user-prefix') }} {{ token.user_name }}</span>
                  <span>|</span>
                  <span class="text-status-error">{{ t('admin-api-tokens-revoked-prefix', { when: formatDate(token.revoked_at) }) }}</span>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Empty state -->
        <EmptyState
          v-if="tokens.length === 0 && !isFirstLoad"
          icon="key"
          :title="$t('empty-api-tokens-title')"
          :description="$t('empty-api-tokens-description')"
          :action-label="$t('admin-api-tokens-create')"
          variant="card"
          @action="openCreateModal"
        />
      </div>
    </div>

    <!-- Create Token Modal -->
    <Modal
      :show="showCreateModal"
      :title="$t('admin-api-tokens-modal-create-title')"
      size="sm"
      @close="showCreateModal = false"
    >
      <form @submit.prevent="createToken" class="flex flex-col gap-4">
        <!-- Name -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">{{ $t('admin-api-tokens-modal-name-label') }}</label>
          <input
            v-model="tokenForm.name"
            type="text"
            :placeholder="$t('admin-api-tokens-modal-name-placeholder')"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            required
          />
          <p class="text-xs text-tertiary mt-1">{{ $t('admin-api-tokens-modal-name-hint') }}</p>
        </div>

        <!-- User selection -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">{{ $t('admin-api-tokens-modal-user-label') }}</label>
          <select
            v-model="tokenForm.user_uuid"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            required
          >
            <option value="" disabled>{{ $t('admin-api-tokens-modal-user-placeholder') }}</option>
            <option v-for="user in users" :key="user.uuid" :value="user.uuid">
              {{ user.name }} ({{ user.role }})
            </option>
          </select>
          <p class="text-xs text-tertiary mt-1">{{ $t('admin-api-tokens-modal-user-hint') }}</p>
        </div>

        <!-- Expiration -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">{{ $t('admin-api-tokens-modal-expiration-label') }}</label>
          <Checkbox
            v-model="noExpiration"
            id="no-expiration"
            size="sm"
            :label="$t('admin-api-tokens-modal-no-expiration-label')"
            class="mb-2"
          />
          <div v-if="!noExpiration" class="flex items-center gap-2">
            <FormNumber
              :model-value="tokenForm.expires_in_days ?? null"
              size="sm"
              integer
              :min="1"
              :max="365"
              class="w-40 shrink-0"
              @update:model-value="(v) => (tokenForm.expires_in_days = v ?? 90)"
            />
            <span class="text-sm text-secondary">{{ $t('admin-api-tokens-modal-expires-days-suffix') }}</span>
          </div>
          <p v-if="!noExpiration" class="text-xs text-tertiary mt-1">{{ t('admin-api-tokens-modal-expires-hint', { days: tokenForm.expires_in_days ?? 0 }) }}</p>
          <p v-else class="text-xs text-status-warning mt-1">{{ $t('admin-api-tokens-modal-no-expiration-warning') }}</p>
        </div>

        <!-- Actions -->
        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="showCreateModal = false"
            class="px-4 py-2 text-sm text-secondary hover:text-primary transition-colors"
          >
            {{ $t('admin-api-tokens-modal-cancel') }}
          </button>
          <button
            type="submit"
            :disabled="isSaving"
            class="px-4 py-2 bg-accent text-white rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors disabled:opacity-50"
          >
            {{ isSaving ? $t('admin-api-tokens-modal-creating') : $t('admin-api-tokens-create') }}
          </button>
        </div>
      </form>
    </Modal>

    <!-- Token Created Modal -->
    <Modal
      :show="showTokenCreated"
      :title="$t('admin-api-tokens-created-title')"
      size="sm"
      @close="showTokenCreated = false"
    >
      <div class="flex flex-col gap-4">
        <div class="flex items-center gap-2 p-3 bg-status-warning/10 border border-status-warning/20 rounded-lg">
          <Icon name="warning" size="md" class="text-status-warning flex-shrink-0" />
          <p class="text-sm text-status-warning">{{ $t('admin-api-tokens-created-warning') }}</p>
        </div>

        <div class="relative">
          <code class="block w-full p-3 bg-surface-alt border border-default rounded-lg text-primary font-mono text-sm break-all">
            {{ createdToken?.token }}
          </code>
          <button
            @click="copyToken"
            class="absolute top-2 right-2 p-1.5 text-secondary hover:text-primary hover:bg-surface-hover rounded transition-colors"
            :title="copiedToken ? $t('admin-api-tokens-copied') : $t('admin-api-tokens-copy-title')"
          >
            <Icon v-if="!copiedToken" name="copy" />
            <Icon v-else name="check" class="text-status-success" />
          </button>
        </div>

        <p class="text-xs text-tertiary">
          {{ $t('admin-api-tokens-bearer-hint-prefix') }} <code class="px-1 py-0.5 bg-surface-alt rounded">Authorization: Bearer &lt;token&gt;</code> {{ $t('admin-api-tokens-bearer-hint-suffix') }}
        </p>

        <div class="flex justify-end pt-2">
          <button
            @click="showTokenCreated = false"
            class="px-4 py-2 bg-accent text-white rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors"
          >
            {{ $t('admin-api-tokens-done') }}
          </button>
        </div>
      </div>
    </Modal>

    <!-- Revoke Confirmation Modal -->
    <Modal
      :show="showRevokeConfirm"
      :title="$t('admin-api-tokens-revoke-modal-title')"
      size="sm"
      @close="showRevokeConfirm = false"
    >
      <div class="flex flex-col gap-4">
        <!-- One-string confirm prompt with { $name } interpolation.
             The previous prefix/suffix split was English-only — French
             and Dutch put the name at a different sentence position. -->
        <p class="text-secondary">
          {{ t('admin-api-tokens-revoke-confirm-message', { name: tokenToRevoke?.name ?? '' }) }}
        </p>
        <p class="text-sm text-status-error">
          {{ $t('admin-api-tokens-revoke-warning') }}
        </p>

        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="showRevokeConfirm = false"
            class="px-4 py-2 text-sm text-secondary hover:text-primary transition-colors"
          >
            {{ $t('admin-api-tokens-modal-cancel') }}
          </button>
          <button
            @click="revokeToken"
            :disabled="isSaving"
            class="px-4 py-2 bg-status-error text-white rounded-lg text-sm hover:bg-status-error/90 font-medium transition-colors disabled:opacity-50"
          >
            {{ isSaving ? $t('admin-api-tokens-revoking') : $t('admin-api-tokens-revoke-modal-title') }}
          </button>
        </div>
      </div>
    </Modal>
  </div>
</template>
