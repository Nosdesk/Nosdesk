<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import authService from '@/services/authService';

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

    // Load OAuth providers from the API using authService
    const providers = await authService.getAuthProviders();

    const methods: AuthMethod[] = [];

    // Always include email as primary method
    methods.push({
      id: 'email-primary',
      type: 'email',
      identifier: 'user@example.com', // TODO: Get from user profile
      isPrimary: true,
      createdAt: '2024-01-01'
    });

    // Add Microsoft provider if it exists and is enabled
    const microsoftProvider = providers.find((p: any) => p.provider_type === 'microsoft' && p.enabled);
    if (microsoftProvider) {
      methods.push({
        id: microsoftProvider.id,
        type: 'microsoft',
        identifier: 'Microsoft Account',
        isPrimary: false,
        createdAt: microsoftProvider.created_at
      });
    }

    authMethods.value = methods;
  } catch (error) {
    console.error('Failed to load auth methods:', error);
    // Don't show error for this as it might just mean no providers are configured
  } finally {
    loading.value = false;
  }
};

const loadActiveSessions = async () => {
  try {
    loading.value = true;
    const sessions = await authService.getSessions();
    console.log('📋 Loaded sessions:', sessions);
    console.log('📋 Sessions count:', sessions?.length || 0);
    activeSessions.value = sessions;
  } catch (error) {
    console.error('Failed to load active sessions:', error);
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
      // Redirect to OAuth provider
      window.location.href = data.auth_url;
      return;
    }

    emit('success', `${type.charAt(0).toUpperCase() + type.slice(1)} account linked successfully`);
  } catch (err) {
    emit('error', `Failed to link ${type} account`);
    console.error(`Error linking ${type} account:`, err);
  } finally {
    loading.value = false;
  }
};

const removeAuthMethod = async (methodId: string, methodType: string) => {
  if (authMethods.value.length <= 1) {
    emit('error', 'You must have at least one authentication method');
    return;
  }

  loading.value = true;
  try {
    if (methodType === 'microsoft') {
      // Handle Microsoft provider removal using authService
      await authService.deleteAuthProvider(methodId);
    }

    authMethods.value = authMethods.value.filter(method => method.id !== methodId);
    emit('success', 'Authentication method removed successfully');
  } catch (err) {
    emit('error', 'Failed to remove authentication method');
    console.error('Error removing auth method:', err);
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
    console.error('Error revoking session:', err);
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
    console.error('Error revoking all sessions:', err);
  } finally {
    loading.value = false;
  }
};

// Utility functions
const formatDate = (dateString: string) => {
  return new Date(dateString).toLocaleDateString('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric'
  });
};

const getAuthMethodIcon = (type: string) => {
  switch (type) {
    case 'microsoft':
      return '🔷';
    default:
      return '📧';
  }
};
</script>

<template>
  <div class="flex flex-col gap-6">
    <!-- Authentication Methods -->
    <div class="bg-slate-800 rounded-xl border border-slate-700/50 hover:border-slate-600/50 transition-colors">
      <div class="px-4 py-3 bg-slate-700/30 border-b border-slate-700/50">
        <h2 class="text-lg font-medium text-white">Authentication Methods</h2>
        <p class="text-sm text-slate-400 mt-1">Manage how you sign in to your account</p>
      </div>
      
      <div class="p-6 flex flex-col gap-4">
        <!-- Existing Auth Methods -->
        <div class="flex flex-col gap-3">
          <h3 class="text-sm font-medium text-white">Connected Accounts</h3>
          <div class="flex flex-col gap-2">
            <div v-for="method in authMethods" :key="method.id" class="flex items-center justify-between p-3 bg-slate-700/30 rounded-lg">
              <div class="flex items-center gap-3">
                <span class="text-xl">{{ getAuthMethodIcon(method.type) }}</span>
                <div>
                  <div class="text-sm font-medium text-white">
                    {{ method.identifier }}
                    <span v-if="method.isPrimary" class="ml-2 px-2 py-1 bg-blue-600/20 text-blue-400 rounded text-xs">Primary</span>
                  </div>
                  <div class="text-xs text-slate-400">
                    Added {{ formatDate(method.createdAt) }}
                  </div>
                </div>
              </div>
              <button
                v-if="!method.isPrimary && authMethods.length > 1"
                @click="removeAuthMethod(method.id, method.type)"
                :disabled="loading"
                class="text-red-400 hover:text-red-300 text-sm font-medium disabled:opacity-50"
              >
                Remove
              </button>
            </div>
          </div>
        </div>

        <!-- Add Auth Methods -->
        <div class="flex flex-col gap-3">
          <h3 class="text-sm font-medium text-white">Add Authentication Method</h3>
          <div class="flex flex-col gap-3">
            <button
              @click="addAuthMethod('microsoft')"
              :disabled="loading || hasMicrosoftConnection"
              class="flex items-center gap-3 p-3 bg-slate-700/30 hover:bg-slate-700/50 rounded-lg border border-slate-600/30 hover:border-slate-500/50 transition-colors disabled:opacity-50 max-w-sm"
            >
              <span class="text-xl">🔷</span>
              <div class="flex flex-col items-start">
                <span class="text-sm font-medium text-white">Microsoft</span>
                <span class="text-xs text-slate-400">
                  {{ hasMicrosoftConnection ? 'Already connected' : 'Azure AD / Entra ID' }}
                </span>
              </div>
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Active Sessions -->
    <div class="bg-slate-800 rounded-xl border border-slate-700/50 hover:border-slate-600/50 transition-colors">
      <div class="px-4 py-3 bg-slate-700/30 border-b border-slate-700/50">
        <h2 class="text-lg font-medium text-white">Active Sessions</h2>
        <p class="text-sm text-slate-400 mt-1">Manage your active login sessions</p>
      </div>
      
      <div class="p-6 flex flex-col gap-4">
        <!-- Sessions List -->
        <div class="flex flex-col gap-3">
          <div class="flex items-center justify-between">
            <h3 class="text-sm font-medium text-white">Current Sessions</h3>
            <button
              @click="revokeAllSessions"
              :disabled="loading || activeSessions.length <= 1"
              class="text-red-400 hover:text-red-300 text-sm font-medium disabled:opacity-50"
            >
              Revoke All Others
            </button>
          </div>
          
          <div class="flex flex-col gap-2">
            <div v-for="session in activeSessions" :key="session.id" class="flex items-center justify-between p-3 bg-slate-700/30 rounded-lg">
              <div class="flex items-center gap-3">
                <div class="w-10 h-10 bg-slate-600 rounded-lg flex items-center justify-center">
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-slate-300" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
                  </svg>
                </div>
                <div>
                  <div class="text-sm font-medium text-white">
                    {{ session.device_name || session.user_agent || 'Unknown Device' }}
                    <span v-if="session.is_current" class="ml-2 px-2 py-1 bg-green-600/20 text-green-400 rounded text-xs">Current</span>
                  </div>
                  <div class="text-xs text-slate-400">
                    {{ session.location || session.ip_address || 'Unknown location' }} • Last active {{ formatDate(session.last_active) }}
                  </div>
                </div>
              </div>
              <button
                v-if="!session.is_current"
                @click="revokeSession(session.id)"
                :disabled="loading"
                class="text-red-400 hover:text-red-300 text-sm font-medium disabled:opacity-50"
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