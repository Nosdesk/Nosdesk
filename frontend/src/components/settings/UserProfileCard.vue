<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { useAuthStore } from "@/stores/auth";
import UserAvatar from "@/components/UserAvatar.vue";
import InlineEdit from "@/components/common/InlineEdit.vue";
import userService from "@/services/userService";
import uploadService from "@/services/uploadService";

interface UserAvatarComponentType {
    refreshUser: (uuid?: string) => Promise<void>;
}

const authStore = useAuthStore();
const loading = ref(false);
const userAvatarComponent = ref<UserAvatarComponentType | null>(null);

// File inputs
const fileInput = ref<HTMLInputElement | null>(null);
const bannerFileInput = ref<HTMLInputElement | null>(null);
const avatarFile = ref<File | null>(null);
const bannerFile = ref<File | null>(null);
const avatarPreview = ref<string | null>(null);
const bannerPreview = ref<string | null>(null);

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
        user?: any; // User data to display (if different from auth user)
        canEdit?: boolean; // Whether editing is allowed
        showEditableFields?: boolean; // Whether to show editable fields
    }>(),
    {
        canEdit: false,
        showEditableFields: false,
    },
);

// Use provided user or fallback to auth user
const displayUser = computed(() => props.user || authStore.user);

// Computed property for determining if component should be in edit mode
const isEditable = computed(() => props.canEdit && props.showEditableFields);

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
        avatarFile.value = file;
        avatarPreview.value = uploadService.createPreviewUrl(file);
        await uploadAvatar();
    } catch (error: any) {
        emit("error", error.message || "Failed to process image");
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
        bannerFile.value = file;
        bannerPreview.value = uploadService.createPreviewUrl(file);
        await uploadBanner();
    } catch (error: any) {
        emit("error", error.message || "Failed to process image");
    }
};

const uploadImage = async (type: "avatar" | "banner") => {
    const file = type === "avatar" ? avatarFile.value : bannerFile.value;
    if (!file) return;

    loading.value = true;

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
        const cacheBustedUrl = `${uploadedUrl}?t=${Date.now()}`;

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

            setTimeout(() => authStore.fetchUserData(), 500);
        }
    } catch (err) {
        emit("error", `Failed to update ${type}`);
        console.error(`Error updating ${type}:`, err);
    } finally {
        loading.value = false;
    }
};

const uploadAvatar = () => uploadImage("avatar");
const uploadBanner = () => uploadImage("banner");

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
            return "bg-red-600/20 text-red-400";
        case "technician":
            return "bg-blue-600/20 text-blue-400";
        case "user":
        default:
            return "bg-slate-600/20 text-slate-400";
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
        class="bg-slate-800 rounded-xl border border-slate-700/50 hover:border-slate-600/50 transition-colors overflow-hidden"
    >
        <!-- Cover/Banner Image -->
        <div
            class="h-32 sm:h-42 bg-gradient-to-r from-blue-600 to-purple-600 relative"
            :style="
                formData.banner_url
                    ? `background-image: url('${formData.banner_url}'); background-size: cover; background-position: center;`
                    : ''
            "
        >
            <!-- Banner upload button (only when editable) -->
            <button
                v-if="isEditable"
                class="absolute bottom-2 right-2 bg-slate-800/50 hover:bg-slate-800/80 text-white rounded-full w-11 h-11 flex items-center justify-center transition-colors"
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
        <div class="px-4 sm:px-6 pb-4 relative">
            <!-- Avatar (overlaps banner) -->
            <div
                class="absolute -top-14 sm:-top-16 left-4 sm:left-6 w-28 h-28 sm:w-32 sm:h-32 rounded-full overflow-hidden border-4 border-slate-800 shadow-lg"
                :class="{ 'cursor-pointer': isEditable }"
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
                <!-- Hover overlay for editing -->
                <div
                    v-if="isEditable"
                    class="absolute inset-0 bg-black/50 flex items-center justify-center opacity-0 hover:opacity-100 transition-opacity"
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
                <div class="pt-16 sm:pt-20">
                    <div class="mb-6">
                        <!-- Name with inline edit -->
                        <div class="mb-3">
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

                        <!-- Role badge -->
                        <div class="flex flex-wrap gap-2">
                            <div
                                class="px-3 py-1.5 rounded-full text-sm font-medium"
                                :class="
                                    getRoleBadgeClass(
                                        displayUser?.role || 'user',
                                    )
                                "
                            >
                                {{
                                    getRoleDisplayName(
                                        displayUser?.role || "user",
                                    )
                                }}
                            </div>
                        </div>
                    </div>

                    <!-- Editable fields -->
                    <div class="grid grid-cols-1 gap-6">
                        <!-- Pronouns -->
                        <div class="flex flex-col gap-1.5">
                            <h3
                                class="text-xs font-medium text-slate-400 uppercase tracking-wide"
                            >
                                Pronouns
                            </h3>
                            <div class="flex flex-col sm:flex-row gap-3">
                                <input
                                    v-model="formData.pronouns"
                                    type="text"
                                    class="flex-1 px-4 py-2.5 bg-slate-700/50 rounded-lg border border-slate-600/30 text-white focus:ring-2 focus:ring-blue-500 focus:outline-none"
                                    placeholder="Add pronouns (e.g., he/him, she/her, they/them)"
                                />
                                <button
                                    @click="updatePronouns"
                                    :disabled="!pronounsModified || loading"
                                    class="px-4 py-2.5 bg-blue-600 text-white rounded-lg hover:bg-blue-700 focus:ring-2 focus:ring-blue-500 disabled:opacity-50 disabled:cursor-not-allowed"
                                >
                                    Save
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            </template>

            <!-- READ-ONLY MODE -->
            <template v-else>
                <!-- Mobile: Stacked layout -->
                <div class="flex flex-col gap-3 pt-16 sm:hidden">
                    <!-- Name on top -->
                    <h2 class="text-2xl font-semibold text-white">
                        {{ displayUser?.name || "Unknown User" }}
                    </h2>
                    <!-- Badge and pronouns below -->
                    <div class="flex items-center gap-3">
                        <div
                            class="px-3 py-1.5 rounded-full text-sm font-medium"
                            :class="
                                getRoleBadgeClass(displayUser?.role || 'user')
                            "
                        >
                            {{
                                getRoleDisplayName(displayUser?.role || "user")
                            }}
                        </div>
                        <span
                            v-if="displayUser?.pronouns"
                            class="text-sm text-slate-400"
                        >
                            {{ displayUser.pronouns }}
                        </span>
                    </div>
                </div>

                <!-- Desktop: Single row with baseline alignment -->
                <div
                    class="hidden sm:flex pl-34 pt-4 items-baseline justify-between gap-4"
                >
                    <!-- Left: Name and pronouns -->
                    <div class="flex items-baseline gap-3 flex-1 min-w-0">
                        <h2 class="text-2xl font-semibold text-white">
                            {{ displayUser?.name || "Unknown User" }}
                        </h2>
                        <span
                            v-if="displayUser?.pronouns"
                            class="text-sm text-slate-400"
                        >
                            {{ displayUser.pronouns }}
                        </span>
                    </div>

                    <!-- Right: Badge -->
                    <div
                        class="px-3 py-1.5 rounded-full text-sm font-medium flex-shrink-0"
                        :class="getRoleBadgeClass(displayUser?.role || 'user')"
                    >
                        {{ getRoleDisplayName(displayUser?.role || "user") }}
                    </div>
                </div>
            </template>
        </div>
    </div>
</template>

<style scoped>
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
