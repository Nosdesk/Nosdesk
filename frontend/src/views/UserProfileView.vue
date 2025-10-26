<script setup lang="ts">
import { ref, onMounted, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useAuthStore } from "@/stores/auth";
import StatusBadge from "@/components/StatusBadge.vue";
import UserAvatar from "@/components/UserAvatar.vue";
import BackButton from "@/components/common/BackButton.vue";
import DeleteButton from "@/components/common/DeleteButton.vue";
import UserProfileCard from "@/components/settings/UserProfileCard.vue";
import UserEmailsCard from "@/components/settings/UserEmailsCard.vue";
import { RouterLink } from "vue-router";
import ticketService from "@/services/ticketService";
import userService from "@/services/userService";
import { getDevicesByUser } from "@/services/deviceService";
import type { Ticket } from "@/services/ticketService";
import type { User } from "@/services/userService";
import type { Device } from "@/types/device";

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
const loading = ref(true);
const error = ref<string | null>(null);
const userProfile = ref<UserProfile | null>(null);
const assignedTickets = ref<Ticket[]>([]);
const requestedTickets = ref<Ticket[]>([]);
const devices = ref<Device[]>([]);

// Creation and editing state
const isCreationMode = ref(false);
const isNewUser = ref(false);
const editingEmail = ref(false);
const editingRole = ref(false);
const editingPronouns = ref(false);
const isSaving = ref(false);

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

// Update document title when user profile changes
watch(userProfile, (newProfile) => {
    if (newProfile) {
        document.title = `${newProfile.name}'s Profile | Nosdesk`;
    }
});

const fetchUserData = async () => {
    try {
        loading.value = true;
        error.value = null;

        // Check if we're in creation mode (no UUID parameter)
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

        // Load tickets from the API
        const allTickets = await ticketService.getTickets();

        // Filter tickets for this user
        assignedTickets.value = allTickets.filter(
            (t) => t.assignee === userUuid,
        );
        requestedTickets.value = allTickets.filter(
            (t) => t.requester === userUuid,
        );

        // Load devices from the API
        try {
            devices.value = await getDevicesByUser(userUuid);
        } catch (deviceError) {
            console.error("Error loading devices for user:", deviceError);
            // Don't fail the whole page if devices can't be loaded
            devices.value = [];
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
            return date.toLocaleDateString("en-US", {
                year: "numeric",
                month: "long",
                day: "numeric",
            });
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
            // Create new user
            const userData = {
                name: editValues.value.name,
                email: editValues.value.email,
                role: editValues.value.role,
                pronouns: editValues.value.pronouns,
            };

            const newUser = await userService.createUser(userData);

            if (!newUser) {
                error.value = "Failed to create user. Please try again.";
                return;
            }

            // Navigate to the newly created user (replace history so back button goes to users list)
            router.replace(`/users/${newUser.uuid}`);
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
        error.value = "Failed to save user. Please try again.";
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
                        class="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors text-sm font-medium flex items-center gap-1"
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
                        class="px-4 py-2 bg-purple-600 text-white rounded-lg hover:bg-purple-700 transition-colors text-sm font-medium flex items-center gap-2"
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
                        Manage User Settings
                    </RouterLink>
                </div>
            </div>

            <div class="flex flex-col gap-4 px-6 py-4 mx-auto w-full max-w-8xl">
                <!-- Creation Mode Header -->
                <div
                    v-if="isCreationMode"
                    class="bg-slate-800 rounded-xl border border-slate-700/50 hover:border-slate-600/50 transition-colors"
                >
                    <div
                        class="px-4 py-3 bg-slate-700/30 border-b border-slate-700/50"
                    >
                        <h1 class="text-lg font-medium text-white">
                            Create New User
                        </h1>
                        <p class="text-slate-400 text-sm mt-1">
                            Enter user details below
                        </p>
                    </div>
                </div>

                <!-- Error Display -->
                <div
                    v-if="error"
                    class="bg-red-900/30 border border-red-700 rounded-lg p-4 text-red-200 text-sm"
                >
                    {{ error }}
                </div>

                <!-- User Creation Form -->
                <div
                    v-if="isCreationMode"
                    class="grid grid-cols-1 xl:grid-cols-2 gap-4"
                >
                    <!-- Basic Information -->
                    <div
                        class="bg-slate-800 rounded-xl border border-slate-700/50 hover:border-slate-600/50 transition-colors"
                    >
                        <div
                            class="px-4 py-3 bg-slate-700/30 border-b border-slate-700/50"
                        >
                            <h2 class="text-lg font-medium text-white">
                                Basic Information
                            </h2>
                        </div>
                        <div class="p-3">
                            <div class="flex flex-col gap-3">
                                <!-- Name -->
                                <div class="flex flex-col gap-1.5">
                                    <h3
                                        class="text-xs font-medium text-slate-400 uppercase tracking-wide"
                                    >
                                        Name *
                                    </h3>
                                    <div
                                        class="bg-slate-700/50 rounded-lg border border-slate-600/30 hover:border-slate-500/50 transition-colors"
                                    >
                                        <input
                                            v-model="editValues.name"
                                            type="text"
                                            placeholder="Enter user name"
                                            class="w-full bg-transparent border-none rounded-lg px-3 py-2.5 text-white placeholder-slate-400 focus:outline-none focus:ring-2 focus:ring-blue-500/50"
                                        />
                                    </div>
                                </div>

                                <!-- Email -->
                                <div class="flex flex-col gap-1.5">
                                    <h3
                                        class="text-xs font-medium text-slate-400 uppercase tracking-wide"
                                    >
                                        Email *
                                    </h3>
                                    <div
                                        class="bg-slate-700/50 rounded-lg border border-slate-600/30 hover:border-slate-500/50 transition-colors"
                                    >
                                        <input
                                            v-model="editValues.email"
                                            type="email"
                                            placeholder="Enter email address"
                                            class="w-full bg-transparent border-none rounded-lg px-3 py-2.5 text-white placeholder-slate-400 focus:outline-none focus:ring-2 focus:ring-blue-500/50"
                                        />
                                    </div>
                                </div>

                                <!-- Role -->
                                <div class="flex flex-col gap-1.5">
                                    <h3
                                        class="text-xs font-medium text-slate-400 uppercase tracking-wide"
                                    >
                                        Role
                                    </h3>
                                    <div
                                        class="bg-slate-700/50 rounded-lg border border-slate-600/30 hover:border-slate-500/50 transition-colors"
                                    >
                                        <select
                                            v-model="editValues.role"
                                            class="w-full bg-transparent border-none rounded-lg px-3 py-2.5 text-white focus:outline-none focus:ring-2 focus:ring-blue-500/50"
                                        >
                                            <option
                                                v-for="option in roleOptions"
                                                :key="option.value"
                                                :value="option.value"
                                                class="bg-slate-700"
                                            >
                                                {{ option.label }}
                                            </option>
                                        </select>
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>

                    <!-- Additional Details -->
                    <div
                        class="bg-slate-800 rounded-xl border border-slate-700/50 hover:border-slate-600/50 transition-colors"
                    >
                        <div
                            class="px-4 py-3 bg-slate-700/30 border-b border-slate-700/50"
                        >
                            <h2 class="text-lg font-medium text-white">
                                Additional Details
                            </h2>
                        </div>
                        <div class="p-3">
                            <div class="flex flex-col gap-3">
                                <!-- Pronouns -->
                                <div class="flex flex-col gap-1.5">
                                    <h3
                                        class="text-xs font-medium text-slate-400 uppercase tracking-wide"
                                    >
                                        Pronouns
                                    </h3>
                                    <div
                                        class="bg-slate-700/50 rounded-lg border border-slate-600/30 hover:border-slate-500/50 transition-colors"
                                    >
                                        <input
                                            v-model="editValues.pronouns"
                                            type="text"
                                            placeholder="e.g., he/him, she/her, they/them"
                                            class="w-full bg-transparent border-none rounded-lg px-3 py-2.5 text-white placeholder-slate-400 focus:outline-none focus:ring-2 focus:ring-blue-500/50"
                                        />
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>

                    <!-- Creation Mode Buttons -->
                    <div class="flex justify-end gap-3">
                        <button
                            @click="router.push('/users')"
                            :disabled="isSaving"
                            class="px-6 py-2.5 bg-slate-600 text-white rounded-lg hover:bg-slate-700 disabled:opacity-50 transition-colors text-sm font-medium"
                        >
                            Cancel
                        </button>
                        <button
                            @click="saveUser"
                            :disabled="
                                isSaving ||
                                !editValues.name ||
                                !editValues.email
                            "
                            class="px-6 py-2.5 bg-green-600 text-white rounded-lg hover:bg-green-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors text-sm font-medium flex items-center gap-2"
                        >
                            <svg
                                v-if="isSaving"
                                class="w-4 h-4 animate-spin"
                                fill="none"
                                viewBox="0 0 24 24"
                            >
                                <circle
                                    class="opacity-25"
                                    cx="12"
                                    cy="12"
                                    r="10"
                                    stroke="currentColor"
                                    stroke-width="4"
                                ></circle>
                                <path
                                    class="opacity-75"
                                    fill="currentColor"
                                    d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 004 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                                ></path>
                            </svg>
                            {{ isSaving ? "Creating..." : "Create User" }}
                        </button>
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
                            class="bg-slate-800 rounded-xl border border-slate-700/50 hover:border-slate-600/50 transition-colors"
                        >
                            <div
                                class="px-4 py-3 bg-slate-700/30 border-b border-slate-700/50"
                            >
                                <h2 class="text-lg font-medium text-white">
                                    Devices
                                </h2>
                            </div>
                            <div class="p-3">
                                <div
                                    v-if="devices.length === 0"
                                    class="text-slate-400 text-sm"
                                >
                                    No devices
                                </div>
                                <div v-else class="flex flex-col gap-3">
                                    <RouterLink
                                        v-for="device in devices"
                                        :key="device.id"
                                        :to="`/devices/${device.id}`"
                                        class="block bg-slate-700/50 p-3 rounded-lg hover:bg-slate-700 transition-colors"
                                    >
                                        <div
                                            class="flex items-start justify-between"
                                        >
                                            <div class="flex-1">
                                                <h3
                                                    class="font-medium text-white"
                                                >
                                                    {{ device.name }}
                                                </h3>
                                                <p
                                                    class="text-sm text-slate-400"
                                                >
                                                    {{
                                                        device.manufacturer ||
                                                        "Unknown"
                                                    }}
                                                    {{ device.model }}
                                                </p>
                                                <p
                                                    class="text-xs text-slate-500"
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
                                                        'text-green-400 bg-green-900/20':
                                                            device.warranty_status ===
                                                            'Active',
                                                        'text-yellow-400 bg-yellow-900/20':
                                                            device.warranty_status ===
                                                            'Warning',
                                                        'text-red-400 bg-red-900/20':
                                                            device.warranty_status ===
                                                            'Expired',
                                                        'text-gray-400 bg-gray-900/20':
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
                    </div>

                    <!-- Tickets Area -->
                    <div class="flex flex-col gap-4 xl:w-1/2 xl:min-w-0">
                        <!-- Assigned Tickets -->
                        <div
                            class="bg-slate-800 rounded-xl border border-slate-700/50 hover:border-slate-600/50 transition-colors"
                        >
                            <div
                                class="px-4 py-3 bg-slate-700/30 border-b border-slate-700/50 flex justify-between items-center"
                            >
                                <h2 class="text-lg font-medium text-white">
                                    {{ assignedTickets.length }} Assigned
                                    Tickets
                                </h2>
                                <RouterLink
                                    :to="`/tickets?assignee=${route.params.uuid}`"
                                    class="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors text-sm font-medium"
                                >
                                    See All
                                </RouterLink>
                            </div>
                            <div class="p-3">
                                <div
                                    v-if="assignedTickets.length === 0"
                                    class="text-slate-400 text-sm"
                                >
                                    No assigned tickets
                                </div>
                                <div v-else class="flex flex-col gap-3">
                                    <RouterLink
                                        v-for="ticket in assignedTickets.slice(
                                            0,
                                            5,
                                        )"
                                        :key="ticket.id"
                                        :to="`/tickets/${ticket.id}`"
                                        class="block bg-slate-700/50 p-3 rounded-lg hover:bg-slate-700 transition-colors"
                                    >
                                        <div
                                            class="flex items-center justify-between"
                                        >
                                            <div>
                                                <h3
                                                    class="font-medium text-white"
                                                >
                                                    {{ ticket.title }}
                                                </h3>
                                                <p
                                                    class="text-sm text-slate-400"
                                                >
                                                    {{
                                                        formatDate(
                                                            ticket.created,
                                                        )
                                                    }}
                                                </p>
                                            </div>
                                            <StatusBadge
                                                type="status"
                                                :value="
                                                    ticket.status as
                                                        | 'open'
                                                        | 'in-progress'
                                                        | 'closed'
                                                "
                                            />
                                        </div>
                                    </RouterLink>
                                </div>
                            </div>
                        </div>

                        <!-- Requested Tickets -->
                        <div
                            class="bg-slate-800 rounded-xl border border-slate-700/50 hover:border-slate-600/50 transition-colors"
                        >
                            <div
                                class="px-4 py-3 bg-slate-700/30 border-b border-slate-700/50 flex justify-between items-center"
                            >
                                <h2 class="text-lg font-medium text-white">
                                    {{ requestedTickets.length }} Requested
                                    Tickets
                                </h2>
                                <RouterLink
                                    :to="`/tickets?requester=${route.params.uuid}`"
                                    class="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors text-sm font-medium"
                                >
                                    See All
                                </RouterLink>
                            </div>
                            <div class="p-3">
                                <div
                                    v-if="requestedTickets.length === 0"
                                    class="text-slate-400 text-sm"
                                >
                                    No requested tickets
                                </div>
                                <div v-else class="flex flex-col gap-3">
                                    <RouterLink
                                        v-for="ticket in requestedTickets.slice(
                                            0,
                                            5,
                                        )"
                                        :key="ticket.id"
                                        :to="`/tickets/${ticket.id}`"
                                        class="block bg-slate-700/50 p-3 rounded-lg hover:bg-slate-700 transition-colors"
                                    >
                                        <div
                                            class="flex items-center justify-between"
                                        >
                                            <div>
                                                <h3
                                                    class="font-medium text-white"
                                                >
                                                    {{ ticket.title }}
                                                </h3>
                                                <p
                                                    class="text-sm text-slate-400"
                                                >
                                                    {{
                                                        formatDate(
                                                            ticket.created,
                                                        )
                                                    }}
                                                </p>
                                            </div>
                                            <StatusBadge
                                                type="status"
                                                :value="
                                                    ticket.status as
                                                        | 'open'
                                                        | 'in-progress'
                                                        | 'closed'
                                                "
                                            />
                                        </div>
                                    </RouterLink>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>

        <div
            v-else-if="loading"
            class="flex justify-center items-center min-h-[200px]"
        >
            <div
                class="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500"
            ></div>
        </div>

        <div v-else class="p-6 text-center text-slate-400">User not found</div>
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
