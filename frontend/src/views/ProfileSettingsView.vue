<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted, computed } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useAuthStore } from '@/stores/auth';
import BackButton from '@/components/common/BackButton.vue';
import Modal from '@/components/Modal.vue';
import {
  UserProfileCard,
  AppearanceSettings,
  NotificationSettings,
  SecuritySettings,
  MFASettings,
  AuthMethodsSettings
} from '@/components/settings';
import UserEmailsCard from '@/components/settings/UserEmailsCard.vue';
import userService from '@/services/userService';
import type { User } from '@/services/userService';
import apiClient from '@/services/apiConfig';

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

// Check if we're in admin user management mode
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
    icon: 'user',
    color: '#FF66B3'
  },
  { 
    id: 'appearance', 
    label: 'Appearance', 
    icon: 'palette',
    color: '#8B5CF6'
  },
  { 
    id: 'notifications', 
    label: 'Notifications', 
    icon: 'bell',
    color: '#FDBD10'
  },
  { 
    id: 'security', 
    label: 'Security', 
    icon: 'shield',
    color: '#00C951'
  }
];

// Available roles for admin management
const availableRoles = [
  { 
    value: 'user', 
    label: 'User', 
    color: '#64748B',
    description: 'Can create tickets and view assigned resources'
  },
  { 
    value: 'technician', 
    label: 'Technician', 
    color: '#3B82F6',
    description: 'Can manage tickets, devices, and assist other users'
  },
  { 
    value: 'admin', 
    label: 'Administrator', 
    color: '#EF4444',
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
        return 'bg-red-600 text-red-100';
      case 'technician':
        return 'bg-blue-600 text-blue-100';
      case 'user':
      default:
        return 'bg-slate-600 text-slate-200';
    }
  }
  return 'bg-slate-600 text-slate-200';
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

// Delete account functionality
const showDeleteModal = ref(false);
const deletePassword = ref('');
const isDeleting = ref(false);

const deleteAccount = async () => {
  if (!deletePassword.value) {
    handleError('Please enter your password to confirm deletion');
    return;
  }

  try {
    isDeleting.value = true;

    const userToDelete = currentUser.value;
    if (!userToDelete?.uuid) {
      handleError('Unable to identify user account');
      return;
    }

    // Delete the user account
    await apiClient.delete(`/users/${userToDelete.uuid}`, {
      data: { password: deletePassword.value }
    });

    // If deleting own account, logout and redirect
    if (!isAdminMode.value) {
      await authStore.logout();
      router.push('/login?deleted=true');
    } else {
      // Admin deleted another user, redirect to users list
      router.push('/users?deleted=true');
    }
  } catch (error: any) {
    console.error('Failed to delete account:', error);
    handleError(error.response?.data?.message || 'Failed to delete account. Please try again.');
    isDeleting.value = false;
  } finally {
    showDeleteModal.value = false;
    deletePassword.value = '';
  }
};

const cancelDelete = () => {
  showDeleteModal.value = false;
  deletePassword.value = '';
};
</script>

<template>
  <div class="flex-1">
    <!-- Navigation and actions bar -->
    <div class="pt-4 px-4 sm:px-6 flex justify-between items-center">
      <BackButton
        :fallbackRoute="isManagingOtherUser ? `/users/${targetUserUuid}` : '/'"
        :label="isManagingOtherUser ? 'Back to User Profile' : 'Back to Dashboard'"
      />
    </div>

    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-7xl">
      <!-- Page Header -->
      <div class="mb-2 sm:mb-6">
        <div v-if="loadingTargetUser" class="flex items-center gap-3">
          <div class="animate-spin h-5 w-5 border-2 border-brand-blue border-t-transparent rounded-full"></div>
          <h1 class="text-xl sm:text-2xl font-bold text-primary">Loading User Settings...</h1>
        </div>
        <div v-else-if="isManagingOtherUser && targetUser">
          <div class="flex flex-col sm:flex-row items-start sm:items-center gap-3 mb-2">
            <div class="w-8 h-8 bg-brand-purple/20 rounded-full flex items-center justify-center flex-shrink-0">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 text-brand-purple" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"
              />
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"
              />
              </svg>
            </div>
            <div class="min-w-0 flex-1">
              <h1 class="text-xl sm:text-2xl font-bold text-primary">Managing User Settings</h1>
              <p class="text-sm sm:text-base text-secondary truncate">
                Managing settings for <span class="text-brand-blue font-medium">{{ targetUser.name }}</span> ({{ targetUser.email }})
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

      <!-- Mobile Tab Navigation (horizontal scroll) -->
      <div class="lg:hidden -mx-4 sm:-mx-6 px-4 sm:px-6 mb-4">
        <div class="flex gap-2 overflow-x-auto pb-2 scrollbar-hide">
          <button
            v-for="tab in settingsTabs"
            :key="tab.id"
            @click="activeTab = tab.id"
            class="relative flex items-center gap-2 px-4 py-2.5 rounded-lg transition-all whitespace-nowrap flex-shrink-0 min-h-[44px]"
            :class="[
              activeTab === tab.id
                ? 'bg-surface-alt border border-default text-primary font-medium'
                : 'bg-surface border border-subtle text-secondary hover:bg-surface-hover hover:text-primary active:scale-95'
            ]"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" v-html="renderTabIcon(tab.icon)"></svg>
            <span class="text-sm">{{ tab.label }}</span>
            <!-- Fancy underline indicator -->
            <div
              v-if="activeTab === tab.id"
              class="absolute bottom-1.5 left-2 right-2 h-0.5 rounded-full transition-all duration-300"
              :style="{
                backgroundColor: tab.color,
                boxShadow: `0 0 8px ${tab.color}40, 0 0 4px ${tab.color}60`
              }"
            ></div>
          </button>
        </div>
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
              class="rounded-lg transition-colors duration-200 text-primary flex items-center gap-3 relative overflow-hidden px-3 py-2.5"
              :class="[
                activeTab === tab.id
                    ? 'bg-surface-hover border border-default text-primary font-medium'
                    : 'text-secondary hover:bg-surface-hover hover:text-primary border border-transparent'
              ]"
            >
              <!-- Active indicator bar -->
              <div
                v-if="activeTab === tab.id"
                  class="absolute left-0 top-0 bottom-0 w-1 rounded-r"
                :style="{ backgroundColor: tab.color }"
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
            <div v-if="isManagingOtherUser && authStore.isAdmin && targetUser" class="bg-surface rounded-xl border border-default hover:border-strong transition-colors">
              <div class="px-4 py-3 bg-surface-alt border-b border-default flex flex-wrap items-center gap-2">
                <div class="w-6 h-6 bg-status-error/20 rounded-full flex items-center justify-center flex-shrink-0">
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-3 w-3 text-status-error" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
                  </svg>
                </div>
                <h2 class="text-base sm:text-lg font-medium text-primary">Role Management</h2>
                <span class="text-xs px-2 py-1 bg-status-error/30 text-status-error rounded-full sm:ml-auto">Admin Only</span>
              </div>
              <div class="p-4 sm:p-6">
                <div class="flex flex-col gap-4">
                  <div class="flex flex-col gap-3">
                    <div class="flex-1">
                      <h3 class="text-sm font-medium text-primary mb-1">User Role</h3>
                      <p class="text-xs text-secondary mb-3">
                        Control what {{ targetUser.name }} can access and manage in the system.
                      </p>

                      <div class="flex flex-wrap items-center gap-3">
                        <div class="flex items-center gap-2">
                          <span class="text-sm text-secondary">Current:</span>
                          <span
                            class="px-2 py-1 rounded text-xs font-medium"
                            :class="getRoleColorClass(targetUser.role)"
                          >
                            {{ targetUser.role.charAt(0).toUpperCase() + targetUser.role.slice(1) }}
                          </span>
                        </div>

                        <div v-if="updatingRole" class="flex items-center gap-2 text-brand-blue">
                          <div class="animate-spin h-3 w-3 border border-brand-blue border-t-transparent rounded-full"></div>
                          <span class="text-xs">Updating...</span>
                        </div>
                      </div>
                    </div>
                  </div>

                  <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
                    <button
                      v-for="role in availableRoles"
                      :key="role.value"
                      @click="updateUserRole(role.value)"
                      :disabled="updatingRole || targetUser.role === role.value"
                      class="p-3 sm:p-4 rounded-lg border transition-all text-left min-h-[80px] active:scale-[0.98]"
                      :class="[
                        targetUser.role === role.value
                          ? 'border-brand-blue/50 bg-brand-blue/20'
                          : 'border-default hover:border-strong bg-surface-alt hover:bg-surface-hover',
                        updatingRole ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'
                      ]"
                    >
                      <div class="flex items-center gap-2 mb-2">
                        <div
                          class="w-3 h-3 rounded-full flex-shrink-0"
                          :style="{ backgroundColor: role.color }"
                        ></div>
                        <span class="font-medium text-primary text-sm">{{ role.label }}</span>
                        <svg
                          v-if="targetUser.role === role.value"
                          xmlns="http://www.w3.org/2000/svg"
                          class="h-4 w-4 text-brand-blue ml-auto flex-shrink-0"
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

                  <div class="bg-status-warning/20 border border-status-warning/50 rounded-lg p-3">
                    <div class="flex gap-2 sm:gap-3">
                      <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 sm:h-5 sm:w-5 text-status-warning flex-shrink-0 mt-0.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L3.732 16.5c-.77.833.192 2.5 1.732 2.5z" />
                      </svg>
                      <div class="min-w-0 flex-1">
                        <p class="text-xs font-medium text-status-warning mb-1">Role Change Warning</p>
                        <p class="text-xs text-status-warning/80 leading-relaxed">
                          Changing a user's role will immediately affect their access permissions.
                          The user will need to log out and back in for changes to take full effect.
                        </p>
                      </div>
                    </div>
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
            <AuthMethodsSettings
              :target-user-uuid="targetUserUuid"
              @success="handleSuccess"
              @error="handleError"
            />

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
                    @click="showDeleteModal = true"
                    class="px-4 py-2 bg-status-error text-primary rounded-lg hover:bg-status-error/80 focus:outline-none focus:ring-2 focus:ring-status-error transition-colors flex items-center gap-2 whitespace-nowrap"
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

        <div class="flex flex-col gap-2">
          <label class="text-sm font-medium text-secondary">
            {{ isAdminMode ? 'Enter your admin password to confirm:' : 'Enter your password to confirm:' }}
          </label>
          <input
            v-model="deletePassword"
            type="password"
            autocomplete="current-password"
            class="w-full px-4 py-2 bg-surface-alt text-primary rounded-lg border border-default focus:ring-2 focus:ring-status-error focus:outline-none"
            placeholder="Password"
            @keyup.enter="deleteAccount"
          />
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
            :disabled="!deletePassword || isDeleting"
            class="px-4 py-2 bg-status-error text-primary rounded-lg hover:bg-status-error/80 transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2"
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
/* Custom toggle switch styling */
.peer:checked ~ div {
  background-color: rgb(37 99 235);
}

.peer:checked ~ div:after {
  transform: translateX(100%);
  border-color: white;
}

/* Smooth transitions for theme selection */
.theme-option {
  transition: all 0.2s ease-in-out;
}

.theme-option:hover {
  transform: translateY(-1px);
}

/* Custom scrollbar for long content */
.overflow-y-auto::-webkit-scrollbar {
  width: 6px;
}

.overflow-y-auto::-webkit-scrollbar-track {
  background: rgb(51 65 85);
  border-radius: 3px;
}

.overflow-y-auto::-webkit-scrollbar-thumb {
  background: rgb(100 116 139);
  border-radius: 3px;
}

.overflow-y-auto::-webkit-scrollbar-thumb:hover {
  background: rgb(148 163 184);
}
</style> 