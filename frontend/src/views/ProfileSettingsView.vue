<script setup lang="ts">
import { ref, watch, onMounted, computed } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery } from '@pinia/colada';
import { useUserProfileBundle } from '@/composables/useUserProfileBundle';
import { useAuthStore } from '@/stores/auth';
import { useBrandingStore } from '@/stores/branding';
import { useToastStore } from '@/stores/toast';
import BackButton from '@/components/common/BackButton.vue';
import Callout from '@/components/common/Callout.vue';
import Spinner from '@/components/common/Spinner.vue';
import Icon from '@/components/common/Icon.vue';
import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';
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
  PasskeySettings,
  SessionsSettings
} from '@/components/settings';
import UserEmailsCard from '@/components/settings/UserEmailsCard.vue';
import userService from '@/services/userService';
import type { User } from '@/services/userService';
import { effectiveRole, rolesFromTier, type UserRole } from '@nosdesk/core/types/user';
import { groupService } from '@/services/groupService';
import type { Group } from '@nosdesk/core/types/group';
import apiClient from '@nosdesk/core/apiClient';
import { useMfa } from '@/composables/useMfa';
import { useColorFilter } from '@/composables/useColorFilter';
import { extractErrorMessage } from '@/utils/errors';

const route = useRoute();
const router = useRouter();
const authStore = useAuthStore();
const fluent = useFluent();
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args);
const toast = useToastStore();
const brandingStore = useBrandingStore();
const { colorFilterStyle } = useColorFilter();

// Global state for notifications. Success messages route through
// the toast store now (see `handleSuccess`); only errors still
// surface inline at the page level.
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

// Fallback swatch colour for groups that have no colour set. Group
// colours are arbitrary user data (rendered as hex + alpha), so this is
// a named default rather than a theme token.
const DEFAULT_GROUP_COLOR = '#6366f1';
const activeTab = ref(routeSection && validTabs.includes(routeSection) ? routeSection : 'profile');

// Admin user management state.
// The target user and the admin-viewed groups list are fetched via
// Pinia Colada with a reactive key so switching admin targets refetches
// per-user and revisits to the same user render instantly from cache.
//
// The user bundle goes through the shared useUserProfileBundle composable
// (single source of truth for the key, query, and include list) so this
// view and UserProfileView can never drift into storing different shapes
// under one cache key again — the bug this replaced was a flat-user query
// here colliding with the profile page's bundle under the same key, which
// surfaced as `targetUser.uuid === undefined` on SPA navigation.
const userProfileBundle = useUserProfileBundle({
  uuid: () => targetUserUuid.value,
  include: ['devices', 'groups'],
  enabled: () => isAdminMode.value,
});
const userGroupsQuery = useQuery({
  key: () => ['user-groups', targetUserUuid.value ?? ''],
  query: () => groupService.getUserGroups(targetUserUuid.value as string),
  enabled: () => isAdminMode.value,
});

// Local mirror of the loaded admin target user. Seeded from the bundle
// and mutated locally by `updateUserRole` so the role-grid reflects the
// change instantly; a refetch then reconciles with server truth.
const targetUser = ref<User | null>(null);
// Reseed when the admin target uuid changes; otherwise leave the
// local ref alone so role mutations aren't clobbered by background
// revalidations.
const seededUuid = ref<string | undefined>(undefined);
watch(
  [userProfileBundle.bundle, targetUserUuid],
  ([bundle, uuid]) => {
    if (!isAdminMode.value) {
      targetUser.value = null;
      seededUuid.value = undefined;
      return;
    }
    if (!bundle) return;
    if (seededUuid.value === uuid) return;
    targetUser.value = bundle.user;
    seededUuid.value = uuid;
  },
  { immediate: true },
);

// Surface a not-found / load-failure for the admin target user. The
// bundle query throws (rather than returning null) on a missing uuid,
// so a 404 maps to the not-found copy and anything else to load-failed.
watch(userProfileBundle.error, (err) => {
  if (!err || !isAdminMode.value) return;
  console.error('Error loading target user:', err);
  const status = (err as { response?: { status?: number } })?.response?.status;
  error.value = status === 404 ? t('user-profile-not-found') : t('user-profile-load-failed');
  setTimeout(() => router.push('/users'), 2000);
});

const isManagingOtherUser = computed(() => isAdminMode.value && !!targetUser.value);
const loadingTargetUser = computed(
  () => isAdminMode.value && userProfileBundle.isLoading.value,
);
const updatingRole = ref(false);

// Get the current user being edited (either targetUser for admin or authStore.user for self)
const currentUser = computed(() => targetUser.value || authStore.user);

// Groups list comes from the cache-backed query in admin mode.
const userGroups = computed<Group[]>(() =>
  Array.isArray(userGroupsQuery.data.value) ? userGroupsQuery.data.value : [],
);
const loadingGroups = computed(
  () =>
    isAdminMode.value &&
    userGroupsQuery.status.value === 'pending' &&
    userGroupsQuery.data.value === undefined,
);

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
  document.title = brandingStore.getPageTitle(title);
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

// Available roles for admin management. Labels / descriptions are
// `computed` so a locale flip re-renders the role grid without
// re-mounting the section. The `colorClass` stays static because it
// belongs to the design system, not the content layer.
const availableRoles = computed<{ value: UserRole; label: string; colorClass: string; description: string }[]>(() => [
  {
    value: 'user',
    label: t('profile-role-user-label'),
    colorClass: 'bg-surface-hover',
    description: t('profile-role-user-description'),
  },
  {
    value: 'technician',
    label: t('profile-role-technician-label'),
    colorClass: 'bg-accent',
    description: t('profile-role-technician-description'),
  },
  {
    value: 'admin',
    label: t('profile-role-admin-label'),
    colorClass: 'bg-status-error',
    description: t('profile-role-admin-description'),
  },
]);

// Clear the inline error banner after a delay (success messages
// auto-dismiss via the toast store's own timer).
const clearMessages = () => {
  setTimeout(() => {
    error.value = null;
  }, 5000);
};

// Surface child-card success messages as toasts. The earlier silent
// behaviour ("communicated through UI state changes") was the
// "save button appears to do nothing" V1-polish complaint; toasts
// give a brief, brand-correct confirmation without blocking flow.
const handleSuccess = (message: string) => {
  error.value = null;
  if (message) toast.success(message);
};

// Handle error messages (inline banner; see G's error UX rule).
const handleError = (message: string) => {
  error.value = message;
  clearMessages();
};

// Initialize from current route on mount
onMounted(async () => {
  // Check if user has completed account setup (for resend invitation feature).
  // Defer until the target user lands; in admin mode the user query may
  // still be in flight on mount, so a watch handles it once data arrives.
  if (isManagingOtherUser.value && targetUser.value) {
    await checkUserSetupStatus();
  }

  // If active tab wasn't set from route, ensure URL matches
  if (!routeSection || !validTabs.includes(routeSection)) {
    updateURL('profile');
  }
});

// In admin mode the target user lands asynchronously from the cache;
// re-run the setup-status probe once it's available.
watch(targetUser, async (u) => {
  if (u && isAdminMode.value) {
    await checkUserSetupStatus();
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

// Effective role tier of the user being managed (for the role-grid
// selection state), derived from the W2 split.
const targetRole = computed<UserRole | null>(() =>
  targetUser.value ? effectiveRole(targetUser.value) : null
);

// Update user role function
const updateUserRole = async (newRole: UserRole) => {
  if (!targetUser.value || !isManagingOtherUser.value || !authStore.isAdmin) {
    console.warn('Unauthorized role update attempt');
    return;
  }

  if (effectiveRole(targetUser.value) === newRole) {
    return; // No change needed
  }

  try {
    updatingRole.value = true;

    // Update user role via API
    const updatedUser = await userService.updateUser(targetUser.value.uuid, {
      role: newRole
    });

    if (updatedUser && targetUser.value) {
      const name = targetUser.value.name;
      // Optimistic local update with the W2 split derived from the picked
      // tier so the role-grid flips instantly...
      targetUser.value = { ...targetUser.value, ...rolesFromTier(newRole) };
      // ...then reconcile the shared bundle from server truth. The
      // seededUuid guard keeps the reseed watch from clobbering the
      // optimistic value before the refetch lands.
      void userProfileBundle.refetch();

      handleSuccess(`Successfully updated ${name}'s role to ${newRole}`);
    }
  } catch (error) {
    console.error('Failed to update user role:', error);
    // Surface the server's reason (e.g. "role does not exist", "user has
    // active tickets") so the operator can act, not retry the same action.
    handleError(extractErrorMessage(error, 'Failed to update user role. Please try again.'));
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
    handleError(extractErrorMessage(error, 'Failed to delete account. Please try again.'));
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
                      backgroundColor: (group.color || DEFAULT_GROUP_COLOR) + '20',
                      color: group.color || DEFAULT_GROUP_COLOR,
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
                  <!-- Role selection grid. Auto-fit wrapping rather than
                       fixed breakpoint columns: cards hold a 11rem floor
                       and reflow 1 -> 2 -> 3 up as width allows, so they
                       never get crushed or clipped on narrow/landscape
                       phones the way `sm:grid-cols-3` did. -->
                  <div class="grid grid-cols-[repeat(auto-fit,minmax(11rem,1fr))] gap-3">
                    <button
                      v-for="role in availableRoles"
                      :key="role.value"
                      @click="updateUserRole(role.value)"
                      :disabled="updatingRole || targetRole === role.value"
                      class="group p-4 rounded-xl border-2 transition-all text-left min-w-0"
                      :class="[
                        targetRole === role.value
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
                          v-if="targetRole === role.value"
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
                    <Button
                      icon="email"
                      :loading="resendingInvitation"
                      @click="resendInvitation"
                    >
                      {{ resendingInvitation ? 'Sending...' : 'Resend Invitation' }}
                    </Button>
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

            <!-- Active sessions: self-only. The /auth/sessions
                 endpoints resolve the user from the JWT, so this card
                 can't show another user's sessions in admin mode. -->
            <SessionsSettings
              v-if="!isAdminMode"
              @success="handleSuccess"
              @error="handleError"
            />

            <!-- Delete Account Section -->
            <Callout severity="error">
              <template #header>
                <p class="font-medium text-primary">{{ t('user-settings-danger-zone-title') }}</p>
                <p class="text-secondary mt-0.5">{{ t('user-settings-danger-zone-subtitle') }}</p>
              </template>

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
                  <Button variant="danger" icon="trash" @click="openDeleteModal">
                    Delete Account
                  </Button>
                </div>
              </div>
            </Callout>
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
          <label for="delete-confirm-password" class="text-sm font-medium text-secondary">
            Enter your password to confirm:
          </label>
          <FormInput
            id="delete-confirm-password"
            v-model="deletePassword"
            type="password"
            autocomplete="current-password"
            :placeholder="t('user-settings-password-placeholder')"
            @keyup.enter="deleteAccount"
          />
          <p class="text-xs text-secondary">
            Enter your account password to confirm this action.
          </p>
        </div>

        <div class="flex justify-end gap-3 pt-2">
          <Button variant="secondary" :disabled="isDeleting" @click="cancelDelete">
            Cancel
          </Button>
          <Button
            variant="danger"
            :loading="isDeleting"
            :disabled="adminMfaEnabled ? (!deleteMfaCode || deleteMfaCode.length < 6) : !deletePassword"
            @click="deleteAccount"
          >
            {{ isDeleting ? 'Deleting...' : 'Delete Account Permanently' }}
          </Button>
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