<script setup lang="ts">
import { ref, watch, onMounted, computed } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useAuthStore } from '@/stores/auth';
import BackButton from '@/components/common/BackButton.vue';
import Spinner from '@/components/common/Spinner.vue';
import Icon from '@/components/common/Icon.vue';
import Modal from '@/components/Modal.vue';
import HorizontalScrollContainer from '@/components/common/HorizontalScrollContainer.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import {
  UserProfileCard,
  AppearanceSettings,
  LocalizationSettings,
  NotificationSettings,
  SecuritySettings,
  MFASettings,
  AuthMethodsSettings,
  PasskeySettings
} from '@/components/settings';
import UserEmailsCard from '@/components/settings/UserEmailsCard.vue';
import userService from '@/services/userService';
import type { User } from '@/services/userService';
import type { UserRole } from '@/types/user';
import { groupService } from '@/services/groupService';
import type { Group } from '@/types/group';
import apiClient from '@/services/apiConfig';
import { useMfa } from '@/composables/useMfa';
import { useColorFilter } from '@/composables/useColorFilter';

const route = useRoute();
const router = useRouter();
const authStore = useAuthStore();
const fluent = useFluent();
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args);
const { colorFilterStyle } = useColorFilter();

// Groups state
const userGroups = ref<Group[]>([]);
const loadingGroups = ref(false);

// Global state for notifications
const successMessage = ref<string | null>(null);
const error = ref<string | null>(null);

// Check if in admin user management mode (derived synchronously from route)
const targetUserUuid = computed(() => {
  return (route.params.uuid as string) || undefined;
});

const isAdminMode = computed(() => {
  return !!targetUserUuid.value && targetUserUuid.value !== authStore.user?.uuid;
});

// Initialize activeTab synchronously from route to avoid flash
const routeSection = route.params.section as string | undefined;
const validTabs = ['profile', 'appearance', 'language', 'notifications', 'security'];
const activeTab = ref(routeSection && validTabs.includes(routeSection) ? routeSection : 'profile');

// Admin user management state
// Start loading immediately if route indicates admin mode, so nothing renders with wrong data
const targetUser = ref<User | null>(null);
const isManagingOtherUser = ref(false);
const loadingTargetUser = ref(isAdminMode.value);
const updatingRole = ref(false);

// Get the current user being edited (either targetUser for admin or authStore.user for self)
const currentUser = computed(() => targetUser.value || authStore.user);

// Update URL when tab changes without causing navigation.
// Uses history.replaceState to avoid triggering Vue Router's reactivity,
// which would destroy/recreate this component due to :key="viewRoute.path" in App.vue.
const updateURL = (section: string) => {
  let newPath: string;

  if (targetUserUuid.value) {
    newPath = section === 'profile'
      ? `/users/${targetUserUuid.value}/settings`
      : `/users/${targetUserUuid.value}/settings/${section}`;
  } else {
    newPath = section === 'profile' ? '/profile/settings' : `/profile/settings/${section}`;
  }

  window.history.replaceState(history.state, '', newPath);

  // Compose the document title from the tab label and a localized
  // "Settings" suffix. Join order isn't great in every language
  // (French wants "Paramètres du profil" not "Profil Paramètres")
  // but it's the browser-tab title; we accept the rough join here
  // and refine when a translator flags it.
  const tabKey = `settings-tab-${section}`;
  const suffix = fluent.$t('settings-section-suffix');
  const tabLabel = fluent.$t(tabKey);
  const title = section in { profile: 1, appearance: 1, language: 1, notifications: 1, security: 1 }
    ? `${tabLabel} ${suffix}`
    : fluent.$t('settings-sidebar-heading');
  document.title = `${title} | Nosdesk`;
};

// Watch for tab changes to update URL
watch(activeTab, (newTab) => {
  updateURL(newTab);
});

// Settings tabs. Language is its own top-level tab rather than
// being buried inside Appearance — locale + timezone aren't a
// visual preference, they're discoverability-critical for any
// non-default-locale user. `labelKey` is a Fluent key the
// template resolves with `$t()`; rendering at template time
// keeps the labels reactive to locale flips without re-running
// this module.
const settingsTabs = [
  { id: 'profile', labelKey: 'settings-tab-profile', icon: 'user' },
  { id: 'appearance', labelKey: 'settings-tab-appearance', icon: 'palette' },
  { id: 'language', labelKey: 'settings-tab-language', icon: 'globe' },
  { id: 'notifications', labelKey: 'settings-tab-notifications', icon: 'bell' },
  { id: 'security', labelKey: 'settings-tab-security', icon: 'shield' },
];

// Available roles for admin management
const availableRoles: { value: UserRole; label: string; colorClass: string; description: string }[] = [
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
const handleSuccess = (_message: string) => {
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

// Load user's groups
const loadUserGroups = async () => {
  const uuid = currentUser.value?.uuid;
  if (!uuid) return;

  try {
    loadingGroups.value = true;
    userGroups.value = await groupService.getUserGroups(uuid);
  } catch (e) {
    console.error('Error loading user groups:', e);
  } finally {
    loadingGroups.value = false;
  }
};

// Initialize from current route on mount
onMounted(async () => {
  // Load target user if in admin mode
  await loadTargetUser();

  // Check if user has completed account setup (for resend invitation feature)
  if (isManagingOtherUser.value && targetUser.value) {
    await checkUserSetupStatus();
  }

  // Load user groups (admin mode only — regular users see groups on their profile page)
  if (isAdminMode.value) {
    await loadUserGroups();
  }

  // If active tab wasn't set from route, ensure URL matches
  if (!routeSection || !validTabs.includes(routeSection)) {
    updateURL('profile');
  }
});


// Tab icon renderer
const renderTabIcon = (iconName: string) => {
  const icons = {
    user: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" />',
    palette: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4.098 19.902a3.75 3.75 0 005.304 0l6.401-6.402M6.75 21A3.75 3.75 0 013 17.25V4.125C3 3.504 3.504 3 4.125 3h5.25c.621 0 1.125.504 1.125 1.125v4.072M6.75 21a3.75 3.75 0 003.75-3.75V8.197M6.75 21h13.125c.621 0 1.125-.504 1.125-1.125v-5.25c0-.621-.504-1.125-1.125-1.125h-4.072M10.5 8.197l2.88-2.88c.438-.439 1.15-.439 1.59 0l3.712 3.713c.44.44.44 1.152 0 1.59l-2.879 2.88M6.75 17.25h.008v.008H6.75v-.008z" />',
    globe: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3.6 9h16.8M3.6 15h16.8M11.5 3a17 17 0 000 18M12.5 3a17 17 0 010 18M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />',
    bell: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 17h5l-1.405-1.405A2.032 2.032 0 0118 14.158V11a6.002 6.002 0 00-4-5.659V5a2 2 0 10-4 0v.341C7.67 6.165 6 8.388 6 11v3.159c0 .538-.214 1.055-.595 1.436L4 17h5m6 0v1a3 3 0 11-6 0v-1m6 0H9" />',
    shield: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z" />'
  };
  return icons[iconName as keyof typeof icons] || '';
};

// Update user role function
const updateUserRole = async (newRole: UserRole) => {
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

    if (updatedUser && targetUser.value) {
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

// Check if user has completed account setup (has a local auth identity)
const checkUserSetupStatus = async () => {
  if (!targetUser.value) return;

  try {
    const identities = await apiClient.get(`/users/${targetUser.value.uuid}/auth-identities`);
    const hasLocalIdentity = identities.data?.some((identity: { provider_type: string }) =>
      identity.provider_type === 'local'
    );
    userHasCompletedSetup.value = hasLocalIdentity;
  } catch {
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
  } catch {
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
            <span class="text-sm">{{ $t(tab.labelKey) }}</span>
          </button>
        </HorizontalScrollContainer>
      </div>
    </div>

    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-7xl flex-1">
      <!-- Page Header -->
      <div class="mb-2 sm:mb-6">
        <div v-if="loadingTargetUser" class="flex items-center gap-3 text-accent">
          <Spinner size="md" />
          <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('settings-loading-user') }}</h1>
        </div>
        <div v-else-if="isManagingOtherUser && targetUser">
          <div class="flex flex-col sm:flex-row items-start sm:items-center gap-3 mb-2">
            <div class="w-9 h-9 rounded-lg flex items-center justify-center flex-shrink-0 bg-accent/15 text-accent">
              <Icon name="settings" size="md" />
            </div>
            <div class="min-w-0 flex-1 overflow-hidden">
              <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('settings-user-heading') }}</h1>
              <p class="text-sm sm:text-base text-secondary">
                <span class="block sm:inline">{{ t('user-settings-managing-for') }} </span>
                <span class="text-accent font-medium break-all">{{ targetUser.name }}</span>
                <span class="text-tertiary break-all"> ({{ targetUser.email }})</span>
              </p>
            </div>
          </div>
        </div>
        <div v-else>
          <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('settings-sidebar-heading') }}</h1>
          <p class="text-sm sm:text-base text-secondary mt-1 sm:mt-2">
            {{ $t('settings-subtitle') }}
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
          <div class="sticky top-4">
            <SectionCard content-padding="p-2">
              <template #title>{{ $t('settings-sidebar-heading') }}</template>
              <nav class="flex flex-col gap-1">
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
                <span class="text-sm whitespace-nowrap">{{ $t(tab.labelKey) }}</span>
              </button>
            </nav>
            </SectionCard>
          </div>
        </aside>

        <!-- Content Area -->
        <div class="flex-1 min-w-0">
          <!-- Loading skeleton while admin target user data is being fetched -->
          <div v-if="loadingTargetUser" class="flex flex-col gap-4">
            <div v-for="i in 3" :key="i" class="bg-surface rounded-xl border border-default overflow-hidden">
              <div class="px-4 py-3 bg-surface-alt border-b border-default">
                <div class="h-5 w-40 bg-surface-hover rounded animate-pulse"></div>
                <div class="h-4 w-64 bg-surface-hover rounded animate-pulse mt-2"></div>
              </div>
              <div class="p-6">
                <div class="h-12 bg-surface-hover rounded-lg animate-pulse"></div>
              </div>
            </div>
          </div>

          <template v-else>
          <!-- Profile Tab -->
          <div v-if="activeTab === 'profile'" class="flex flex-col gap-6">
            <UserProfileCard
              :user="currentUser ?? undefined"
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

            <!-- Groups Card (admin mode only) -->
            <SectionCard v-if="isAdminMode && authStore.isAdmin" content-padding="p-4 sm:p-6">
              <template #leading>
                <span class="text-accent inline-flex"><Icon name="team" /></span>
              </template>
              <template #title>{{ t('user-settings-groups-title') }}</template>
              <template #headerActions>
                <router-link
                  to="/admin/groups"
                  class="text-[11px] font-medium text-accent hover:underline whitespace-nowrap inline-flex items-center gap-1"
                >
                  <Icon name="settings" size="xs" />
                  Manage Groups
                </router-link>
              </template>

              <div>
                <!-- Loading state -->
                <div v-if="loadingGroups" class="flex items-center gap-3 text-secondary">
                  <span class="text-accent inline-flex"><Spinner /></span>
                  <span class="text-sm">{{ $t('common-loading-groups') }}</span>
                </div>

                <!-- Empty state -->
                <p v-else-if="userGroups.length === 0" class="text-sm text-secondary">
                  This user is not a member of any groups.
                </p>

                <!-- Groups list -->
                <div v-else class="flex flex-wrap gap-2">
                  <router-link
                    v-for="group in userGroups"
                    :key="group.id"
                    :to="`/groups/${group.uuid}`"
                    class="inline-flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm font-medium cursor-pointer hover:opacity-80 transition-opacity"
                    :style="{
                      backgroundColor: (group.color || '#6366f1') + '20',
                      color: group.color || '#6366f1',
                      ...colorFilterStyle
                    }"
                  >
                    <Icon name="team" />
                    {{ group.name }}
                  </router-link>
                </div>
              </div>
            </SectionCard>

            <!-- Admin Role Management Card -->
            <SectionCard v-if="isManagingOtherUser && authStore.isAdmin && targetUser" content-padding="p-4 sm:p-6">
              <template #leading>
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 text-status-warning flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z" />
                </svg>
              </template>
              <template #title>{{ t('user-settings-role-management-title') }}</template>

              <div>
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
                        <Icon
                          v-if="targetUser.role === role.value"
                          name="check"
                          size="md"
                          class="text-accent flex-shrink-0"
                        />
                      </div>
                      <p class="text-xs text-secondary leading-relaxed">{{ role.description }}</p>
                    </button>
                  </div>

                  <!-- Warning notice -->
                  <div class="flex items-start gap-3 p-3 bg-status-warning/10 border border-status-warning/30 rounded-lg">
                    <Icon name="info" size="md" class="text-status-warning flex-shrink-0 mt-0.5" />
                    <p class="text-sm text-secondary">
                      Role changes take effect immediately. The user may need to refresh their session to see updated permissions.
                    </p>
                  </div>
                </div>
              </div>
            </SectionCard>

            <!-- Account Setup Card (Admin only, for users who haven't completed setup) -->
            <SectionCard
              v-if="isManagingOtherUser && authStore.isAdmin && targetUser && !userHasCompletedSetup"
              content-padding="p-4 sm:p-6"
            >
              <template #leading>
                <span class="text-accent inline-flex"><Icon name="email" /></span>
              </template>
              <template #title>{{ t('user-settings-account-setup-title') }}</template>
              <template #headerActions>
                <span class="text-[11px] px-2 py-0.5 bg-status-warning/20 text-status-warning rounded-full font-medium">{{ t('user-settings-account-setup-pending') }}</span>
              </template>

              <div>
                <div class="flex flex-col gap-4">
                  <!-- Status banner -->
                  <div class="flex items-start gap-3 p-4 bg-status-warning/10 border border-status-warning/30 rounded-lg">
                    <Icon name="clock" size="md" class="text-status-warning flex-shrink-0" />
                    <div class="flex flex-col gap-1">
                      <p class="text-sm font-medium text-status-warning">{{ t('user-settings-invitation-pending') }}</p>
                      <p class="text-xs text-status-warning/80">
                        {{ targetUser.name }} has not yet set up their account. You can resend the invitation email with a new setup link.
                      </p>
                    </div>
                  </div>

                  <!-- Resend invitation action -->
                  <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
                    <div class="flex-1">
                      <h3 class="text-base font-medium text-primary">{{ t('user-settings-resend-invitation-title') }}</h3>
                      <p class="text-sm text-secondary">
                        Send a new invitation email to <span class="text-primary font-medium">{{ targetUser.email }}</span> with a secure link to set up their password.
                      </p>
                    </div>
                    <button
                      @click="resendInvitation"
                      :disabled="resendingInvitation"
                      class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 focus:outline-none focus:ring-2 focus:ring-accent transition-colors flex items-center gap-2 whitespace-nowrap disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                      <Spinner v-if="resendingInvitation" />
                      <Icon v-else name="email" />
                      {{ resendingInvitation ? 'Sending...' : 'Resend Invitation' }}
                    </button>
                  </div>

                  <!-- Info notice -->
                  <p class="text-xs text-tertiary">
                    The invitation link expires in 7 days. Any previous invitation links will be invalidated when a new one is sent.
                  </p>
                </div>
              </div>
            </SectionCard>
          </div>

          <!-- Appearance Tab -->
          <div v-if="activeTab === 'appearance'">
            <AppearanceSettings
              :target-user-uuid="targetUserUuid"
              :target-user-theme="targetUser?.theme"
              @success="handleSuccess"
              @error="handleError"
            />
          </div>

          <!-- Language Tab -->
          <div v-if="activeTab === 'language'">
            <LocalizationSettings
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
                  <Icon name="warning" size="md" class="text-status-error" />
                  <h2 class="text-lg font-medium text-status-error">{{ t('user-settings-danger-zone-title') }}</h2>
                </div>
                <p class="text-sm text-status-error/80 mt-1">{{ t('user-settings-danger-zone-subtitle') }}</p>
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
                    <Icon name="trash" />
                    Delete Account
                  </button>
                </div>
              </div>
            </div>
          </div>
          </template>
        </div>
      </div>
    </div>

    <!-- Delete Confirmation Modal -->
    <Modal
      :show="showDeleteModal"
      :title="t('user-settings-delete-modal-title')"
      @close="cancelDelete"
    >
      <div class="flex flex-col gap-4">
        <div class="bg-status-error/20 border border-status-error/50 rounded-lg p-4">
          <div class="flex gap-3">
            <Icon name="warning" size="lg" class="text-status-error flex-shrink-0" />
            <div>
              <p class="font-medium text-status-error mb-2">This action is permanent and cannot be undone!</p>
              <p class="text-sm text-status-error/80">
                {{ isAdminMode
                  ? `Deleting ${currentUser?.name}'s account will permanently remove:`
                  : 'Deleting your account will permanently remove:'
                }}
              </p>
              <ul class="list-disc list-inside text-sm text-status-error/80 mt-2 space-y-1">
                <li>{{ t('user-settings-delete-item-profile') }}</li>
                <li>{{ t('user-settings-delete-item-tickets') }}</li>
                <li>{{ t('user-settings-delete-item-comments') }}</li>
                <li>{{ t('user-settings-delete-item-access') }}</li>
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
            :placeholder="t('user-settings-password-placeholder')"
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
            <Spinner v-if="isDeleting" />
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