<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import authService from '@/services/authService';
import { formatDate } from '@/utils/dateUtils';
import { logger } from '@/utils/logger';

interface AuthMethod {
  id: string;
  type: 'email' | 'microsoft';
  identifier: string;
  isPrimary: boolean;
  createdAt: string;
}

interface ActiveSession {
  id: number;
  device_name: string | null;
  ip_address: string | null;
  user_agent: string | null;
  location: string | null;
  created_at: string;
  last_active: string;
  expires_at: string;
  is_current: boolean;
}

// State
const authMethods = ref<AuthMethod[]>([]);
const activeSessions = ref<ActiveSession[]>([]);
const loading = ref(false);

// Computed properties
const hasMicrosoftConnection = computed(() => authMethods.value.some(m => m.type === 'microsoft'));
const microsoftMethod = computed(() => authMethods.value.find(m => m.type === 'microsoft'));

// Emits for notifications
const emit = defineEmits<{
  (e: 'success', message: string): void;
  (e: 'error', message: string): void;
}>();

// Load data on mount
onMounted(async () => {
  await loadAuthMethods();
  await loadActiveSessions();
});

// Load authentication methods from API
const loadAuthMethods = async () => {
  try {
    loading.value = true;

    const methods: AuthMethod[] = [];

    // Always include email as primary method
    methods.push({
      id: 'email-primary',
      type: 'email',
      identifier: 'Email / Password', // Email is always available
      isPrimary: true,
      createdAt: '2024-01-01'
    });

    // Load user's connected auth identities from the API
    const identities = await authService.getUserAuthIdentities();

    // Add connected OAuth providers
    identities.forEach((identity: { id: number; provider_type: string; email?: string; provider_name?: string; created_at: string }) => {
      methods.push({
        id: identity.id.toString(),
        type: identity.provider_type as 'microsoft',
        identifier: identity.email || `${identity.provider_name} Account`,
        isPrimary: false,
        createdAt: identity.created_at
      });
    });

    authMethods.value = methods;
  } catch (error) {
    logger.error('Failed to load auth methods', { error });
    // Don't show error for this as it might just mean no identities are connected
  } finally {
    loading.value = false;
  }
};

const loadActiveSessions = async () => {
  try {
    loading.value = true;
    const sessions = await authService.getSessions();
    logger.debug('Loaded active sessions', { count: sessions?.length || 0 });
    activeSessions.value = sessions;
  } catch (error) {
    logger.error('Failed to load active sessions', { error });
    emit('error', 'Failed to load active sessions');
  } finally {
    loading.value = false;
  }
};

// Auth method functions
const addAuthMethod = async (type: 'microsoft') => {
  loading.value = true;
  try {
    // Use authService to connect OAuth provider
    const data = await authService.connectOAuthProvider(type);

    if (data.auth_url) {
      // Redirect to OAuth provider - when user returns, the page will reload
      // and the new connected account will be displayed
      window.location.href = data.auth_url;
      return;
    }

    emit('success', `${type.charAt(0).toUpperCase() + type.slice(1)} account linked successfully`);
    // Reload auth methods to show the newly connected account
    await loadAuthMethods();
  } catch (err) {
    emit('error', `Failed to link ${type} account`);
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
    await loadAuthMethods();
    emit('success', 'Authentication method removed successfully');
  } catch (err) {
    // Extract error message from backend response
    const axiosError = err as { response?: { data?: { message?: string } } };
    const errorMessage = axiosError.response?.data?.message || 'Failed to remove authentication method';
    emit('error', errorMessage);
    logger.error('Failed to remove auth method', { error: err, methodId, methodType });
  } finally {
    loading.value = false;
  }
};

// Session functions
const revokeSession = async (sessionId: number) => {
  loading.value = true;
  try {
    await authService.revokeSession(sessionId);
    // Refresh sessions list
    await loadActiveSessions();
    emit('success', 'Session revoked successfully');
  } catch (err) {
    emit('error', 'Failed to revoke session');
    logger.error('Failed to revoke session', { error: err, sessionId });
  } finally {
    loading.value = false;
  }
};

const revokeAllSessions = async () => {
  loading.value = true;
  try {
    await authService.revokeAllOtherSessions();
    // Refresh sessions list
    await loadActiveSessions();
    emit('success', 'All other sessions revoked successfully');
  } catch (err) {
    emit('error', 'Failed to revoke sessions');
    logger.error('Failed to revoke all sessions', { error: err });
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
      return 'email';
  }
};
</script>

<template>
  <div class="flex flex-col gap-6">
    <!-- Authentication Methods -->
    <div class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden">
      <div class="px-4 py-3 bg-surface-alt border-b border-default">
        <h2 class="text-lg font-medium text-primary">Authentication Methods</h2>
        <p class="text-sm text-tertiary mt-1">Manage how you sign in to your account</p>
      </div>
      
      <div class="p-6 flex flex-col gap-4">
        <!-- Existing Auth Methods -->
        <div class="flex flex-col gap-3">
          <h3 class="text-sm font-medium text-primary">Connected Accounts</h3>
          <div class="flex flex-col gap-2">
            <div v-for="method in authMethods" :key="method.id" class="flex items-center justify-between p-3 bg-surface-alt rounded-lg">
              <div class="flex items-center gap-3">
                <!-- Email Icon -->
                <div v-if="getAuthMethodIcon(method.type) === 'email'" class="w-10 h-10 bg-surface-hover rounded-lg flex items-center justify-center flex-shrink-0">
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-accent" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
                  </svg>
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
                    {{ method.identifier }}
                    <span v-if="method.isPrimary" class="ml-2 px-2 py-1 bg-accent/20 text-accent rounded text-xs">Primary</span>
                  </div>
                  <div class="text-xs text-tertiary">
                    Added {{ formatDate(method.createdAt, 'MMM d, yyyy') }}
                  </div>
                </div>
              </div>
              <button
                v-if="!method.isPrimary && authMethods.length > 1"
                @click="removeAuthMethod(method.id, method.type)"
                :disabled="loading"
                class="text-status-error hover:opacity-80 text-sm font-medium disabled:opacity-50"
              >
                Remove
              </button>
            </div>
          </div>
        </div>

        <!-- Add Auth Methods -->
        <div class="flex flex-col gap-3">
          <h3 class="text-sm font-medium text-primary">Add Authentication Method</h3>
          <div class="flex flex-col gap-3">
            <button
              @click="addAuthMethod('microsoft')"
              :disabled="loading || hasMicrosoftConnection"
              class="flex items-center gap-3 p-3 bg-surface-alt hover:bg-surface-hover rounded-lg border border-subtle hover:border-default transition-colors disabled:opacity-50 w-full"
            >
              <div class="w-10 h-10 bg-surface-hover rounded-lg flex items-center justify-center flex-shrink-0">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 24 24" fill="none">
                  <rect x="4" y="4" width="7" height="7" class="fill-accent" />
                  <rect x="13" y="4" width="7" height="7" class="fill-accent" />
                  <rect x="4" y="13" width="7" height="7" class="fill-accent" />
                  <rect x="13" y="13" width="7" height="7" class="fill-accent" />
                </svg>
              </div>
              <div class="flex flex-col items-start">
                <span class="text-sm font-medium text-primary">Microsoft</span>
                <span class="text-xs text-tertiary">
                  {{ hasMicrosoftConnection ? 'Already connected' : 'Azure AD / Entra ID' }}
                </span>
              </div>
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Active Sessions -->
    <div class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden">
      <div class="px-4 py-3 bg-surface-alt border-b border-default">
        <h2 class="text-lg font-medium text-primary">Active Sessions</h2>
        <p class="text-sm text-tertiary mt-1">Manage your active login sessions</p>
      </div>
      
      <div class="p-6 flex flex-col gap-4">
        <!-- Sessions List -->
        <div class="flex flex-col gap-3">
          <div class="flex items-center justify-between">
            <h3 class="text-sm font-medium text-primary">Current Sessions</h3>
            <button
              @click="revokeAllSessions"
              :disabled="loading || activeSessions.length <= 1"
              class="text-status-error hover:opacity-80 text-sm font-medium disabled:opacity-50"
            >
              Revoke All Others
            </button>
          </div>

          <div class="flex flex-col gap-2">
            <div v-for="session in activeSessions" :key="session.id" class="flex items-center justify-between p-3 bg-surface-alt rounded-lg">
              <div class="flex items-center gap-3">
                <div class="w-10 h-10 bg-surface-hover rounded-lg flex items-center justify-center">
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-secondary" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
                  </svg>
                </div>
                <div>
                  <div class="text-sm font-medium text-primary">
                    {{ session.device_name || session.user_agent || 'Unknown Device' }}
                    <span v-if="session.is_current" class="ml-2 px-2 py-1 bg-status-success/20 text-status-success rounded text-xs">Current</span>
                  </div>
                  <div class="text-xs text-tertiary">
                    {{ session.location || session.ip_address || 'Unknown location' }} • Last active {{ formatDate(session.last_active, 'MMM d, yyyy') }}
                  </div>
                </div>
              </div>
              <button
                v-if="!session.is_current"
                @click="revokeSession(session.id)"
                :disabled="loading"
                class="text-status-error hover:opacity-80 text-sm font-medium disabled:opacity-50"
              >
                Revoke
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template> 