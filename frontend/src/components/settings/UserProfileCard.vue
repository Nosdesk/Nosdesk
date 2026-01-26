<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { RouterLink } from "vue-router";
import { useAuthStore } from "@/stores/auth";
import UserAvatar from "@/components/UserAvatar.vue";
import InlineEdit from "@/components/common/InlineEdit.vue";
import LoadingSpinner from "@/components/common/LoadingSpinner.vue";
import userService from "@/services/userService";
import uploadService from "@/services/uploadService";
import type { User } from "@/types/user";

interface UserAvatarComponentType {
    refreshUser: (uuid?: string) => Promise<void>;
}

const authStore = useAuthStore();
const loading = ref(false);
const userAvatarComponent = ref<UserAvatarComponentType | null>(null);

// File inputs
const fileInput = ref<HTMLInputElement | null>(null);
const bannerFileInput = ref<HTMLInputElement | null>(null);

// Upload progress states
const avatarUploading = ref(false);
const bannerUploading = ref(false);
const avatarProgress = ref(0);
const bannerProgress = ref(0);

// Form data
const formData = ref({
    name: "",
    email: "",
    pronouns: "",
    avatar_url: "",
    banner_url: "",
});

// Original data for comparison
const originalData = ref({
    name: "",
    email: "",
});

// Emits
const emit = defineEmits<{
    (e: "success", message: string): void;
    (e: "error", message: string): void;
}>();

// Props for external control
const props = withDefaults(
    defineProps<{
        user?: User; // User data to display (if different from auth user)
        canEdit?: boolean; // Whether editing is allowed
        showEditableFields?: boolean; // Whether to show editable fields
        variant?: 'full' | 'compact'; // Display variant
        showBanner?: boolean; // Whether to show banner
        showPronouns?: boolean; // Whether to show pronouns
        showEmail?: boolean; // Whether to show email
        enableAvatarNavigation?: boolean; // Whether clicking avatar navigates to profile
    }>(),
    {
        canEdit: false,
        showEditableFields: false,
        variant: 'full',
        showBanner: true,
        showPronouns: true,
        showEmail: true,
        enableAvatarNavigation: false,
    },
);

// Use provided user or fallback to auth user
const displayUser = computed(() => props.user || authStore.user);

// Computed property for determining if component should be in edit mode
const isEditable = computed(() => props.canEdit && props.showEditableFields);

// Computed properties for variant-based styling
const isCompact = computed(() => props.variant === 'compact');

const bannerHeight = computed(() => isCompact.value ? 'h-20' : 'h-32 sm:h-40');

const avatarSize = computed(() => isCompact.value ? 'w-20 h-20' : 'w-28 h-28 sm:w-36 sm:h-36');

const avatarOffset = computed(() => isCompact.value ? '-top-10' : '-top-14 sm:-top-12');

const contentPadding = computed(() => isCompact.value ? 'pt-12' : 'pt-16 sm:pt-20');

// Editing states (name editing handled by InlineEdit component)
const editingEmail = ref(false);
const editingPronouns = ref(false);

// Computed properties to check if fields have been modified
const nameModified = computed(
    () =>
        formData.value.name !== originalData.value.name &&
        formData.value.name.trim() !== "",
);

const emailModified = computed(
    () =>
        formData.value.email !== originalData.value.email &&
        formData.value.email.trim() !== "",
);

const pronounsModified = computed(() => {
    const originalPronouns = displayUser.value?.pronouns || "";
    return (
        formData.value.pronouns !== originalPronouns &&
        formData.value.pronouns !== undefined
    );
});

// Watch for user data changes
watch(
    () => displayUser.value,
    (newUserData) => {
        if (newUserData) {
            formData.value.name = newUserData.name || "";
            formData.value.email = newUserData.email || "";
            formData.value.pronouns = newUserData.pronouns || "";
            formData.value.avatar_url = newUserData.avatar_url || "";
            formData.value.banner_url = newUserData.banner_url || "";

            originalData.value.name = newUserData.name || "";
            originalData.value.email = newUserData.email || "";
        }
    },
    { immediate: true },
);

// File handling functions
const handleAvatarClick = () => {
    fileInput.value?.click();
};

const handleBannerClick = () => {
    bannerFileInput.value?.click();
};

const handleFileChange = async (event: Event) => {
    const input = event.target as HTMLInputElement;
    if (!input.files?.length) return;

    let file = input.files[0];

    const validation = uploadService.validateFile(file, {
        allowedTypes: ["image/*"],
    });
    if (!validation.valid) {
        emit("error", validation.error || "Invalid file");
        return;
    }

    try {
        file = await uploadService.convertHeicToJpeg(file, (message) => {
            emit("success", message);
        });
        await uploadImage(file, "avatar");
    } catch (error) {
        const err = error as Error;
        emit("error", err.message || "Failed to process image");
    }
};

const handleBannerChange = async (event: Event) => {
    const input = event.target as HTMLInputElement;
    if (!input.files?.length) return;

    let file = input.files[0];

    const validation = uploadService.validateFile(file, {
        allowedTypes: ["image/*"],
    });
    if (!validation.valid) {
        emit("error", validation.error || "Invalid file");
        return;
    }

    try {
        file = await uploadService.convertHeicToJpeg(file, (message) => {
            emit("success", message);
        });
        await uploadImage(file, "banner");
    } catch (error) {
        const err = error as Error;
        emit("error", err.message || "Failed to process image");
    }
};

const uploadImage = async (file: File, type: "avatar" | "banner") => {
    loading.value = true;

    // Reset and set uploading state
    if (type === "avatar") {
        avatarUploading.value = true;
        avatarProgress.value = 0;
    } else {
        bannerUploading.value = true;
        bannerProgress.value = 0;
    }

    // Progress callback
    const onProgress = (progress: number) => {
        if (type === "avatar") {
            avatarProgress.value = progress;
        } else {
            bannerProgress.value = progress;
        }
    };

    try {
        const targetUserUuid = displayUser.value?.uuid;
        if (!targetUserUuid) {
            emit("error", "User UUID not found");
            return;
        }

        const uploadedUrl = await userService.uploadImage(
            file,
            type,
            targetUserUuid,
            onProgress,
        );

        if (!uploadedUrl) {
            emit("error", `Failed to upload ${type}`);
            return;
        }

        const successMessage =
            type === "avatar"
                ? "Profile picture updated successfully"
                : "Cover image updated successfully";
        emit("success", successMessage);

        // Add cache-busting parameter to force browser to reload the image
        // Handle URLs that may already have query params
        const separator = uploadedUrl.includes('?') ? '&' : '?';
        const cacheBustedUrl = `${uploadedUrl}${separator}_cb=${Date.now()}`;

        // Update form data
        if (type === "avatar") {
            formData.value.avatar_url = cacheBustedUrl;
        } else {
            formData.value.banner_url = cacheBustedUrl;
        }

        // Only update auth store if editing the current user
        const isCurrentUser = authStore.user?.uuid === targetUserUuid;
        if (isCurrentUser && authStore.user) {
            authStore.user = {
                ...authStore.user,
                [type === "avatar" ? "avatar_url" : "banner_url"]:
                    cacheBustedUrl,
            };

            if (userAvatarComponent.value?.refreshUser) {
                userAvatarComponent.value.refreshUser(targetUserUuid);
            }
        }
    } catch (err) {
        emit("error", `Failed to update ${type}`);
        console.error(`Error updating ${type}:`, err);
    } finally {
        loading.value = false;
        if (type === "avatar") {
            avatarUploading.value = false;
        } else {
            bannerUploading.value = false;
        }
    }
};

// Update functions
const updateName = async () => {
    if (!nameModified.value) return;

    loading.value = true;

    try {
        const userUuid = displayUser.value?.uuid;
        if (!userUuid) {
            emit("error", "User not authenticated");
            return;
        }

        const updatedUser = await userService.updateUser(userUuid, {
            name: formData.value.name,
        });

        if (updatedUser) {
            emit("success", "Name updated successfully");
            originalData.value.name = formData.value.name;

            // Update auth store if this is the current user
            if (authStore.user?.uuid === userUuid && authStore.user) {
                authStore.user = { ...authStore.user, name: updatedUser.name };
            }
        } else {
            emit("error", "Failed to update name");
        }
    } catch (err) {
        emit("error", "Failed to update name");
        console.error("Error updating name:", err);
    } finally {
        loading.value = false;
    }
};

const updateEmail = async () => {
    if (!emailModified.value) return;

    loading.value = true;

    try {
        const userUuid = displayUser.value?.uuid;
        if (!userUuid) {
            emit("error", "User not authenticated");
            return;
        }

        const updatedUser = await userService.updateUser(userUuid, {
            email: formData.value.email,
        });

        if (updatedUser) {
            emit("success", "Email updated successfully");
            originalData.value.email = formData.value.email;
            editingEmail.value = false;

            // Update auth store if this is the current user
            if (authStore.user?.uuid === userUuid && authStore.user) {
                authStore.user = {
                    ...authStore.user,
                    email: updatedUser.email,
                };
            }
        } else {
            emit("error", "Failed to update email");
        }
    } catch (err) {
        emit("error", "Failed to update email");
        console.error("Error updating email:", err);
    } finally {
        loading.value = false;
    }
};

const updatePronouns = async () => {
    if (!formData.value.pronouns && !displayUser.value?.pronouns) return;

    loading.value = true;

    try {
        const userUuid = displayUser.value?.uuid;
        if (!userUuid) {
            emit("error", "User not authenticated");
            return;
        }

        const updatedUser = await userService.updateUser(userUuid, {
            pronouns: formData.value.pronouns,
        });

        if (updatedUser) {
            emit("success", "Pronouns updated successfully");
            editingPronouns.value = false;

            // Update auth store if this is the current user
            if (authStore.user?.uuid === userUuid && authStore.user) {
                authStore.user = {
                    ...authStore.user,
                    pronouns: updatedUser.pronouns,
                };
            }
        } else {
            emit("error", "Failed to update pronouns");
        }
    } catch (err) {
        emit("error", "Failed to update pronouns");
        console.error("Error updating pronouns:", err);
    } finally {
        loading.value = false;
    }
};

// Handle name updates from InlineEdit component
const handleNameUpdate = (newName: string) => {
    if (newName !== originalData.value.name && newName.trim() !== "") {
        updateName();
    }
};

// Cancel editing functions
const cancelEdit = (field: "email" | "pronouns") => {
    const originalUser = displayUser.value;
    if (!originalUser) return;

    switch (field) {
        case "email":
            formData.value.email = originalUser.email || "";
            editingEmail.value = false;
            break;
        case "pronouns":
            formData.value.pronouns = originalUser.pronouns || "";
            editingPronouns.value = false;
            break;
    }
};

// Role badge styling functions
const getRoleBadgeClass = (role: string) => {
    // Easter egg: Purple shiny badge for Kyle Phillips
    if (
        role === "admin" &&
        displayUser.value?.name === "Kyle Phillips" &&
        displayUser.value?.email?.endsWith("@kyle.au")
    ) {
        return "bg-purple-600/20 text-purple-400 developer-badge";
    }

    switch (role) {
        case "admin":
            return "bg-status-error/20 text-status-error";
        case "technician":
            return "bg-accent/20 text-accent";
        case "user":
        default:
            return "bg-surface-hover/20 text-secondary";
    }
};

const getRoleDisplayName = (role: string) => {
    // Easter egg: Special role for Kyle Phillips
    if (
        role === "admin" &&
        displayUser.value?.name === "Kyle Phillips" &&
        displayUser.value?.email?.endsWith("@kyle.au")
    ) {
        return "Developer";
    }

    switch (role) {
        case "admin":
            return "Administrator";
        case "technician":
            return "Technician";
        case "user":
        default:
            return "User";
    }
};
</script>

<template>
    <div
        class="profile-card-themed bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden"
    >
        <!-- Cover/Banner Image -->
        <div
            v-if="showBanner"
            class="profile-banner bg-surface-alt relative overflow-hidden"
            :class="bannerHeight"
        >
            <!-- Skeleton during upload -->
            <div
                v-if="bannerUploading"
                class="absolute inset-0 bg-surface-alt"
            >
                <!-- Progress bar at bottom -->
                <div class="absolute bottom-0 left-0 right-0 h-1 bg-black/20">
                    <div
                        class="h-full bg-accent transition-all duration-150"
                        :style="{ width: `${bannerProgress}%` }"
                    />
                </div>
                <!-- Centered spinner with progress -->
                <div class="absolute inset-0 flex items-center justify-center text-secondary">
                    <div class="flex flex-col items-center gap-2">
                        <LoadingSpinner variant="inline" />
                        <span class="text-sm font-medium">{{ bannerProgress }}%</span>
                    </div>
                </div>
            </div>
            <!-- Actual banner image (hidden during upload) -->
            <!-- :key forces Vue to recreate element when URL changes, bypassing browser cache -->
            <img
                v-else-if="formData.banner_url"
                :key="formData.banner_url"
                :src="formData.banner_url"
                alt="Profile banner"
                class="absolute inset-0 w-full h-full object-cover"
            />
            <!-- Default accent background when no banner -->
            <div
                v-else
                class="absolute inset-0 bg-accent"
            />
            <!-- Banner upload button (only when editable) -->
            <button
                v-if="isEditable && !bannerUploading"
                class="absolute bottom-2 right-2 bg-surface/50 hover:bg-surface/80 text-white rounded-full w-11 h-11 flex items-center justify-center transition-colors z-10"
                @click="handleBannerClick"
            >
                <svg
                    xmlns="http://www.w3.org/2000/svg"
                    class="h-6 w-6"
                    fill="none"
                    viewBox="0 0 24 24"
                    stroke="currentColor"
                >
                    <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M3 9a2 2 0 012-2h.93a2 2 0 001.664-.89l.812-1.22A2 2 0 0110.07 4h3.86a2 2 0 011.664.89l.812 1.22A2 2 0 0018.07 7H19a2 2 0 012 2v9a2 2 0 01-2 2H5a2 2 0 01-2-2V9z"
                    />
                    <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M15 13a3 3 0 11-6 0 3 3 0 016 0z"
                    />
                </svg>
            </button>
            <input
                v-if="isEditable"
                ref="bannerFileInput"
                type="file"
                accept="image/*"
                class="hidden"
                @change="handleBannerChange"
            />
        </div>

        <!-- Profile Content -->
        <div class="px-4 sm:px-6 relative">
            <!-- Avatar (overlaps banner or positioned at top if no banner) -->
            <!-- Clickable avatar with RouterLink when navigation is enabled and not editing -->
            <RouterLink
                v-if="enableAvatarNavigation && !isEditable && displayUser?.uuid"
                :to="`/users/${displayUser.uuid}`"
                class="block rounded-full overflow-hidden border-4 border-surface shadow-lg hover:ring-2 hover:ring-accent transition-all z-30"
                :class="[
                    avatarSize,
                    showBanner ? `absolute ${avatarOffset} left-3 sm:left-4` : 'mx-auto mt-4'
                ]"
            >
                <UserAvatar
                    :name="displayUser?.name || ''"
                    size="full"
                    :avatar="formData.avatar_url || null"
                    :showName="false"
                    :clickable="false"
                    class="w-full h-full"
                />
            </RouterLink>
            <!-- Non-clickable avatar for edit mode or when navigation is disabled -->
            <div
                v-else
                class="rounded-full overflow-hidden border-4 border-surface shadow-lg z-30"
                :class="[
                    avatarSize,
                    showBanner ? `absolute ${avatarOffset} left-3 sm:left-4` : 'mx-auto mt-4',
                    { 'cursor-pointer': isEditable }
                ]"
                @click="isEditable ? handleAvatarClick() : undefined"
            >
                <UserAvatar
                    :name="displayUser?.name || ''"
                    size="full"
                    :avatar="formData.avatar_url || null"
                    :showName="false"
                    :clickable="false"
                    class="w-full h-full"
                    ref="userAvatarComponent"
                />
                <!-- Avatar upload loading overlay with progress -->
                <div
                    v-if="avatarUploading"
                    class="absolute inset-0 bg-surface-alt flex flex-col items-center justify-center rounded-full z-10 text-secondary"
                >
                    <LoadingSpinner variant="inline" size="sm" />
                    <span class="text-xs font-medium mt-1">{{ avatarProgress }}%</span>
                </div>
                <!-- Hover overlay for editing -->
                <div
                    v-if="isEditable && !avatarUploading"
                    class="absolute inset-0 bg-black/50 flex items-center justify-center opacity-0 hover:opacity-100 transition-opacity rounded-full"
                >
                    <div class="text-white flex flex-col items-center gap-1">
                        <svg
                            xmlns="http://www.w3.org/2000/svg"
                            class="h-8 w-8"
                            fill="none"
                            viewBox="0 0 24 24"
                            stroke="currentColor"
                        >
                            <path
                                stroke-linecap="round"
                                stroke-linejoin="round"
                                stroke-width="2"
                                d="M3 9a2 2 0 012-2h.93a2 2 0 001.664-.89l.812-1.22A2 2 0 0110.07 4h3.86a2 2 0 011.664.89l.812 1.22A2 2 0 0018.07 7H19a2 2 0 012 2v9a2 2 0 01-2 2H5a2 2 0 01-2-2V9z"
                            />
                            <path
                                stroke-linecap="round"
                                stroke-linejoin="round"
                                stroke-width="2"
                                d="M15 13a3 3 0 11-6 0 3 3 0 016 0z"
                            />
                        </svg>
                        <span class="text-xs">Change Photo</span>
                    </div>
                </div>
                <input
                    v-if="isEditable"
                    ref="fileInput"
                    type="file"
                    accept="image/*"
                    class="hidden"
                    @change="handleFileChange"
                />
            </div>

            <!-- EDITABLE MODE -->
            <template v-if="isEditable">
                <!-- Name and role badge - positioned to the right of avatar -->
                <div
                    class="flex flex-wrap items-center gap-3 pb-4 sm:pb-0"
                    :class="showBanner ? 'pt-16 sm:py-6 sm:pl-[9.5rem]' : 'pt-4'"
                >
                    <!-- Left: Name with inline edit -->
                    <div class="min-w-0 flex-1 basis-48 name-input-field">
                        <InlineEdit
                            v-model="formData.name"
                            :placeholder="
                                displayUser?.name || 'Enter name...'
                            "
                            text-size="2xl"
                            :can-edit="true"
                            @update:modelValue="handleNameUpdate"
                        />
                    </div>

                    <!-- Right: Role badge -->
                    <div
                        class="px-3 py-1.5 rounded-full text-sm font-medium whitespace-nowrap flex-shrink-0"
                        :class="getRoleBadgeClass(displayUser?.role || 'user')"
                    >
                        {{ getRoleDisplayName(displayUser?.role || "user") }}
                    </div>
                </div>

                <!-- Pronouns field - full width below name/badge -->
                <div v-if="showPronouns" class="pt-2 pb-6" :class="showBanner ? 'sm:pl-[9.5rem]' : ''">
                    <div class="flex flex-col gap-1.5">
                        <h3
                            class="text-xs font-medium text-tertiary uppercase tracking-wide"
                        >
                            Pronouns
                        </h3>
                        <div class="flex flex-wrap gap-3">
                            <input
                                v-model="formData.pronouns"
                                type="text"
                                class="flex-1 min-w-48 px-4 py-2.5 bg-surface-alt rounded-lg border border-subtle text-primary focus:ring-2 focus:ring-accent focus:outline-none"
                                placeholder="Add pronouns (e.g., he/him, she/her, they/them)"
                            />
                            <button
                                @click="updatePronouns"
                                :disabled="!pronounsModified || loading"
                                class="px-4 py-2.5 bg-accent text-white rounded-lg hover:opacity-90 focus:ring-2 focus:ring-accent disabled:opacity-50 disabled:cursor-not-allowed whitespace-nowrap"
                            >
                                Save
                            </button>
                        </div>
                    </div>
                </div>
            </template>

            <!-- READ-ONLY MODE -->
            <template v-else>
                <!-- Content area - uses same padding top as avatar offset to align vertically -->
                <div
                    class="flex flex-wrap items-start gap-3 pb-4"
                    :class="showBanner ? 'pt-16 sm:pt-6 sm:pl-[9.5rem]' : 'pt-4'"
                >
                    <!-- Left: Name, email, pronouns -->
                    <div class="flex flex-col gap-1 min-w-0 flex-1 basis-48">
                        <!-- Name with pronouns inline -->
                        <div class="flex flex-wrap items-baseline gap-x-2 gap-y-1">
                            <h2 class="text-2xl font-semibold text-primary">
                                {{ displayUser?.name || "Unknown User" }}
                            </h2>
                            <span
                                v-if="showPronouns && displayUser?.pronouns"
                                class="text-sm text-tertiary"
                            >
                                {{ displayUser.pronouns }}
                            </span>
                        </div>
                        <!-- Email below name -->
                        <p v-if="showEmail && displayUser?.email" class="text-secondary truncate">
                            {{ displayUser.email }}
                        </p>
                    </div>

                    <!-- Right: Role badge -->
                    <div
                        class="px-3 py-1.5 rounded-full text-sm font-medium whitespace-nowrap flex-shrink-0"
                        :class="getRoleBadgeClass(displayUser?.role || 'user')"
                    >
                        {{ getRoleDisplayName(displayUser?.role || "user") }}
                    </div>
                </div>

                <!-- Custom content slot for additional information -->
                <div v-if="$slots.default" class="mt-6 space-y-4">
                    <slot></slot>
                </div>
            </template>
        </div>
    </div>
</template>

<style scoped>
/* Style the InlineEdit component within the name field to have input field appearance */
.name-input-field :deep(.relative > div),
.name-input-field :deep(.relative > input) {
    background-color: var(--color-surface-alt);
    border: 1px solid var(--color-border-subtle);
    padding: 0.5rem 1rem;
}

.name-input-field :deep(.relative > input:focus) {
    border-color: var(--color-accent);
}

.developer-badge {
    position: relative;
    overflow: hidden;
}

.developer-badge::before {
    content: "";
    position: absolute;
    inset: -100%;
    background: linear-gradient(
        110deg,
        transparent 20%,
        rgba(168, 85, 247, 0.3) 40%,
        rgba(217, 70, 239, 0.5) 50%,
        rgba(168, 85, 247, 0.3) 60%,
        transparent 80%
    );
    animation: enchant 6s linear infinite;
    pointer-events: none;
}

@keyframes enchant {
    0% {
        transform: translateX(-100%);
    }
    100% {
        transform: translateX(100%);
    }
}
</style>
