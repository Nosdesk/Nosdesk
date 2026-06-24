<script setup lang="ts">
import { formatDate as formatDateUtil } from '@/utils/dateUtils';
import { effectiveRole, type UserRole } from '@/types/user';
import { ref, computed, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useUserProfileBundle } from '@/composables/useUserProfileBundle';
import { useDelayedFlag } from '@/composables/useDelayedFlag';
import { useFluent } from 'fluent-vue';
import { useAuthStore } from "@/stores/auth";
import BackButton from "@/components/common/BackButton.vue";
import UserProfileCard from "@/components/settings/UserProfileCard.vue";
import UserEmailsCard from "@/components/settings/UserEmailsCard.vue";
import UserContactCard from "@/components/settings/UserContactCard.vue";
import UserAssignedTickets from "@/components/UserAssignedTickets.vue";
import BaseDropdown from "@/components/common/BaseDropdown.vue";
import Icon from "@/components/common/Icon.vue";
import Spinner from "@/components/common/Spinner.vue";
import SectionCard from "@/components/common/SectionCard.vue";
import { RouterLink } from "vue-router";
import userService from "@/services/userService";
import { useColorFilter } from "@/composables/useColorFilter";
import type { User } from "@/services/userService";
import type { Asset } from "@/types/asset";
import type { Group } from "@/types/group";

interface UserProfile extends User {
    department?: string;
    joinedDate?: string;
}

interface UserFormData {
    name: string;
    email: string;
    role: UserRole;
    pronouns?: string;
}

const route = useRoute();
const router = useRouter();
const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const authStore = useAuthStore();
const { colorFilterStyle } = useColorFilter();
const error = ref<string | null>(null);

// Cache-first: the existing-user bundle is a Pinia Colada query keyed on
// the route uuid, so userProfile / devices / groups are derived from it.
// A revisit renders instantly from cache, then refreshes silently.
// Creation mode (no uuid, or "new") runs no query: it's a form.
const userUuid = computed(() => route.params.uuid as string | undefined);
const isCreationMode = computed(() => !userUuid.value || userUuid.value === 'new');

// Shared bundle query (single source of truth for the cache key + shape;
// see useUserProfileBundle). ProfileSettingsView uses the same composable,
// so both pages share one cache entry and one shape.
const profileQuery = useUserProfileBundle({
    uuid: () => userUuid.value,
    include: ['devices', 'groups'],
    enabled: () => !isCreationMode.value,
});

const userProfile = computed<UserProfile | null>(() => {
    const user = profileQuery.bundle.value?.user;
    if (!user) return null;
    // Department is a placeholder until the backend carries it.
    return { ...user, department: 'IT Support', joinedDate: user.created_at };
});
const devices = computed<Asset[]>(() => profileQuery.bundle.value?.devices ?? []);
const groups = computed<Group[]>(() => profileQuery.bundle.value?.groups ?? []);

// Cold-load flag (initial fetch with no cached bundle). Exposed as a
// top-level ref so the template can use it directly.
const loading = computed(() => profileQuery.isLoading.value);

// Cold-load spinner only after 300ms with no cached data, so a warm
// revisit (or a fast load) shows no flash.
const showLoadingSpinner = useDelayedFlag(
    () => loading.value && !userProfile.value,
    300,
);

// Editing state (creation form + inline edits)
const editingEmail = ref(false);
const editingRole = ref(false);
const editingPronouns = ref(false);
const isSaving = ref(false);

// Invitation/password state
const smtpConfigured = ref(true);  // Default to true (will check on mount)
const sendInvitation = ref(true);  // Default to sending invitation
const manualPassword = ref("");
const confirmPassword = ref("");
const showPassword = ref(false);

// Editing values
const editValues = ref<UserFormData>({
    name: "",
    email: "",
    role: "user",
    pronouns: "",
});

// Role options
const roleOptions = computed(() => [
    { value: "user", label: t('user-profile-role-user') },
    { value: "technician", label: t('user-profile-role-technician') },
    { value: "audit_reviewer", label: t('user-profile-role-audit_reviewer') },
    { value: "admin", label: t('user-profile-role-admin') },
]);

// Permissions (derived). Admins can edit any profile; a user can edit
// their own; only admins can change roles.
const isOwnProfile = computed(() => authStore.user?.uuid === userUuid.value);
const canEdit = computed(() => isOwnProfile.value || authStore.isAdmin);
const canEditRole = computed(() => authStore.isAdmin);

// Check if the profile user can have assigned tickets (technicians and admins only)
const canHaveAssignedTickets = computed(() => {
    const role = userProfile.value ? effectiveRole(userProfile.value) : undefined;
    return role === 'technician' || role === 'admin';
});

// Update document title when user profile changes
watch(userProfile, (newProfile) => {
    if (newProfile) {
        document.title = t('user-profile-document-title', { name: newProfile.name });
    }
});

// Navigate to group detail page
const navigateToGroup = (group: Group) => {
    router.push(`/groups/${group.uuid}`);
};

// Creation mode is a form, not cached data: seed defaults, enable the
// editable fields, and probe SMTP so we can offer invitation vs manual
// password. Driven by the isCreationMode watcher below.
const setupCreationMode = async () => {
    error.value = null;
    if (!canEdit.value) {
        error.value = t('user-profile-error-no-create-permission');
        return;
    }

    editValues.value = { name: "", email: "", role: "user", pronouns: "" };
    editingEmail.value = true;
    editingRole.value = true;
    editingPronouns.value = true;

    // Check SMTP configuration status
    try {
        const emailConfig = await userService.getEmailConfigStatus();
        smtpConfigured.value = emailConfig.is_configured && emailConfig.enabled;
        // If SMTP is not configured, default to manual password
        if (!smtpConfigured.value) {
            sendInvitation.value = false;
        }
    } catch (err) {
        console.error("Failed to check email config:", err);
        smtpConfigured.value = false;
        sendInvitation.value = false;
    }

    // Focus on name field after DOM update
    setTimeout(() => {
        (document.getElementById("name-input") as HTMLInputElement | null)?.focus();
    }, 100);
};

// Seed the editable form from the loaded user. (User emails are loaded by
// UserEmailsCard, ticket lists by UserAssignedTickets.)
watch(
    () => profileQuery.bundle.value,
    (bundle) => {
        if (!bundle?.user) return;
        editValues.value = {
            name: bundle.user.name,
            email: bundle.user.email,
            role: effectiveRole(bundle.user),
            pronouns: bundle.user.pronouns || "",
        };
    },
    { immediate: true },
);

// Surface a load failure in the existing error banner (mirrors the old
// fetch's catch; the not-found block also covers the no-data case).
watch(
    () => profileQuery.error.value,
    (e) => {
        if (e) {
            error.value = t('user-profile-error-load');
            console.error("Error loading user profile:", e);
        }
    },
);

const formatDate = (dateString: string) => {
    try {
        const date = new Date(dateString);
        const now = new Date();
        const diffTime = now.getTime() - date.getTime();
        const diffDays = Math.floor(diffTime / (1000 * 60 * 60 * 24));
        const diffHours = Math.floor(diffTime / (1000 * 60 * 60));
        const diffMinutes = Math.floor(diffTime / (1000 * 60));

        if (diffMinutes < 1) {
            return t('user-profile-relative-just-now');
        } else if (diffMinutes < 60) {
            return t('user-profile-relative-minutes-ago', { count: diffMinutes });
        } else if (diffHours < 24) {
            return t('user-profile-relative-hours-ago', { count: diffHours });
        } else if (diffDays < 30) {
            return t('user-profile-relative-days-ago', { count: diffDays });
        } else {
            return formatDateUtil(dateString, "MMM d, yyyy");
        }
    } catch {
        return dateString;
    }
};

// Save user (create or update)
const saveUser = async () => {
    try {
        isSaving.value = true;

        if (isCreationMode.value) {
            // Validate password if not sending invitation
            if (!sendInvitation.value) {
                if (!manualPassword.value || manualPassword.value.length < 8) {
                    error.value = t('user-profile-error-password-too-short');
                    return;
                }
                if (manualPassword.value !== confirmPassword.value) {
                    error.value = t('user-profile-error-passwords-mismatch');
                    return;
                }
            }

            // Create new user
            const userData: {
                name: string;
                email: string;
                role: string;
                pronouns?: string;
                password?: string;
                send_invitation?: boolean;
            } = {
                name: editValues.value.name,
                email: editValues.value.email,
                role: editValues.value.role,
                pronouns: editValues.value.pronouns,
            };

            // Add password or invitation flag based on selected option
            if (sendInvitation.value && smtpConfigured.value) {
                userData.send_invitation = true;
            } else if (manualPassword.value) {
                userData.password = manualPassword.value;
            }

            const newUser = await userService.createUser(userData);
            console.log('✅ User created successfully:', newUser);

            if (!newUser?.uuid) {
                console.error('User created but no UUID returned:', newUser);
                error.value = t('user-profile-error-created-no-uuid');
                return;
            }

            // Navigate to the newly created user (replace history so back button goes to users list)
            console.log('🔄 Navigating to user:', `/users/${newUser.uuid}`);
            await router.replace(`/users/${newUser.uuid}`);
            console.log('✅ Navigation complete');
        } else {
            // Update existing user
            if (!userProfile.value) return;

            const updatedUser = await userService.updateUser(
                userProfile.value.uuid,
                {
                    name: editValues.value.name,
                    email: editValues.value.email,
                    role: editValues.value.role,
                    pronouns: editValues.value.pronouns,
                },
            );

            // Refresh authoritative data (userProfile is derived from the
            // bundle, so refetch rather than reassign).
            void updatedUser;
            await profileQuery.refetch();

            // Exit edit mode for all fields (name is handled by UserProfileCard)
            editingEmail.value = false;
            editingRole.value = false;
            editingPronouns.value = false;
        }
    } catch (err) {
        console.error("Error saving user:", err);
        // Extract error message - handles both Error objects and other types
        if (err instanceof Error) {
            error.value = err.message;
        } else {
            error.value = t('user-profile-error-save-generic');
        }
    } finally {
        isSaving.value = false;
    }
};

// Note: Name editing is now handled by UserProfileCard component

// View mode refetches automatically: the query key is reactive on the
// route uuid, so navigating between users re-runs it (and serves any
// cached entry instantly). Only creation mode needs explicit setup; when
// we leave it (e.g. after creating a user and routing to it), clear the
// edit flags so the now-read-only profile view isn't left in edit state.
watch(
    isCreationMode,
    (creating) => {
        if (creating) {
            void setupCreationMode();
        } else {
            editingEmail.value = false;
            editingRole.value = false;
            editingPronouns.value = false;
        }
    },
    { immediate: true },
);
</script>

<template>
    <div class="flex-1">
        <div v-if="userProfile || isCreationMode" class="flex flex-col">
            <!-- Navigation and actions bar -->
            <div class="pt-4 px-6 flex justify-between items-center">
                <BackButton fallbackRoute="/users" :label="$t('user-profile-back-to-users')" />
                <div v-if="!isCreationMode" class="flex items-center gap-2">
                    <!-- Own Profile Settings Button -->
                    <RouterLink
                        v-if="isOwnProfile"
                        to="/profile/settings"
                        class="px-4 py-2 bg-accent text-on-accent rounded-lg hover:opacity-90 transition-colors text-sm font-medium flex items-center gap-1"
                    >
                        <Icon name="settings" />
                        {{ $t('user-profile-action-profile-settings') }}
                    </RouterLink>

                    <!-- Admin: Manage User Settings Button -->
                    <RouterLink
                        v-else-if="canEditRole && userProfile && !isOwnProfile"
                        :to="`/users/${userProfile.uuid}/settings`"
                        class="px-4 py-2 bg-accent text-on-accent rounded-lg hover:bg-accent-hover transition-colors text-sm font-medium flex items-center gap-2"
                    >
                        <Icon name="settings" />
                        {{ $t('user-profile-action-user-settings') }}
                    </RouterLink>
                </div>
            </div>

            <div class="flex flex-col gap-4 px-6 py-4 mx-auto w-full max-w-8xl">
                <!-- Error Display -->
                <div
                    v-if="error"
                    class="bg-status-error/10 border border-status-error/30 rounded-xl p-4 flex items-start gap-3"
                >
                    <Icon name="warning" size="md" class="text-status-error flex-shrink-0 mt-0.5" />
                    <span class="text-status-error text-sm">{{ error }}</span>
                </div>

                <!-- User Creation Form -->
                <div v-if="isCreationMode" class="flex flex-col gap-6">
                    <!-- Main Form Card -->
                    <div class="bg-surface rounded-xl border border-default overflow-hidden">
                        <!-- Card Header -->
                        <div class="px-6 py-4 bg-surface-alt border-b border-default">
                            <div class="flex items-center gap-3">
                                <div class="w-10 h-10 rounded-full bg-accent/20 flex items-center justify-center">
                                    <Icon name="userPlus" size="md" class="text-accent" />
                                </div>
                                <div>
                                    <h1 class="text-lg font-semibold text-primary">{{ $t('user-profile-create-title') }}</h1>
                                    <p class="text-sm text-tertiary">{{ $t('user-profile-create-subtitle') }}</p>
                                </div>
                            </div>
                        </div>

                        <!-- Form Content -->
                        <div class="p-6">
                            <div class="grid grid-cols-1 lg:grid-cols-2 gap-8">
                                <!-- Left Column: Basic Info -->
                                <div class="flex flex-col gap-5">
                                    <h2 class="text-sm font-semibold text-primary flex items-center gap-2">
                                        <Icon name="user" class="text-tertiary" />
                                        {{ $t('user-profile-section-basic-info') }}
                                    </h2>

                                    <!-- Name Field -->
                                    <div class="flex flex-col gap-2">
                                        <label class="text-xs font-medium text-tertiary uppercase tracking-wider">
                                            {{ $t('user-profile-field-name') }} <span class="text-status-error">*</span>
                                        </label>
                                        <div class="relative">
                                            <input
                                                id="name-input"
                                                v-model="editValues.name"
                                                type="text"
                                                :placeholder="$t('user-profile-field-name-placeholder')"
                                                class="w-full px-4 py-3 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent transition-colors"
                                            />
                                        </div>
                                    </div>

                                    <!-- Email Field -->
                                    <div class="flex flex-col gap-2">
                                        <label class="text-xs font-medium text-tertiary uppercase tracking-wider">
                                            {{ $t('user-profile-field-email') }} <span class="text-status-error">*</span>
                                        </label>
                                        <div class="relative">
                                            <input
                                                v-model="editValues.email"
                                                type="email"
                                                :placeholder="$t('user-profile-field-email-placeholder')"
                                                class="w-full px-4 py-3 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent transition-colors"
                                            />
                                        </div>
                                    </div>

                                    <!-- Role Field -->
                                    <div class="flex flex-col gap-2">
                                        <label class="text-xs font-medium text-tertiary uppercase tracking-wider">{{ $t('user-profile-field-role') }}</label>
                                        <BaseDropdown
                                            v-model="editValues.role"
                                            :options="roleOptions"
                                            :placeholder="$t('user-profile-field-role-placeholder')"
                                        />
                                    </div>

                                    <!-- Pronouns Field -->
                                    <div class="flex flex-col gap-2">
                                        <label class="text-xs font-medium text-tertiary uppercase tracking-wider">{{ $t('user-profile-field-pronouns') }}</label>
                                        <input
                                            v-model="editValues.pronouns"
                                            type="text"
                                            :placeholder="$t('user-profile-field-pronouns-placeholder')"
                                            class="w-full px-4 py-3 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent transition-colors"
                                        />
                                    </div>
                                </div>

                                <!-- Right Column: Account Setup -->
                                <div class="flex flex-col gap-5">
                                    <h2 class="text-sm font-semibold text-primary flex items-center gap-2">
                                        <Icon name="key" class="text-tertiary" />
                                        {{ $t('user-profile-section-account-setup') }}
                                    </h2>

                                    <!-- SMTP Warning Banner -->
                                    <div
                                        v-if="!smtpConfigured"
                                        class="flex items-start gap-3 p-4 bg-status-warning/10 border border-status-warning/20 rounded-lg"
                                    >
                                        <Icon name="warning" size="md" class="text-status-warning flex-shrink-0 mt-0.5" />
                                        <div class="flex flex-col gap-1">
                                            <span class="text-sm font-medium text-status-warning">{{ $t('user-profile-smtp-warning-title') }}</span>
                                            <span class="text-xs text-status-warning/80">{{ $t('user-profile-smtp-warning-body') }}</span>
                                        </div>
                                    </div>

                                    <!-- Setup Method Selection -->
                                    <div class="flex flex-col gap-3">
                                        <label class="text-xs font-medium text-tertiary uppercase tracking-wider">{{ $t('user-profile-setup-method') }}</label>

                                        <!-- Send Invitation Option -->
                                        <button
                                            v-if="smtpConfigured"
                                            type="button"
                                            @click="sendInvitation = true"
                                            class="relative flex items-start gap-3 p-4 rounded-lg border-2 transition-all text-left"
                                            :class="sendInvitation
                                                ? 'border-accent bg-accent/5'
                                                : 'border-default bg-surface-alt hover:border-strong'"
                                        >
                                            <div
                                                class="w-9 h-9 rounded-lg flex items-center justify-center flex-shrink-0 transition-colors"
                                                :class="sendInvitation ? 'bg-accent/20' : 'bg-surface-hover'"
                                            >
                                                <Icon name="email" size="md" :class="sendInvitation ? 'text-accent' : 'text-tertiary'" />
                                            </div>
                                            <div class="flex-1 min-w-0">
                                                <span class="font-medium" :class="sendInvitation ? 'text-primary' : 'text-secondary'">{{ $t('user-profile-setup-invite-title') }}</span>
                                                <p class="text-xs text-tertiary mt-0.5">{{ $t('user-profile-setup-invite-body') }}</p>
                                            </div>
                                        </button>

                                        <!-- Set Password Option -->
                                        <button
                                            type="button"
                                            @click="sendInvitation = false"
                                            class="relative flex items-start gap-3 p-4 rounded-lg border-2 transition-all text-left"
                                            :class="!sendInvitation
                                                ? 'border-accent bg-accent/5'
                                                : 'border-default bg-surface-alt hover:border-strong'"
                                        >
                                            <div
                                                class="w-9 h-9 rounded-lg flex items-center justify-center flex-shrink-0 transition-colors"
                                                :class="!sendInvitation ? 'bg-accent/20' : 'bg-surface-hover'"
                                            >
                                                <Icon name="lock" size="md" :class="!sendInvitation ? 'text-accent' : 'text-tertiary'" />
                                            </div>
                                            <div class="flex-1 min-w-0">
                                                <span class="font-medium" :class="!sendInvitation ? 'text-primary' : 'text-secondary'">{{ $t('user-profile-setup-password-title') }}</span>
                                                <p class="text-xs text-tertiary mt-0.5">{{ $t('user-profile-setup-password-body') }}</p>
                                            </div>
                                        </button>
                                    </div>

                                    <!-- Password Fields (shown when manual password selected) -->
                                    <div
                                        v-if="!sendInvitation"
                                        class="flex flex-col gap-4 pt-2"
                                    >
                                        <!-- Password Input -->
                                        <div class="flex flex-col gap-2">
                                            <label class="text-xs font-medium text-tertiary uppercase tracking-wider">
                                                {{ $t('user-profile-field-password') }} <span class="text-status-error">*</span>
                                            </label>
                                            <div class="relative">
                                                <input
                                                    v-model="manualPassword"
                                                    :type="showPassword ? 'text' : 'password'"
                                                    :placeholder="$t('user-profile-field-password-placeholder')"
                                                    autocomplete="new-password"
                                                    class="w-full px-4 py-3 pr-12 bg-surface-alt border rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-1 transition-colors"
                                                    :class="manualPassword && manualPassword.length < 8
                                                        ? 'border-status-warning focus:border-status-warning focus:ring-status-warning'
                                                        : manualPassword.length >= 8
                                                            ? 'border-status-success focus:border-status-success focus:ring-status-success'
                                                            : 'border-default focus:border-accent focus:ring-accent'"
                                                />
                                                <button
                                                    type="button"
                                                    @click="showPassword = !showPassword"
                                                    class="absolute right-3 top-1/2 -translate-y-1/2 text-tertiary hover:text-primary transition-colors p-1"
                                                    tabindex="-1"
                                                >
                                                    <Icon v-if="showPassword" name="eye" size="md" />
                                                    <Icon v-else name="eyeOff" size="md" />
                                                </button>
                                            </div>
                                            <!-- Password strength indicator -->
                                            <div class="flex items-center gap-2">
                                                <div class="flex-1 h-1 bg-surface-alt rounded-full overflow-hidden">
                                                    <div
                                                        class="h-full transition-all duration-300"
                                                        :class="manualPassword.length >= 8 ? 'bg-status-success' : manualPassword.length >= 4 ? 'bg-status-warning' : 'bg-status-error'"
                                                        :style="{ width: `${Math.min(100, (manualPassword.length / 8) * 100)}%` }"
                                                    />
                                                </div>
                                                <span
                                                    class="text-xs"
                                                    :class="manualPassword.length >= 8 ? 'text-status-success' : 'text-tertiary'"
                                                >
                                                    {{ manualPassword.length }}/8
                                                </span>
                                            </div>
                                        </div>

                                        <!-- Confirm Password Input -->
                                        <div class="flex flex-col gap-2">
                                            <label class="text-xs font-medium text-tertiary uppercase tracking-wider">
                                                {{ $t('user-profile-field-confirm-password') }} <span class="text-status-error">*</span>
                                            </label>
                                            <input
                                                v-model="confirmPassword"
                                                :type="showPassword ? 'text' : 'password'"
                                                :placeholder="$t('user-profile-field-confirm-password-placeholder')"
                                                autocomplete="new-password"
                                                class="w-full px-4 py-3 bg-surface-alt border rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-1 transition-colors"
                                                :class="confirmPassword && manualPassword !== confirmPassword
                                                    ? 'border-status-error focus:border-status-error focus:ring-status-error'
                                                    : confirmPassword && manualPassword === confirmPassword && manualPassword.length >= 8
                                                        ? 'border-status-success focus:border-status-success focus:ring-status-success'
                                                        : 'border-default focus:border-accent focus:ring-accent'"
                                            />
                                            <!-- Match indicator -->
                                            <p
                                                v-if="confirmPassword"
                                                class="text-xs flex items-center gap-1.5"
                                                :class="manualPassword === confirmPassword && manualPassword.length >= 8 ? 'text-status-success' : 'text-status-error'"
                                            >
                                                <Icon
                                                    v-if="manualPassword === confirmPassword && manualPassword.length >= 8"
                                                    name="check"
                                                />
                                                <Icon v-else name="close" />
                                                {{ manualPassword === confirmPassword && manualPassword.length >= 8 ? $t('user-profile-passwords-match') : $t('user-profile-passwords-no-match') }}
                                            </p>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </div>

                        <!-- Card Footer with Actions -->
                        <div class="px-6 py-4 bg-surface-alt border-t border-default flex items-center justify-between">
                            <p class="text-xs text-tertiary">
                                <span class="text-status-error">*</span> {{ $t('user-profile-required-note') }}
                            </p>
                            <div class="flex items-center gap-3">
                                <button
                                    @click="router.push('/users')"
                                    :disabled="isSaving"
                                    class="px-5 py-2.5 text-sm font-medium text-secondary hover:text-primary bg-transparent hover:bg-surface-hover rounded-lg transition-colors disabled:opacity-50"
                                >
                                    {{ $t('user-profile-action-cancel') }}
                                </button>
                                <button
                                    @click="saveUser"
                                    :disabled="
                                        isSaving ||
                                        !editValues.name ||
                                        !editValues.email ||
                                        (!sendInvitation && (manualPassword.length < 8 || manualPassword !== confirmPassword))
                                    "
                                    class="px-5 py-2.5 text-sm font-medium text-on-accent bg-accent hover:bg-accent-hover rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2"
                                >
                                    <Spinner v-if="isSaving" />
                                    <Icon v-else name="add" />
                                    {{ isSaving ? $t('user-profile-action-creating') : $t('user-profile-action-create') }}
                                </button>
                            </div>
                        </div>
                    </div>
                </div>

                <!-- Responsive Container for existing user -->
                <div
                    v-else-if="userProfile"
                    class="flex flex-col xl:flex-row gap-4"
                >
                    <!-- User Info Area -->
                    <div class="flex flex-col gap-4 xl:w-1/2 xl:min-w-0">
                        <!-- User Profile Card (Read-only in profile view) -->
                        <UserProfileCard
                            :user="userProfile"
                            :canEdit="false"
                            :showEditableFields="false"
                        />

                        <!-- Email Addresses Section -->
                        <UserEmailsCard
                            v-if="userProfile?.uuid"
                            :user-uuid="userProfile.uuid"
                            :can-edit="false"
                        />

                        <!-- Contact details: standard + custom fields -->
                        <UserContactCard
                            v-if="userProfile?.uuid"
                            :uuid="userProfile.uuid"
                            :editable="canEdit"
                        />

                        <!-- Devices Section -->
                        <SectionCard content-padding="p-3">
                            <template #title>{{ $t('user-profile-assets-title') }}</template>
                            <div>
                                <div
                                    v-if="devices.length === 0"
                                    class="text-secondary text-sm"
                                >
                                    {{ $t('user-profile-assets-empty') }}
                                </div>
                                <div v-else class="flex flex-col gap-3">
                                    <RouterLink
                                        v-for="device in devices"
                                        :key="device.id"
                                        :to="`/devices/${device.id}`"
                                        class="block bg-surface-alt p-3 rounded-lg hover:bg-surface-hover transition-colors"
                                    >
                                        <div
                                            class="flex items-start justify-between"
                                        >
                                            <div class="flex-1">
                                                <h3
                                                    class="font-medium text-primary"
                                                >
                                                    {{ device.name }}
                                                </h3>
                                                <p
                                                    class="text-sm text-secondary"
                                                >
                                                    {{
                                                        device.manufacturer ||
                                                        $t('user-profile-asset-manufacturer-unknown')
                                                    }}
                                                    {{ device.model }}
                                                </p>
                                                <p
                                                    class="text-xs text-tertiary"
                                                >
                                                    {{ $t('user-profile-asset-last-updated', { when: formatDate(device.updated_at) }) }}
                                                </p>
                                            </div>
                                            <div v-if="device.attributes?.warranty_status" class="flex-shrink-0 ml-3">
                                                <span
                                                    class="text-xs px-2 py-1 rounded-full"
                                                    :class="{
                                                        'text-status-success bg-status-success/20':
                                                            device.attributes.warranty_status ===
                                                            'Active',
                                                        'text-status-warning bg-status-warning/20':
                                                            device.attributes.warranty_status ===
                                                            'Warning',
                                                        'text-status-error bg-status-error/20':
                                                            device.attributes.warranty_status ===
                                                            'Expired',
                                                        'text-secondary bg-surface-alt':
                                                            device.attributes.warranty_status ===
                                                            'Unknown',
                                                    }"
                                                >
                                                    {{ device.attributes.warranty_status }}
                                                </span>
                                            </div>
                                        </div>
                                    </RouterLink>
                                </div>
                            </div>
                        </SectionCard>

                        <!-- Groups Section -->
                        <SectionCard
                            v-if="groups.length > 0"
                            content-padding="p-3"
                        >
                            <template #title>{{ $t('user-profile-groups-title') }}</template>
                            <div class="flex flex-wrap gap-2">
                                <button
                                    v-for="group in groups"
                                    :key="group.id"
                                    @click="navigateToGroup(group)"
                                    class="inline-flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm font-medium cursor-pointer hover:opacity-80 transition-opacity"
                                    :style="{
                                        backgroundColor: (group.color || '#6366f1') + '20',
                                        color: group.color || '#6366f1',
                                        ...colorFilterStyle
                                    }"
                                >
                                    <Icon name="team" />
                                    {{ group.name }}
                                </button>
                            </div>
                        </SectionCard>
                    </div>

                    <!-- Tickets Area -->
                    <div class="flex flex-col gap-4 xl:w-1/2 xl:min-w-0">
                        <!-- Assigned Tickets (only for technicians and admins) -->
                        <UserAssignedTickets
                            v-if="canHaveAssignedTickets"
                            :user-uuid="userProfile.uuid"
                            ticket-type="assigned"
                            :limit="5"
                            :show-filters="false"
                        />

                        <!-- Requested Tickets -->
                        <UserAssignedTickets
                            :user-uuid="userProfile.uuid"
                            ticket-type="requested"
                            :limit="5"
                            :show-filters="false"
                        />
                    </div>
                </div>
            </div>
        </div>

        <div
            v-else-if="showLoadingSpinner"
            class="flex justify-center items-center min-h-[200px]"
        >
            <div
                class="animate-spin rounded-full h-8 w-8 border-b-2 border-accent"
            ></div>
        </div>

        <!-- Settled with no user (and not mid-load): genuine not-found. The
             sub-300ms loading window renders nothing, avoiding a flash. -->
        <div v-else-if="!loading" class="p-6 text-center text-secondary">{{ $t('user-profile-not-found') }}</div>
    </div>
</template>

<style scoped>
.transition-all {
    transition-property: all;
    transition-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
}

.transition-colors {
    transition-property:
        color, background-color, border-color, text-decoration-color, fill,
        stroke;
    transition-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
    transition-duration: 150ms;
}

.transition-opacity {
    transition-property: opacity;
    transition-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
    transition-duration: 200ms;
}

@media (prefers-reduced-motion: reduce) {
    .transition-all,
    .transition-colors,
    .transition-opacity {
        transition: opacity 0.1s ease-in-out;
        transform: none;
    }
}
</style>
