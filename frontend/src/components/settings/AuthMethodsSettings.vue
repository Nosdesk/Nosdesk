<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { useFluent } from 'fluent-vue';
import { useAuthStore } from '@/stores/auth';
import authService from '@/services/authService';
import userService from '@/services/userService';
import { formatDate } from '@/utils/dateUtils';
import { extractErrorMessage } from '@/utils/errors';
import { logger } from '@/utils/logger';
import Icon from '@/components/common/Icon.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import Button from '@/components/common/Button.vue';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const props = defineProps<{
  targetUserUuid?: string;
}>();

interface AuthMethod {
  id: string;
  type: 'local' | 'microsoft';
  identifier: string;
  isPrimary: boolean;
  createdAt?: string;
}

// State
const authStore = useAuthStore();
const authMethods = ref<AuthMethod[]>([]);
const loading = ref(false);

const isManagingOtherUser = computed(() => {
  return !!props.targetUserUuid && props.targetUserUuid !== authStore.user?.uuid;
});

// Computed properties
const hasMicrosoftConnection = computed(() => authMethods.value.some(m => m.type === 'microsoft'));

// Emits for notifications
const emit = defineEmits<{
  (e: 'success', message: string): void;
  (e: 'error', message: string): void;
}>();

// Load data on mount
onMounted(() => loadAuthData());

const getAuthMethodLabel = (type: string) => {
  switch (type) {
    case 'local': return t('settings-auth-methods-type-local');
    case 'microsoft': return t('settings-auth-methods-type-microsoft');
    default: return type;
  }
};

// Load auth methods (works for both self and admin paths)
const loadAuthData = async () => {
  try {
    loading.value = true;

    let identities: { id: number; provider_type: string; email?: string | null; provider_name?: string; created_at: string }[];

    if (isManagingOtherUser.value && props.targetUserUuid) {
      const securityInfo = await userService.getUserSecurityInfo(props.targetUserUuid);
      identities = securityInfo.auth_identities;
    } else {
      identities = await authService.getUserAuthIdentities();
    }

    authMethods.value = identities.map((identity) => ({
      id: identity.id.toString(),
      type: identity.provider_type as AuthMethod['type'],
      identifier: identity.email || `${identity.provider_name} Account`,
      isPrimary: identity.provider_type === 'local',
      createdAt: identity.created_at,
    }));
  } catch (error) {
    logger.error('Failed to load auth methods', { error });
  } finally {
    loading.value = false;
  }
};

// Auth method functions
const addAuthMethod = async (type: 'microsoft') => {
  loading.value = true;
  const providerName = type.charAt(0).toUpperCase() + type.slice(1);
  try {
    // Use authService to connect OAuth provider
    const data = await authService.connectOAuthProvider(type);

    if (data.auth_url) {
      // Redirect to OAuth provider - when user returns, the page will reload
      // and the new connected account will be displayed
      window.location.href = data.auth_url;
      return;
    }

    emit('success', t('settings-auth-methods-link-success', { provider: providerName }));
    // Reload auth methods to show the newly connected account
    await loadAuthData();
  } catch (err) {
    emit('error', t('settings-auth-methods-link-error', { provider: providerName }));
    logger.error('Failed to link account', { error: err, type });
  } finally {
    loading.value = false;
  }
};

const removeAuthMethod = async (methodId: string, methodType: string) => {
  loading.value = true;
  try {
    if (methodType === 'microsoft') {
      // Handle Microsoft identity removal using authService
      await authService.deleteUserAuthIdentity(parseInt(methodId));
    }

    // Reload auth methods after deletion
    await loadAuthData();
    emit('success', t('settings-auth-methods-remove-success'));
  } catch (err) {
    // Extract error message from backend response
    const errorMessage = extractErrorMessage(err, t('settings-auth-methods-remove-error'));
    emit('error', errorMessage);
    logger.error('Failed to remove auth method', { error: err, methodId, methodType });
  } finally {
    loading.value = false;
  }
};

// Admin: remove an auth method for the target user
const adminRemoveAuthMethod = async (methodId: string) => {
  if (!props.targetUserUuid) return;
  loading.value = true;
  try {
    await userService.adminDeleteUserAuthIdentity(props.targetUserUuid, parseInt(methodId));
    await loadAuthData();
    emit('success', t('settings-auth-methods-remove-success'));
  } catch (err) {
    const errorMessage = extractErrorMessage(err, t('settings-auth-methods-remove-error'));
    emit('error', errorMessage);
    logger.error('Failed to remove auth method (admin)', { error: err, methodId });
  } finally {
    loading.value = false;
  }
};

// Utility functions
const getAuthMethodIcon = (type: string) => {
  switch (type) {
    case 'microsoft':
      return 'microsoft';
    default:
      return 'local';
  }
};
</script>

<template>
  <div class="flex flex-col gap-6">
    <!-- Authentication Methods -->
    <SectionCard content-padding="p-4 sm:p-6">
      <template #title>{{ $t('settings-auth-methods-section-title') }}</template>

      <div class="flex flex-col gap-3">
        <!-- Auth Methods -->
        <div class="flex flex-col gap-2">
            <div v-for="method in authMethods" :key="method.id" class="flex items-center justify-between p-3 bg-surface-alt rounded-lg">
              <div class="flex items-center gap-3">
                <!-- Email Icon -->
                <div v-if="getAuthMethodIcon(method.type) === 'local'" class="w-10 h-10 bg-surface-hover rounded-lg flex items-center justify-center flex-shrink-0">
                  <span class="text-accent inline-flex">
                    <Icon name="email" size="md" />
                  </span>
                </div>
                <!-- Microsoft Icon (4-square grid pattern) -->
                <div v-else-if="getAuthMethodIcon(method.type) === 'microsoft'" class="w-10 h-10 bg-surface-hover rounded-lg flex items-center justify-center flex-shrink-0">
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 24 24" fill="none">
                    <rect x="4" y="4" width="7" height="7" class="fill-accent" />
                    <rect x="13" y="4" width="7" height="7" class="fill-accent" />
                    <rect x="4" y="13" width="7" height="7" class="fill-accent" />
                    <rect x="13" y="13" width="7" height="7" class="fill-accent" />
                  </svg>
                </div>
                <div>
                  <div class="text-sm font-medium text-primary">
                    {{ getAuthMethodLabel(method.type) }}
                    <span v-if="method.isPrimary" class="ml-2 px-2 py-1 bg-accent/20 text-accent rounded text-xs">{{ $t('settings-auth-methods-primary-badge') }}</span>
                  </div>
                  <div v-if="method.identifier" class="text-xs text-tertiary">
                    {{ method.identifier }}<template v-if="method.createdAt"> {{ $t('settings-auth-methods-added-suffix', { date: formatDate(method.createdAt, 'MMM d, yyyy') }) }}</template>
                  </div>
                </div>
              </div>
              <Button
                v-if="!method.isPrimary && authMethods.length > 1"
                variant="ghost-danger"
                size="sm"
                :disabled="loading"
                @click="isManagingOtherUser ? adminRemoveAuthMethod(method.id) : removeAuthMethod(method.id, method.type)"
              >
                {{ $t('settings-auth-methods-remove') }}
              </Button>
            </div>
        </div>

        <!-- Add Auth Method (hidden for admin viewing another user) -->
        <button
          v-if="!isManagingOtherUser"
          @click="addAuthMethod('microsoft')"
          :disabled="loading || hasMicrosoftConnection"
          class="flex items-center gap-3 p-3 bg-surface-alt hover:bg-surface-hover rounded-lg border border-dashed border-subtle hover:border-default transition-colors disabled:opacity-50 w-full"
        >
          <div class="w-8 h-8 bg-surface-hover rounded-lg flex items-center justify-center flex-shrink-0">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 24 24" fill="none">
              <rect x="4" y="4" width="7" height="7" class="fill-accent" />
              <rect x="13" y="4" width="7" height="7" class="fill-accent" />
              <rect x="4" y="13" width="7" height="7" class="fill-accent" />
              <rect x="13" y="13" width="7" height="7" class="fill-accent" />
            </svg>
          </div>
          <div class="flex flex-col items-start">
            <span class="text-sm font-medium text-primary">{{ $t('settings-auth-methods-connect-microsoft') }}</span>
            <span class="text-xs text-tertiary">
              {{ hasMicrosoftConnection ? $t('settings-auth-methods-connect-microsoft-already') : $t('settings-auth-methods-connect-microsoft-provider') }}
            </span>
          </div>
        </button>
      </div>
    </SectionCard>
  </div>
</template>
