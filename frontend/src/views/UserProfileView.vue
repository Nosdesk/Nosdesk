<script setup lang="ts">
import { formatDate as formatDateUtil } from '@/utils/dateUtils';
import { ref, computed, onMounted, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useAuthStore } from "@/stores/auth";
import UserAvatar from "@/components/UserAvatar.vue";
import BackButton from "@/components/common/BackButton.vue";
import UserProfileCard from "@/components/settings/UserProfileCard.vue";
import UserEmailsCard from "@/components/settings/UserEmailsCard.vue";
import UserAssignedTickets from "@/components/UserAssignedTickets.vue";
import BaseDropdown from "@/components/common/BaseDropdown.vue";
import { RouterLink } from "vue-router";
import userService from "@/services/userService";
import { getDevicesByUser } from "@/services/deviceService";
import { groupService } from "@/services/groupService";
import { useColorFilter } from "@/composables/useColorFilter";
import type { User } from "@/services/userService";
import type { Device } from "@/types/device";
import type { Group } from "@/types/group";

interface UserProfile extends User {
    department?: string;
    joinedDate?: string;
}

interface UserFormData {
    name: string;
    email: string;
    role: string;
    pronouns?: string;
}

const route = useRoute();
const router = useRouter();
const authStore = useAuthStore();
const { colorFilterStyle } = useColorFilter();
const loading = ref(true);
const error = ref<string | null>(null);
const userProfile = ref<UserProfile | null>(null);
const devices = ref<Device[]>([]);
const groups = ref<Group[]>([]);

// Creation and editing state
const isCreationMode = ref(false);
const isNewUser = ref(false);
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
const roleOptions = [
    { value: "user", label: "User" },
    { value: "technician", label: "Technician" },
    { value: "admin", label: "Admin" },
];

// Check permissions
const canEdit = ref(false);
const canEditRole = ref(false);
const isOwnProfile = ref(false);

// Check if the profile user can have assigned tickets (technicians and admins only)
const canHaveAssignedTickets = computed(() => {
    const role = userProfile.value?.role;
    return role === 'technician' || role === 'admin';
});

// Update document title when user profile changes
watch(userProfile, (newProfile) => {
    if (newProfile) {
        document.title = `${newProfile.name}'s Profile | Nosdesk`;
    }
});

// Navigate to group detail page
const navigateToGroup = (group: Group) => {
    router.push(`/groups/${group.uuid}`);
};

const fetchUserData = async () => {
    try {
        loading.value = true;
        error.value = null;

        // Check for creation mode (no UUID parameter)
        if (!route.params.uuid || route.params.uuid === "new") {
            isCreationMode.value = true;
            isNewUser.value = true;

            // Set default values for new user
            editValues.value = {
                name: "",
                email: "",
                role: "user",
                pronouns: "",
            };

            // Enable editing mode for all fields
            editingEmail.value = true;
            editingRole.value = true;
            editingPronouns.value = true;

            // Set permissions for creation
            canEdit.value =
                authStore.isAdmin || authStore.user?.role === "admin";
            canEditRole.value =
                authStore.isAdmin || authStore.user?.role === "admin";

            if (!canEdit.value) {
                error.value = "You do not have permission to create users";
                return;
            }

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
                const nameInput = document.getElementById(
                    "name-input",
                ) as HTMLInputElement;
                if (nameInput) {
                    nameInput.focus();
                }
            }, 100);

            loading.value = false;
            return;
        }

        // Get the UUID from the route params
        const userUuid = route.params.uuid as string;

        if (!userUuid) {
            error.value = "User ID is missing";
            return;
        }

        // Fetch the user from the API
        const user = await userService.getUserByUuid(userUuid);

        if (!user) {
            error.value = "User not found";
            return;
        }

        // Create the user profile with the fetched data
        userProfile.value = {
            ...user,
            department: "IT Support", // Default department (could be added to backend later)
            joinedDate: user.created_at, // Use the actual created_at from the database
        };

        // Set edit values
        editValues.value = {
            name: user.name,
            email: user.email,
            role: user.role,
            pronouns: user.pronouns || "",
        };

        // Check if this is a new user (name starts with "New User")
        isNewUser.value = user.name.startsWith("New User");

        // Set permissions
        const userIsOwnProfile = authStore.user?.uuid === userUuid;
        const isAdmin = authStore.isAdmin || authStore.user?.role === "admin";

        isOwnProfile.value = userIsOwnProfile;
        canEdit.value = userIsOwnProfile || isAdmin;
        canEditRole.value = isAdmin; // Only admins can change roles

        // Name editing is now handled by UserProfileCard
        // No need to focus on name field since it's in the card component

        // Tickets are now loaded by UserAssignedTickets component

        // Load devices from the API
        try {
            devices.value = await getDevicesByUser(userUuid);
        } catch (deviceError) {
            console.error("Error loading devices for user:", deviceError);
            // Don't fail the whole page if devices can't be loaded
            devices.value = [];
        }

        // Load groups for the user (admin only)
        if (authStore.isAdmin || authStore.user?.role === "admin") {
            try {
                groups.value = await groupService.getUserGroups(userUuid);
            } catch (groupError) {
                console.error("Error loading groups for user:", groupError);
                groups.value = [];
            }
        }

        // User emails are now loaded by the UserEmailsCard component
    } catch (e) {
        error.value = "Failed to load user profile";
        console.error("Error loading user profile:", e);
    } finally {
        loading.value = false;
    }
};

const formatDate = (dateString: string) => {
    try {
        const date = new Date(dateString);
        const now = new Date();
        const diffTime = now.getTime() - date.getTime();
        const diffDays = Math.floor(diffTime / (1000 * 60 * 60 * 24));
        const diffHours = Math.floor(diffTime / (1000 * 60 * 60));
        const diffMinutes = Math.floor(diffTime / (1000 * 60));

        if (diffMinutes < 1) {
            return "just now";
        } else if (diffMinutes < 60) {
            return `${diffMinutes} minute${diffMinutes === 1 ? "" : "s"} ago`;
        } else if (diffHours < 24) {
            return `${diffHours} hour${diffHours === 1 ? "" : "s"} ago`;
        } else if (diffDays < 30) {
            return `${diffDays} day${diffDays === 1 ? "" : "s"} ago`;
        } else {
            return formatDateUtil(dateString, "MMM d, yyyy");
        }
    } catch (e) {
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
                    error.value = "Password must be at least 8 characters long";
                    return;
                }
                if (manualPassword.value !== confirmPassword.value) {
                    error.value = "Passwords do not match";
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
                error.value = "User created but navigation failed. Please go to Users list.";
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

            // Update the user profile data
            userProfile.value = { ...userProfile.value, ...updatedUser };

            // Exit edit mode for all fields (name is handled by UserProfileCard)
            editingEmail.value = false;
            editingRole.value = false;
            editingPronouns.value = false;
            isNewUser.value = false;
        }
    } catch (err) {
        console.error("Error saving user:", err);
        // Extract error message - handles both Error objects and other types
        if (err instanceof Error) {
            error.value = err.message;
        } else {
            error.value = "Failed to save user. Please try again.";
        }
    } finally {
        isSaving.value = false;
    }
};

// Note: Name editing is now handled by UserProfileCard component

const handleDeleteUser = async () => {
    if (!userProfile.value) return;

    try {
        await userService.deleteUser(userProfile.value.uuid);
        // Navigate back to users list after successful deletion
        router.push("/users");
    } catch (error) {
        console.error("Error deleting user:", error);
        // TODO: Show error notification to user
    }
};

onMounted(() => {
    fetchUserData();
});

// Re-fetch user data when route params change (e.g., after creating a new user)
watch(
    () => route.params.uuid,
    (newUuid, oldUuid) => {
        if (newUuid !== oldUuid) {
            // Reset state when navigating to a different user
            isCreationMode.value = false;
            isNewUser.value = false;
            editingEmail.value = false;
            editingRole.value = false;
            editingPronouns.value = false;
            fetchUserData();
        }
    }
);
</script>

<template>
    <div class="flex-1">
        <div v-if="userProfile || isCreationMode" class="flex flex-col">
            <!-- Navigation and actions bar -->
            <div class="pt-4 px-6 flex justify-between items-center">
                <BackButton fallbackRoute="/users" label="Back to Users" />
                <div v-if="!isCreationMode" class="flex items-center gap-2">
                    <!-- Own Profile Settings Button -->
                    <RouterLink
                        v-if="isOwnProfile"
                        to="/profile/settings"
                        class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 transition-colors text-sm font-medium flex items-center gap-1"
                    >
                        <svg
                            xmlns="http://www.w3.org/2000/svg"
                            class="h-4 w-4"
                            fill="none"
                            viewBox="0 0 24 24"
                            stroke="currentColor"
                        >
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
                        Profile Settings
                    </RouterLink>

                    <!-- Admin: Manage User Settings Button -->
                    <RouterLink
                        v-else-if="canEditRole && userProfile && !isOwnProfile"
                        :to="`/users/${userProfile.uuid}/settings`"
                        class="px-4 py-2 bg-accent text-white rounded-lg hover:bg-accent-hover transition-colors text-sm font-medium flex items-center gap-2"
                    >
                        <svg
                            xmlns="http://www.w3.org/2000/svg"
                            class="h-4 w-4"
                            fill="none"
                            viewBox="0 0 24 24"
                            stroke="currentColor"
                        >
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
                        User Settings
                    </RouterLink>
                </div>
            </div>

            <div class="flex flex-col gap-4 px-6 py-4 mx-auto w-full max-w-8xl">
                <!-- Error Display -->
                <div
                    v-if="error"
                    class="bg-status-error/10 border border-status-error/30 rounded-xl p-4 flex items-start gap-3"
                >
                    <svg class="w-5 h-5 text-status-error flex-shrink-0 mt-0.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
                    </svg>
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
                                    <svg class="w-5 h-5 text-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M18 9v3m0 0v3m0-3h3m-3 0h-3m-2-5a4 4 0 11-8 0 4 4 0 018 0zM3 20a6 6 0 0112 0v1H3v-1z" />
                                    </svg>
                                </div>
                                <div>
                                    <h1 class="text-lg font-semibold text-primary">Create New User</h1>
                                    <p class="text-sm text-tertiary">Add a new user to your organization</p>
                                </div>
                            </div>
                        </div>

                        <!-- Form Content -->
                        <div class="p-6">
                            <div class="grid grid-cols-1 lg:grid-cols-2 gap-8">
                                <!-- Left Column: Basic Info -->
                                <div class="flex flex-col gap-5">
                                    <h2 class="text-sm font-semibold text-primary flex items-center gap-2">
                                        <svg class="w-4 h-4 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" />
                                        </svg>
                                        Basic Information
                                    </h2>

                                    <!-- Name Field -->
                                    <div class="flex flex-col gap-2">
                                        <label class="text-xs font-medium text-tertiary uppercase tracking-wider">
                                            Full Name <span class="text-status-error">*</span>
                                        </label>
                                        <div class="relative">
                                            <input
                                                id="name-input"
                                                v-model="editValues.name"
                                                type="text"
                                                placeholder="Enter full name"
                                                class="w-full px-4 py-3 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent transition-colors"
                                            />
                                        </div>
                                    </div>

                                    <!-- Email Field -->
                                    <div class="flex flex-col gap-2">
                                        <label class="text-xs font-medium text-tertiary uppercase tracking-wider">
                                            Email Address <span class="text-status-error">*</span>
                                        </label>
                                        <div class="relative">
                                            <input
                                                v-model="editValues.email"
                                                type="email"
                                                placeholder="user@example.com"
                                                class="w-full px-4 py-3 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent transition-colors"
                                            />
                                        </div>
                                    </div>

                                    <!-- Role Field -->
                                    <div class="flex flex-col gap-2">
                                        <label class="text-xs font-medium text-tertiary uppercase tracking-wider">Role</label>
                                        <BaseDropdown
                                            v-model="editValues.role"
                                            :options="roleOptions"
                                            placeholder="Select a role"
                                        />
                                    </div>

                                    <!-- Pronouns Field -->
                                    <div class="flex flex-col gap-2">
                                        <label class="text-xs font-medium text-tertiary uppercase tracking-wider">Pronouns</label>
                                        <input
                                            v-model="editValues.pronouns"
                                            type="text"
                                            placeholder="e.g., he/him, she/her, they/them"
                                            class="w-full px-4 py-3 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent transition-colors"
                                        />
                                    </div>
                                </div>

                                <!-- Right Column: Account Setup -->
                                <div class="flex flex-col gap-5">
                                    <h2 class="text-sm font-semibold text-primary flex items-center gap-2">
                                        <svg class="w-4 h-4 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z" />
                                        </svg>
                                        Account Setup
                                    </h2>

                                    <!-- SMTP Warning Banner -->
                                    <div
                                        v-if="!smtpConfigured"
                                        class="flex items-start gap-3 p-4 bg-status-warning/10 border border-status-warning/20 rounded-lg"
                                    >
                                        <svg class="w-5 h-5 text-status-warning flex-shrink-0 mt-0.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
                                        </svg>
                                        <div class="flex flex-col gap-1">
                                            <span class="text-sm font-medium text-status-warning">Email not configured</span>
                                            <span class="text-xs text-status-warning/80">You must set a password manually since email invitations are unavailable.</span>
                                        </div>
                                    </div>

                                    <!-- Setup Method Selection -->
                                    <div class="flex flex-col gap-3">
                                        <label class="text-xs font-medium text-tertiary uppercase tracking-wider">Setup Method</label>

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
                                                <svg class="w-5 h-5" :class="sendInvitation ? 'text-accent' : 'text-tertiary'" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
                                                </svg>
                                            </div>
                                            <div class="flex-1 min-w-0">
                                                <span class="font-medium" :class="sendInvitation ? 'text-primary' : 'text-secondary'">Send invitation email</span>
                                                <p class="text-xs text-tertiary mt-0.5">User will receive an email with a secure link to set their own password</p>
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
                                                <svg class="w-5 h-5" :class="!sendInvitation ? 'text-accent' : 'text-tertiary'" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
                                                </svg>
                                            </div>
                                            <div class="flex-1 min-w-0">
                                                <span class="font-medium" :class="!sendInvitation ? 'text-primary' : 'text-secondary'">Set password manually</span>
                                                <p class="text-xs text-tertiary mt-0.5">Create a password for the user now and share it with them securely</p>
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
                                                Password <span class="text-status-error">*</span>
                                            </label>
                                            <div class="relative">
                                                <input
                                                    v-model="manualPassword"
                                                    :type="showPassword ? 'text' : 'password'"
                                                    placeholder="Minimum 8 characters"
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
                                                    <svg v-if="showPassword" class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                                                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" />
                                                    </svg>
                                                    <svg v-else class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.88 9.88l-3.29-3.29m7.532 7.532l3.29 3.29M3 3l3.59 3.59m0 0A9.953 9.953 0 0112 5c4.478 0 8.268 2.943 9.543 7a10.025 10.025 0 01-4.132 5.411m0 0L21 21" />
                                                    </svg>
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
                                                Confirm Password <span class="text-status-error">*</span>
                                            </label>
                                            <input
                                                v-model="confirmPassword"
                                                :type="showPassword ? 'text' : 'password'"
                                                placeholder="Re-enter password"
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
                                                <svg
                                                    v-if="manualPassword === confirmPassword && manualPassword.length >= 8"
                                                    class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"
                                                >
                                                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
                                                </svg>
                                                <svg
                                                    v-else
                                                    class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"
                                                >
                                                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
                                                </svg>
                                                {{ manualPassword === confirmPassword && manualPassword.length >= 8 ? 'Passwords match' : 'Passwords do not match' }}
                                            </p>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </div>

                        <!-- Card Footer with Actions -->
                        <div class="px-6 py-4 bg-surface-alt border-t border-default flex items-center justify-between">
                            <p class="text-xs text-tertiary">
                                <span class="text-status-error">*</span> Required fields
                            </p>
                            <div class="flex items-center gap-3">
                                <button
                                    @click="router.push('/users')"
                                    :disabled="isSaving"
                                    class="px-5 py-2.5 text-sm font-medium text-secondary hover:text-primary bg-transparent hover:bg-surface-hover rounded-lg transition-colors disabled:opacity-50"
                                >
                                    Cancel
                                </button>
                                <button
                                    @click="saveUser"
                                    :disabled="
                                        isSaving ||
                                        !editValues.name ||
                                        !editValues.email ||
                                        (!sendInvitation && (manualPassword.length < 8 || manualPassword !== confirmPassword))
                                    "
                                    class="px-5 py-2.5 text-sm font-medium text-white bg-accent hover:bg-accent-hover rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2"
                                >
                                    <svg
                                        v-if="isSaving"
                                        class="w-4 h-4 animate-spin"
                                        fill="none"
                                        viewBox="0 0 24 24"
                                    >
                                        <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
                                        <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 004 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
                                    </svg>
                                    <svg v-else class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6v6m0 0v6m0-6h6m-6 0H6" />
                                    </svg>
                                    {{ isSaving ? "Creating..." : "Create User" }}
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

                        <!-- Devices Section -->
                        <div
                            class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden"
                        >
                            <div
                                class="px-4 py-3 bg-surface-alt border-b border-default"
                            >
                                <h2 class="text-lg font-medium text-primary">
                                    Devices
                                </h2>
                            </div>
                            <div class="p-3">
                                <div
                                    v-if="devices.length === 0"
                                    class="text-secondary text-sm"
                                >
                                    No devices
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
                                                        "Unknown"
                                                    }}
                                                    {{ device.model }}
                                                </p>
                                                <p
                                                    class="text-xs text-tertiary"
                                                >
                                                    Last updated
                                                    {{
                                                        formatDate(
                                                            device.updated_at,
                                                        )
                                                    }}
                                                </p>
                                            </div>
                                            <div class="flex-shrink-0 ml-3">
                                                <span
                                                    class="text-xs px-2 py-1 rounded-full"
                                                    :class="{
                                                        'text-status-success bg-status-success/20':
                                                            device.warranty_status ===
                                                            'Active',
                                                        'text-status-warning bg-status-warning/20':
                                                            device.warranty_status ===
                                                            'Warning',
                                                        'text-status-error bg-status-error/20':
                                                            device.warranty_status ===
                                                            'Expired',
                                                        'text-secondary bg-surface-alt':
                                                            device.warranty_status ===
                                                            'Unknown',
                                                    }"
                                                >
                                                    {{ device.warranty_status }}
                                                </span>
                                            </div>
                                        </div>
                                    </RouterLink>
                                </div>
                            </div>
                        </div>

                        <!-- Groups Section (Admin only) -->
                        <div
                            v-if="groups.length > 0"
                            class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden"
                        >
                            <div
                                class="px-4 py-3 bg-surface-alt border-b border-default"
                            >
                                <h2 class="text-lg font-medium text-primary">
                                    Groups
                                </h2>
                            </div>
                            <div class="p-3">
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
                                        <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                                            <path stroke-linecap="round" stroke-linejoin="round" d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z" />
                                        </svg>
                                        {{ group.name }}
                                    </button>
                                </div>
                            </div>
                        </div>
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
            v-else-if="loading"
            class="flex justify-center items-center min-h-[200px]"
        >
            <div
                class="animate-spin rounded-full h-8 w-8 border-b-2 border-accent"
            ></div>
        </div>

        <div v-else class="p-6 text-center text-secondary">User not found</div>
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
