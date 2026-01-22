<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted, computed } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useAuthStore } from '@/stores/auth';
import BackButton from '@/components/common/BackButton.vue';
import Modal from '@/components/Modal.vue';
import HorizontalScrollContainer from '@/components/common/HorizontalScrollContainer.vue';
import {
  UserProfileCard,
  AppearanceSettings,
  NotificationSettings,
  SecuritySettings,
  MFASettings,
  AuthMethodsSettings,
  PasskeySettings
} from '@/components/settings';
import UserEmailsCard from '@/components/settings/UserEmailsCard.vue';
import userService from '@/services/userService';
import type { User } from '@/services/userService';
import apiClient from '@/services/apiConfig';
import { useMfa } from '@/composables/useMfa';

const route = useRoute();
const router = useRouter();
const authStore = useAuthStore();

// Global state for notifications
const successMessage = ref<string | null>(null);
const error = ref<string | null>(null);

// Active tab state (reactive)
const activeTab = ref('profile');

// Admin user management state
const targetUser = ref<User | null>(null);
const isManagingOtherUser = ref(false);
const loadingTargetUser = ref(false);
const updatingRole = ref(false);

// Get the current user being edited (either targetUser for admin or authStore.user for self)
const currentUser = computed(() => targetUser.value || authStore.user);

// Check if in admin user management mode
const targetUserUuid = computed(() => {
  return (route.params.uuid as string) || undefined;
});

const isAdminMode = computed(() => {
  return !!targetUserUuid.value && targetUserUuid.value !== authStore.user?.uuid;
});

// Update URL when tab changes without causing navigation
const updateURL = (section: string) => {
  let newPath: string;
  
  if (targetUserUuid.value) {
    // Admin managing another user
    newPath = section === 'profile' 
      ? `/users/${targetUserUuid.value}/settings` 
      : `/users/${targetUserUuid.value}/settings/${section}`;
  } else {
    // User managing their own profile
    newPath = section === 'profile' ? '/profile/settings' : `/profile/settings/${section}`;
  }
  
  // Use History API to update URL without triggering router navigation
  window.history.replaceState({}, '', newPath);
  
  // Update page title manually
  const prefix = isAdminMode.value ? 'User ' : '';
  const sectionTitles: Record<string, string> = {
    profile: `${prefix}Profile Settings`,
    appearance: `${prefix}Appearance Settings`,
    notifications: `${prefix}Notification Settings`,
    security: `${prefix}Security Settings`
  };
  
  const title = sectionTitles[section] || 'Settings';
  document.title = `${title} | Nosdesk`;
};

// Flag to prevent infinite loops during programmatic updates
const isUpdatingFromRoute = ref(false);

// Watch for tab changes to update URL
watch(activeTab, (newTab) => {
  if (!isUpdatingFromRoute.value) {
    updateURL(newTab);
  }
});

// Settings tabs
const settingsTabs = [
  {
    id: 'profile',
    label: 'Profile',
    icon: 'user'
  },
  {
    id: 'appearance',
    label: 'Appearance',
    icon: 'palette'
  },
  {
    id: 'notifications',
    label: 'Notifications',
    icon: 'bell'
  },
  {
    id: 'security',
    label: 'Security',
    icon: 'shield'
  }
];

// Available roles for admin management
const availableRoles = [
  {
    value: 'user',
    label: 'User',
    colorClass: 'bg-surface-hover',
    description: 'Can create tickets and view assigned resources'
  },
  {
    value: 'technician',
    label: 'Technician',
    colorClass: 'bg-accent',
    description: 'Can manage tickets, devices, and assist other users'
  },
  {
    value: 'admin',
    label: 'Administrator',
    colorClass: 'bg-status-error',
    description: 'Full access to all system features and user management'
  }
];

// Load target user if in admin mode
const loadTargetUser = async () => {
  if (!targetUserUuid.value || targetUserUuid.value === authStore.user?.uuid) {
    isManagingOtherUser.value = false;
    targetUser.value = null;
    return;
  }

  try {
    loadingTargetUser.value = true;
    const user = await userService.getUserByUuid(targetUserUuid.value);
    
    if (user) {
      targetUser.value = user;
      isManagingOtherUser.value = true;
    } else {
      error.value = 'User not found';
      // Redirect back to users list after a delay
      setTimeout(() => router.push('/users'), 2000);
    }
  } catch (e) {
    console.error('Error loading target user:', e);
    error.value = 'Failed to load user information';
    // Redirect back to users list after a delay
    setTimeout(() => router.push('/users'), 2000);
  } finally {
    loadingTargetUser.value = false;
  }
};

// Clear messages after a delay
const clearMessages = () => {
  setTimeout(() => {
  successMessage.value = null;
  error.value = null;
  }, 5000);
};

// Handle success messages (silently - no banner)
const handleSuccess = (message: string) => {
  // Clear any existing errors
  error.value = null;
  // Success is communicated through UI state changes, not banners
};

// Handle error messages  
const handleError = (message: string) => {
  error.value = message;
  successMessage.value = null;
  clearMessages();
};

// Handle browser back/forward navigation
const handlePopState = () => {
  const path = window.location.pathname;
  
  isUpdatingFromRoute.value = true;
  
  // Handle different URL patterns
  if (path.includes('/users/') && path.includes('/settings')) {
    // Admin managing another user: /users/:uuid/settings/:section?
    const parts = path.split('/');
    const section = parts[parts.length - 1];
    if (section === 'settings' || !settingsTabs.some(tab => tab.id === section)) {
      activeTab.value = 'profile';
    } else {
      activeTab.value = section;
    }
  } else if (path === '/profile/settings') {
    // Base profile URL maps to profile
    activeTab.value = 'profile';
  } else {
    // Extract section from URL
    const section = path.split('/').pop();
    if (section && settingsTabs.some(tab => tab.id === section)) {
      activeTab.value = section;
    } else {
      activeTab.value = 'profile';
    }
  }
  
  isUpdatingFromRoute.value = false;
};

// Initialize from current route on mount
onMounted(async () => {
  const section = route.params.section as string;
  const path = window.location.pathname;
  
  // Load target user if in admin mode
  await loadTargetUser();

  // Check if user has completed account setup (for resend invitation feature)
  if (isManagingOtherUser.value && targetUser.value) {
    await checkUserSetupStatus();
  }

  isUpdatingFromRoute.value = true;
  
  // Handle initialization based on current URL
  if (path.includes('/users/') && path.includes('/settings')) {
    // Admin managing another user
    if (!section || section === 'settings') {
      activeTab.value = 'profile';
    } else if (settingsTabs.some(tab => tab.id === section)) {
      activeTab.value = section;
    } else {
      activeTab.value = 'profile';
      updateURL('profile');
    }
  } else if (path === '/profile/settings') {
    // Base profile URL maps to profile
    activeTab.value = 'profile';
  } else if (section && settingsTabs.some(tab => tab.id === section)) {
    // Valid section in URL
    activeTab.value = section;
  } else {
    // Invalid or missing section, default to profile and update URL
    activeTab.value = 'profile';
    updateURL('profile');
  }
  
  isUpdatingFromRoute.value = false;
  
  // Listen for browser navigation
  window.addEventListener('popstate', handlePopState);
});

// Cleanup on unmount
onUnmounted(() => {
  window.removeEventListener('popstate', handlePopState);
});

// Tab icon renderer
const renderTabIcon = (iconName: string) => {
  const icons = {
    user: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" />',
    palette: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4.098 19.902a3.75 3.75 0 005.304 0l6.401-6.402M6.75 21A3.75 3.75 0 013 17.25V4.125C3 3.504 3.504 3 4.125 3h5.25c.621 0 1.125.504 1.125 1.125v4.072M6.75 21a3.75 3.75 0 003.75-3.75V8.197M6.75 21h13.125c.621 0 1.125-.504 1.125-1.125v-5.25c0-.621-.504-1.125-1.125-1.125h-4.072M10.5 8.197l2.88-2.88c.438-.439 1.15-.439 1.59 0l3.712 3.713c.44.44.44 1.152 0 1.59l-2.879 2.88M6.75 17.25h.008v.008H6.75v-.008z" />',
    bell: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 17h5l-1.405-1.405A2.032 2.032 0 0118 14.158V11a6.002 6.002 0 00-4-5.659V5a2 2 0 10-4 0v.341C7.67 6.165 6 8.388 6 11v3.159c0 .538-.214 1.055-.595 1.436L4 17h5m6 0v1a3 3 0 11-6 0v-1m6 0H9" />',
    shield: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z" />'
  };
  return icons[iconName as keyof typeof icons] || '';
};

// Get role color class for styling
const getRoleColorClass = (role: string) => {
  const roleConfig = availableRoles.find(r => r.value === role);
  if (roleConfig) {
    switch (role) {
      case 'admin':
        return 'bg-status-error text-white';
      case 'technician':
        return 'bg-accent text-white';
      case 'user':
      default:
        return 'bg-surface-hover text-secondary';
    }
  }
  return 'bg-surface-hover text-secondary';
};

// Update user role function
const updateUserRole = async (newRole: string) => {
  if (!targetUser.value || !isManagingOtherUser.value || !authStore.isAdmin) {
    console.warn('Unauthorized role update attempt');
    return;
  }

  if (targetUser.value.role === newRole) {
    return; // No change needed
  }

  try {
    updatingRole.value = true;

    // Update user role via API
    const updatedUser = await userService.updateUser(targetUser.value.uuid, {
      role: newRole
    });

    if (updatedUser) {
      // Update the local user object
      targetUser.value = { ...targetUser.value, role: newRole };
      
      handleSuccess(`Successfully updated ${targetUser.value.name}'s role to ${newRole}`);
    }
  } catch (error) {
    console.error('Failed to update user role:', error);
    handleError(`Failed to update user role. Please try again.`);
  } finally {
    updatingRole.value = false;
  }
};

// Resend invitation functionality
const resendingInvitation = ref(false);
const userHasCompletedSetup = ref(true); // Default to true, will be checked on load

// Check if user has completed account setup (has password)
const checkUserSetupStatus = async () => {
  if (!targetUser.value) return;

  try {
    // Get auth identities to check if user has a password set
    const identities = await apiClient.get(`/users/${targetUser.value.uuid}/auth-identities`);
    const hasPassword = identities.data?.some((identity: { provider_type: string; password_hash?: string }) =>
      identity.provider_type === 'local' && identity.password_hash
    );
    userHasCompletedSetup.value = hasPassword;
  } catch (e) {
    // If unable to check, assume setup complete
    userHasCompletedSetup.value = true;
  }
};

// Resend invitation email
const resendInvitation = async () => {
  if (!targetUser.value) return;

  try {
    resendingInvitation.value = true;
    const result = await userService.resendInvitation(targetUser.value.uuid);

    if (result.success) {
      handleSuccess(`Invitation email sent to ${result.email || targetUser.value.email}`);
    } else {
      handleError(result.message);
    }
  } catch (e) {
    handleError('Failed to resend invitation email');
  } finally {
    resendingInvitation.value = false;
  }
};

// Delete account functionality
const showDeleteModal = ref(false);
const deleteMfaCode = ref('');
const deletePassword = ref('');
const isDeleting = ref(false);

// MFA status for delete confirmation
const { mfaEnabled: adminMfaEnabled, checkMFAStatus: checkAdminMfaStatus } = useMfa();

// Check admin's MFA status when delete modal opens
const openDeleteModal = async () => {
  showDeleteModal.value = true;
  await checkAdminMfaStatus();
};

const deleteAccount = async () => {
  // Validate input based on MFA status
  if (adminMfaEnabled.value) {
    if (!deleteMfaCode.value) {
      handleError('Please enter your 2FA code to confirm deletion');
      return;
    }
  } else {
    if (!deletePassword.value) {
      handleError('Please enter your password to confirm deletion');
      return;
    }
  }

  try {
    isDeleting.value = true;

    const userToDelete = currentUser.value;
    if (!userToDelete?.uuid) {
      handleError('Unable to identify user account');
      return;
    }

    // Delete the user account (requires admin's MFA code or password)
    const requestData = adminMfaEnabled.value
      ? { mfa_code: deleteMfaCode.value }
      : { password: deletePassword.value };

    await apiClient.delete(`/users/${userToDelete.uuid}`, {
      data: requestData
    });

    // If deleting own account, logout and redirect
    if (!isAdminMode.value) {
      await authStore.logout();
      router.push('/login?deleted=true');
    } else {
      // Admin deleted another user, redirect to users list
      router.push('/users?deleted=true');
    }
  } catch (error) {
    console.error('Failed to delete account:', error);
    const axiosError = error as { response?: { data?: { message?: string } } };
    handleError(axiosError.response?.data?.message || 'Failed to delete account. Please try again.');
    isDeleting.value = false;
  } finally {
    showDeleteModal.value = false;
    deleteMfaCode.value = '';
    deletePassword.value = '';
  }
};

const cancelDelete = () => {
  showDeleteModal.value = false;
  deleteMfaCode.value = '';
  deletePassword.value = '';
};
</script>

<template>
  <div class="flex-1 flex flex-col">
    <!-- Navigation and actions bar -->
    <div class="pt-4 px-4 sm:px-6 flex justify-between items-center">
      <BackButton
        :fallbackRoute="isManagingOtherUser ? `/users/${targetUserUuid}` : '/'"
        :label="isManagingOtherUser ? 'Back to User Profile' : 'Back to Dashboard'"
      />
    </div>

    <!-- Mobile Tab Navigation (horizontal scroll) - sticky full-width on mobile -->
    <div class="lg:hidden sticky top-0 z-20 bg-app border-b border-default">
      <div class="px-4 sm:px-6 py-2">
        <HorizontalScrollContainer container-class="gap-2" fade-background="bg-app" :show-dots="false">
          <button
            v-for="tab in settingsTabs"
            :key="tab.id"
            @click="activeTab = tab.id"
            class="flex items-center gap-2 px-4 py-2.5 rounded-lg transition-all whitespace-nowrap flex-shrink-0 min-h-[44px]"
            :class="[
              activeTab === tab.id
                ? 'bg-accent/10 border border-accent text-accent font-medium'
                : 'bg-surface border border-subtle text-secondary hover:bg-surface-hover hover:text-primary active:scale-95'
            ]"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" v-html="renderTabIcon(tab.icon)"></svg>
            <span class="text-sm">{{ tab.label }}</span>
          </button>
        </HorizontalScrollContainer>
      </div>
    </div>

    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-7xl flex-1">
      <!-- Page Header -->
      <div class="mb-2 sm:mb-6">
        <div v-if="loadingTargetUser" class="flex items-center gap-3">
          <div class="animate-spin h-5 w-5 border-2 border-accent border-t-transparent rounded-full"></div>
          <h1 class="text-xl sm:text-2xl font-bold text-primary">Loading User Settings...</h1>
        </div>
        <div v-else-if="isManagingOtherUser && targetUser">
          <div class="flex flex-col sm:flex-row items-start sm:items-center gap-3 mb-2">
            <div class="w-9 h-9 rounded-lg flex items-center justify-center flex-shrink-0 bg-accent/15 text-accent">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path
                stroke-linecap="round"
                stroke-linejoin="round"
                d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"
              />
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"
              />
              </svg>
            </div>
            <div class="min-w-0 flex-1 overflow-hidden">
              <h1 class="text-xl sm:text-2xl font-bold text-primary">User Settings</h1>
              <p class="text-sm sm:text-base text-secondary">
                <span class="block sm:inline">Managing settings for </span>
                <span class="text-accent font-medium break-all">{{ targetUser.name }}</span>
                <span class="text-tertiary break-all"> ({{ targetUser.email }})</span>
              </p>
            </div>
          </div>
        </div>
        <div v-else>
          <h1 class="text-xl sm:text-2xl font-bold text-primary">Settings</h1>
          <p class="text-sm sm:text-base text-secondary mt-1 sm:mt-2">
            Manage your profile, preferences, and security settings
          </p>
        </div>
      </div>

      <!-- Error messages only -->
      <div v-if="error" class="p-3 sm:p-4 bg-status-error/50 text-status-error rounded-lg text-sm sm:text-base border border-status-error/50">
        {{ error }}
      </div>

      <!-- Main content -->
      <div class="flex flex-col lg:flex-row gap-4 lg:gap-6">
        <!-- Desktop Sidebar Navigation -->
        <aside class="hidden lg:block lg:w-64 flex-shrink-0">
          <div class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden sticky top-4">
            <div class="px-4 py-3 bg-surface-alt border-b border-default">
              <h2 class="text-lg font-medium text-primary">Settings</h2>
            </div>
            <nav class="p-2 flex flex-col gap-1">
            <button
              v-for="tab in settingsTabs"
              :key="tab.id"
              @click="activeTab = tab.id"
              class="rounded-lg transition-colors duration-200 flex items-center gap-3 relative overflow-hidden px-3 py-2.5"
              :class="[
                activeTab === tab.id
                    ? 'bg-accent/10 border border-accent text-accent font-medium'
                    : 'text-secondary hover:bg-surface-hover hover:text-primary border border-transparent'
              ]"
            >
              <!-- Active indicator bar -->
              <div
                v-if="activeTab === tab.id"
                class="absolute left-0 top-0 bottom-0 w-1 bg-accent rounded-r"
              ></div>

              <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" v-html="renderTabIcon(tab.icon)"></svg>
              <span class="text-sm whitespace-nowrap">{{ tab.label }}</span>
            </button>
          </nav>
          </div>
        </aside>

        <!-- Content Area -->
        <div class="flex-1 min-w-0">
          <!-- Profile Tab -->
          <div v-if="activeTab === 'profile'" class="flex flex-col gap-6">
            <UserProfileCard
              :user="currentUser"
              :can-edit="true"
              :show-editable-fields="true"
              @success="handleSuccess"
              @error="handleError"
            />

            <!-- Email Addresses Management -->
            <UserEmailsCard
              v-if="currentUser?.uuid"
              :user-uuid="currentUser.uuid"
              :can-edit="true"
              @success="handleSuccess"
              @error="handleError"
            />

            <!-- Admin Role Management Card -->
            <div v-if="isManagingOtherUser && authStore.isAdmin && targetUser" class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden">
              <div class="px-4 sm:px-6 py-4 bg-surface-alt border-b border-default">
                <div class="flex items-center gap-3">
                  <div class="w-8 h-8 bg-status-warning/20 rounded-lg flex items-center justify-center flex-shrink-0">
                    <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 text-status-warning" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z" />
                    </svg>
                  </div>
                  <div>
                    <h2 class="text-base sm:text-lg font-semibold text-primary">Role Management</h2>
                    <p class="text-xs text-secondary hidden sm:block">Control user access permissions</p>
                  </div>
                </div>
              </div>

              <div class="p-4 sm:p-6">
                <div class="flex flex-col gap-5">
                  <!-- Role selection grid -->
                  <div class="grid grid-cols-1 sm:grid-cols-3 gap-3">
                    <button
                      v-for="role in availableRoles"
                      :key="role.value"
                      @click="updateUserRole(role.value)"
                      :disabled="updatingRole || targetUser.role === role.value"
                      class="group p-4 rounded-xl border-2 transition-all text-left"
                      :class="[
                        targetUser.role === role.value
                          ? 'border-accent bg-accent/10'
                          : 'border-transparent bg-surface-alt hover:bg-surface-hover hover:border-default',
                        updatingRole ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'
                      ]"
                    >
                      <div class="flex items-start justify-between gap-2 mb-2">
                        <div class="flex items-center gap-2.5">
                          <div
                            class="w-2.5 h-2.5 rounded-full flex-shrink-0"
                            :class="role.colorClass"
                          ></div>
                          <span class="font-semibold text-primary text-sm">{{ role.label }}</span>
                        </div>
                        <svg
                          v-if="targetUser.role === role.value"
                          xmlns="http://www.w3.org/2000/svg"
                          class="h-5 w-5 text-accent flex-shrink-0"
                          fill="none"
                          viewBox="0 0 24 24"
                          stroke="currentColor"
                        >
                          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
                        </svg>
                      </div>
                      <p class="text-xs text-secondary leading-relaxed">{{ role.description }}</p>
                    </button>
                  </div>

                  <!-- Warning notice -->
                  <div class="flex items-start gap-3 p-3 bg-status-warning/10 border border-status-warning/30 rounded-lg">
                    <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-status-warning flex-shrink-0 mt-0.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
                    </svg>
                    <p class="text-sm text-secondary">
                      Role changes take effect immediately. The user may need to refresh their session to see updated permissions.
                    </p>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <!-- Appearance Tab -->
          <div v-if="activeTab === 'appearance'">
            <AppearanceSettings
              :target-user-uuid="targetUserUuid"
              @success="handleSuccess"
              @error="handleError"
            />
          </div>

          <!-- Notifications Tab -->
          <div v-if="activeTab === 'notifications'">
            <NotificationSettings
              :target-user-uuid="targetUserUuid"
              @success="handleSuccess"
              @error="handleError"
            />
          </div>

          <!-- Security Tab -->
          <div v-if="activeTab === 'security'" class="flex flex-col gap-4">
            <SecuritySettings
              :target-user-uuid="targetUserUuid"
              @success="handleSuccess"
              @error="handleError"
            />
            <MFASettings
              :target-user-uuid="targetUserUuid"
              @success="handleSuccess"
              @error="handleError"
            />
            <PasskeySettings
              @success="handleSuccess"
              @error="handleError"
            />
            <AuthMethodsSettings
              :target-user-uuid="targetUserUuid"
              @success="handleSuccess"
              @error="handleError"
            />

            <!-- Resend Invitation Card (Admin only, for users who haven't completed setup) -->
            <div
              v-if="isManagingOtherUser && authStore.isAdmin && targetUser && !userHasCompletedSetup"
              class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden"
            >
              <div class="px-4 sm:px-6 py-4 bg-surface-alt border-b border-default">
                <div class="flex items-center justify-between gap-3">
                  <div class="flex items-center gap-3">
                    <div class="w-8 h-8 bg-accent/20 rounded-lg flex items-center justify-center flex-shrink-0">
                      <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 text-accent" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
                      </svg>
                    </div>
                    <div>
                      <h2 class="text-base sm:text-lg font-semibold text-primary">Account Setup</h2>
                      <p class="text-xs text-secondary hidden sm:block">User has not completed account setup</p>
                    </div>
                  </div>
                  <span class="text-xs px-2.5 py-1 bg-status-warning/20 text-status-warning rounded-full font-medium">Pending</span>
                </div>
              </div>

              <div class="p-4 sm:p-6">
                <div class="flex flex-col gap-4">
                  <!-- Status banner -->
                  <div class="flex items-start gap-3 p-4 bg-status-warning/10 border border-status-warning/30 rounded-lg">
                    <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-status-warning flex-shrink-0 mt-0.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                    </svg>
                    <div>
                      <p class="text-sm font-medium text-status-warning">Invitation pending</p>
                      <p class="text-xs text-status-warning/80 mt-1">
                        {{ targetUser.name }} has not yet set up their account. You can resend the invitation email with a new setup link.
                      </p>
                    </div>
                  </div>

                  <!-- Resend invitation action -->
                  <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
                    <div class="flex-1">
                      <h3 class="text-base font-medium text-primary mb-1">Resend Invitation Email</h3>
                      <p class="text-sm text-secondary">
                        Send a new invitation email to <span class="text-primary font-medium">{{ targetUser.email }}</span> with a secure link to set up their password.
                      </p>
                    </div>
                    <button
                      @click="resendInvitation"
                      :disabled="resendingInvitation"
                      class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 focus:outline-none focus:ring-2 focus:ring-accent transition-colors flex items-center gap-2 whitespace-nowrap disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                      <svg v-if="resendingInvitation" class="w-4 h-4 animate-spin" fill="none" viewBox="0 0 24 24">
                        <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                        <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 004 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                      </svg>
                      <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
                      </svg>
                      {{ resendingInvitation ? 'Sending...' : 'Resend Invitation' }}
                    </button>
                  </div>

                  <!-- Info notice -->
                  <p class="text-xs text-tertiary">
                    The invitation link expires in 7 days. Any previous invitation links will be invalidated when a new one is sent.
                  </p>
                </div>
              </div>
            </div>

            <!-- Delete Account Section -->
            <div class="bg-surface rounded-xl border border-status-error hover:border-status-error transition-colors overflow-hidden">
              <div class="px-4 py-3 bg-status-error/10 border-b border-status-error">
                <div class="flex items-center gap-2">
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-status-error" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
                  </svg>
                  <h2 class="text-lg font-medium text-status-error">Danger Zone</h2>
                </div>
                <p class="text-sm text-status-error/80 mt-1">Irreversible and destructive actions</p>
              </div>

              <div class="p-6">
                <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
                  <div class="flex-1">
                    <h3 class="text-base font-medium text-primary mb-1">
                      {{ isAdminMode ? 'Delete User Account' : 'Delete Account' }}
                    </h3>
                    <p class="text-sm text-secondary">
                      {{ isAdminMode
                        ? `Permanently delete ${currentUser?.name}'s account and all associated data.`
                        : 'Permanently delete your account and all associated data.'
                      }}
                      This action cannot be undone.
                    </p>
                  </div>
                  <button
                    @click="openDeleteModal"
                    class="btn-danger px-4 py-2 bg-status-error text-white rounded-lg hover:bg-status-error/80 focus:outline-none focus:ring-2 focus:ring-status-error transition-colors flex items-center gap-2 whitespace-nowrap"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                    </svg>
                    Delete Account
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Delete Confirmation Modal -->
    <Modal
      :show="showDeleteModal"
      title="Confirm Account Deletion"
      @close="cancelDelete"
    >
      <div class="flex flex-col gap-4">
        <div class="bg-status-error/20 border border-status-error/50 rounded-lg p-4">
          <div class="flex gap-3">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-6 w-6 text-status-error flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
            </svg>
            <div>
              <p class="font-medium text-status-error mb-2">This action is permanent and cannot be undone!</p>
              <p class="text-sm text-status-error/80">
                {{ isAdminMode
                  ? `Deleting ${currentUser?.name}'s account will permanently remove:`
                  : 'Deleting your account will permanently remove:'
                }}
              </p>
              <ul class="list-disc list-inside text-sm text-status-error/80 mt-2 space-y-1">
                <li>Profile information and settings</li>
                <li>All tickets created or assigned to this user</li>
                <li>Comments and activity history</li>
                <li>Access to all systems and resources</li>
              </ul>
            </div>
          </div>
        </div>

        <!-- MFA Code Input (shown when admin has MFA enabled) -->
        <div v-if="adminMfaEnabled" class="flex flex-col gap-2">
          <label class="text-sm font-medium text-secondary">
            Enter your 2FA code from your authenticator app to confirm:
          </label>
          <input
            v-model="deleteMfaCode"
            type="text"
            inputmode="numeric"
            pattern="[0-9]*"
            autocomplete="one-time-code"
            maxlength="6"
            class="w-full px-4 py-2 bg-surface-alt text-primary rounded-lg border border-default focus:ring-2 focus:ring-status-error focus:outline-none text-center text-2xl tracking-widest font-mono"
            placeholder="000000"
            @keyup.enter="deleteAccount"
          />
          <p class="text-xs text-secondary">
            Enter the 6-digit code from your authenticator app.
          </p>
        </div>

        <!-- Password Input (shown when admin doesn't have MFA enabled) -->
        <div v-else class="flex flex-col gap-2">
          <label class="text-sm font-medium text-secondary">
            Enter your password to confirm:
          </label>
          <input
            v-model="deletePassword"
            type="password"
            autocomplete="current-password"
            class="w-full px-4 py-2 bg-surface-alt text-primary rounded-lg border border-default focus:ring-2 focus:ring-status-error focus:outline-none"
            placeholder="Enter your password"
            @keyup.enter="deleteAccount"
          />
          <p class="text-xs text-secondary">
            Enter your account password to confirm this action.
          </p>
        </div>

        <div class="flex justify-end gap-3 pt-2">
          <button
            @click="cancelDelete"
            :disabled="isDeleting"
            class="px-4 py-2 bg-surface-alt text-primary rounded-lg hover:bg-surface-hover transition-colors disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            @click="deleteAccount"
            :disabled="(adminMfaEnabled ? (!deleteMfaCode || deleteMfaCode.length < 6) : !deletePassword) || isDeleting"
            class="btn-danger px-4 py-2 bg-status-error text-white rounded-lg hover:bg-status-error/80 transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2"
          >
            <span v-if="isDeleting" class="animate-spin h-4 w-4">
              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 004 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
            </span>
            {{ isDeleting ? 'Deleting...' : 'Delete Account Permanently' }}
          </button>
        </div>
      </div>
    </Modal>
  </div>
</template>

<style scoped>
/* Smooth transitions for theme selection */
.theme-option {
  transition: all 0.2s ease-in-out;
}

.theme-option:hover {
  transform: translateY(-1px);
}
</style> 