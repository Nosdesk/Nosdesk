<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import BackButton from '@/components/common/BackButton.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Modal from '@/components/Modal.vue';
import ColorHueSlider from '@/components/common/ColorHueSlider.vue';
import { categoryService } from '@/services/categoryService';
import { groupService } from '@/services/groupService';
import type { CategoryWithVisibility, CreateCategoryRequest, UpdateCategoryRequest } from '@/types/category';
import type { GroupWithMemberCount } from '@/types/group';

// State
const isLoading = ref(false);
const isSaving = ref(false);
const errorMessage = ref('');
const successMessage = ref('');
const categories = ref<CategoryWithVisibility[]>([]);
const availableGroups = ref<GroupWithMemberCount[]>([]);

// Modal states
const showCategoryModal = ref(false);
const showDeleteConfirm = ref(false);
const editingCategory = ref<CategoryWithVisibility | null>(null);
const categoryToDelete = ref<CategoryWithVisibility | null>(null);

// Form state
const categoryForm = ref<{
  name: string;
  description: string;
  color: string;
  icon: string;
  is_active: boolean;
  visible_to_group_ids: number[];
}>({
  name: '',
  description: '',
  color: '#6366f1',
  icon: 'folder',
  is_active: true,
  visible_to_group_ids: []
});

// Available icons
const iconOptions = [
  { name: 'folder', label: 'Folder', path: 'M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z' },
  { name: 'tag', label: 'Tag', path: 'M7 7h.01M7 3h5c.512 0 1.024.195 1.414.586l7 7a2 2 0 010 2.828l-7 7a2 2 0 01-2.828 0l-7-7A1.994 1.994 0 013 12V7a4 4 0 014-4z' },
  { name: 'bug', label: 'Bug', path: 'M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z' },
  { name: 'cog', label: 'Settings', path: 'M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z M15 12a3 3 0 11-6 0 3 3 0 016 0z' },
  { name: 'lightbulb', label: 'Idea', path: 'M9.663 17h4.673M12 3v1m6.364 1.636l-.707.707M21 12h-1M4 12H3m3.343-5.657l-.707-.707m2.828 9.9a5 5 0 117.072 0l-.548.547A3.374 3.374 0 0014 18.469V19a2 2 0 11-4 0v-.531c0-.895-.356-1.754-.988-2.386l-.548-.547z' },
  { name: 'question', label: 'Question', path: 'M8.228 9c.549-1.165 2.03-2 3.772-2 2.21 0 4 1.343 4 3 0 1.4-1.278 2.575-3.006 2.907-.542.104-.994.54-.994 1.093m0 3h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z' },
  { name: 'exclamation', label: 'Alert', path: 'M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z' },
  { name: 'star', label: 'Star', path: 'M11.049 2.927c.3-.921 1.603-.921 1.902 0l1.519 4.674a1 1 0 00.95.69h4.915c.969 0 1.371 1.24.588 1.81l-3.976 2.888a1 1 0 00-.363 1.118l1.518 4.674c.3.922-.755 1.688-1.538 1.118l-3.976-2.888a1 1 0 00-1.176 0l-3.976 2.888c-.783.57-1.838-.197-1.538-1.118l1.518-4.674a1 1 0 00-.363-1.118l-3.976-2.888c-.784-.57-.38-1.81.588-1.81h4.914a1 1 0 00.951-.69l1.519-4.674z' }
];

// Get icon path by name
const getIconPath = (iconName: string) => {
  const icon = iconOptions.find(i => i.name === iconName);
  return icon?.path || iconOptions[0].path;
};

// Load categories
const loadCategories = async () => {
  isLoading.value = true;
  errorMessage.value = '';

  try {
    categories.value = await categoryService.getAllCategoriesAdmin();
  } catch (error) {
    console.error('Failed to load categories:', error);
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to load categories';
  } finally {
    isLoading.value = false;
  }
};

// Load groups for visibility selection
const loadGroups = async () => {
  try {
    availableGroups.value = await groupService.getGroups();
  } catch (error) {
    console.error('Failed to load groups:', error);
  }
};

// Open create modal
const openCreateModal = () => {
  editingCategory.value = null;
  categoryForm.value = {
    name: '',
    description: '',
    color: '#6366f1',
    icon: 'folder',
    is_active: true,
    visible_to_group_ids: []
  };
  showCategoryModal.value = true;
};

// Open edit modal
const openEditModal = async (category: CategoryWithVisibility) => {
  editingCategory.value = category;
  categoryForm.value = {
    name: category.name,
    description: category.description || '',
    color: category.color || '#6366f1',
    icon: category.icon || 'folder',
    is_active: category.is_active,
    visible_to_group_ids: category.visible_to_groups.map(g => g.id)
  };
  showCategoryModal.value = true;
};

// Toggle group visibility
const toggleGroupVisibility = (groupId: number) => {
  const index = categoryForm.value.visible_to_group_ids.indexOf(groupId);
  if (index === -1) {
    categoryForm.value.visible_to_group_ids.push(groupId);
  } else {
    categoryForm.value.visible_to_group_ids.splice(index, 1);
  }
};

// Save category
const saveCategory = async () => {
  if (!categoryForm.value.name.trim()) {
    errorMessage.value = 'Category name is required';
    return;
  }

  isSaving.value = true;
  errorMessage.value = '';

  try {
    if (editingCategory.value) {
      const updateData: UpdateCategoryRequest = {
        name: categoryForm.value.name,
        description: categoryForm.value.description || undefined,
        color: categoryForm.value.color,
        icon: categoryForm.value.icon,
        is_active: categoryForm.value.is_active,
        visible_to_group_ids: categoryForm.value.visible_to_group_ids.length > 0
          ? categoryForm.value.visible_to_group_ids
          : undefined
      };
      await categoryService.updateCategory(editingCategory.value.id, updateData);
      successMessage.value = 'Category updated successfully';
    } else {
      const createData: CreateCategoryRequest = {
        name: categoryForm.value.name,
        description: categoryForm.value.description || undefined,
        color: categoryForm.value.color,
        icon: categoryForm.value.icon,
        visible_to_group_ids: categoryForm.value.visible_to_group_ids.length > 0
          ? categoryForm.value.visible_to_group_ids
          : undefined
      };
      await categoryService.createCategory(createData);
      successMessage.value = 'Category created successfully';
    }

    showCategoryModal.value = false;
    await loadCategories();

    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to save category';
  } finally {
    isSaving.value = false;
  }
};

// Toggle category active status
const toggleActive = async (category: CategoryWithVisibility) => {
  try {
    await categoryService.updateCategory(category.id, {
      is_active: !category.is_active
    });
    await loadCategories();
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to update category';
  }
};

// Confirm delete
const confirmDelete = (category: CategoryWithVisibility) => {
  categoryToDelete.value = category;
  showDeleteConfirm.value = true;
};

// Delete category
const deleteCategory = async () => {
  if (!categoryToDelete.value) return;

  isSaving.value = true;
  errorMessage.value = '';

  try {
    await categoryService.deleteCategory(categoryToDelete.value.id);
    successMessage.value = 'Category deleted successfully';
    showDeleteConfirm.value = false;
    categoryToDelete.value = null;
    await loadCategories();

    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to delete category';
  } finally {
    isSaving.value = false;
  }
};

// Move category up/down
const moveCategory = async (category: CategoryWithVisibility, direction: 'up' | 'down') => {
  const index = categories.value.findIndex(c => c.id === category.id);
  if (index === -1) return;

  const newIndex = direction === 'up' ? index - 1 : index + 1;
  if (newIndex < 0 || newIndex >= categories.value.length) return;

  // Build new order
  const reorderedCategories = [...categories.value];
  const [moved] = reorderedCategories.splice(index, 1);
  reorderedCategories.splice(newIndex, 0, moved);

  const orders = reorderedCategories.map((c, i) => ({
    id: c.id,
    display_order: i
  }));

  try {
    await categoryService.reorderCategories({ orders });
    await loadCategories();
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to reorder categories';
  }
};

onMounted(() => {
  loadCategories();
  loadGroups();
});
</script>

<template>
  <div class="flex-1">
    <!-- Navigation and actions bar -->
    <div class="pt-4 px-6 flex justify-between items-center">
      <BackButton fallbackRoute="/admin/settings" label="Back to Administration" />
      <button
        @click="openCreateModal"
        class="px-3 py-1.5 bg-accent text-white rounded-lg text-sm hover:opacity-90 font-medium transition-colors flex items-center gap-1.5"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6m0 0v6m0-6h6m-6 0H6" />
        </svg>
        New Category
      </button>
    </div>

    <div class="flex flex-col gap-4 px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-2">
        <h1 class="text-2xl font-bold text-primary">Categories</h1>
        <p class="text-secondary mt-1">Manage ticket categories and group visibility</p>
      </div>

      <!-- Info notice -->
      <div class="bg-accent/10 border border-accent/30 rounded-lg p-3 text-sm text-accent flex items-start gap-2">
        <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <span>Categories with no group restrictions are visible to all users. Assign groups to restrict visibility.</span>
      </div>

      <!-- Success message -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

      <!-- Error message -->
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Loading state -->
      <LoadingSpinner v-if="isLoading" text="Loading categories..." />

      <!-- Categories list -->
      <div v-else class="flex flex-col gap-3">
        <div
          v-for="(category, index) in categories"
          :key="category.id"
          class="bg-surface border border-default rounded-xl transition-colors"
          :class="{ 'opacity-60': !category.is_active }"
        >
          <div class="p-4 flex items-center gap-4">
            <!-- Reorder buttons -->
            <div class="flex flex-col gap-0.5 flex-shrink-0">
              <button
                @click="moveCategory(category, 'up')"
                :disabled="index === 0"
                class="p-1 text-tertiary hover:text-primary disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                title="Move up"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M5 15l7-7 7 7" />
                </svg>
              </button>
              <button
                @click="moveCategory(category, 'down')"
                :disabled="index === categories.length - 1"
                class="p-1 text-tertiary hover:text-primary disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                title="Move down"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M19 9l-7 7-7-7" />
                </svg>
              </button>
            </div>

            <!-- Icon -->
            <div
              class="w-10 h-10 rounded-lg flex items-center justify-center flex-shrink-0"
              :style="{ backgroundColor: (category.color || '#6366f1') + '20' }"
            >
              <svg
                xmlns="http://www.w3.org/2000/svg"
                class="h-5 w-5"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
                stroke-width="2"
                :style="{ color: category.color || '#6366f1' }"
              >
                <path stroke-linecap="round" stroke-linejoin="round" :d="getIconPath(category.icon || 'folder')" />
              </svg>
            </div>

            <!-- Category info -->
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2 flex-wrap">
                <h3 class="font-medium text-primary">{{ category.name }}</h3>
                <span
                  v-if="category.is_public"
                  class="px-2 py-0.5 text-xs bg-status-success/20 text-status-success rounded-full"
                >
                  Public
                </span>
                <span
                  v-else
                  class="px-2 py-0.5 text-xs bg-status-warning/20 text-status-warning rounded-full"
                >
                  {{ category.visible_to_groups.length }} group{{ category.visible_to_groups.length !== 1 ? 's' : '' }}
                </span>
                <span
                  v-if="!category.is_active"
                  class="px-2 py-0.5 text-xs bg-surface-alt text-tertiary rounded-full"
                >
                  Inactive
                </span>
              </div>
              <p v-if="category.description" class="text-sm text-secondary mt-0.5 truncate">
                {{ category.description }}
              </p>
              <!-- Show visible groups -->
              <div v-if="!category.is_public && category.visible_to_groups.length > 0" class="flex items-center gap-1 mt-1 flex-wrap">
                <span
                  v-for="group in category.visible_to_groups.slice(0, 3)"
                  :key="group.id"
                  class="px-1.5 py-0.5 text-xs bg-surface-alt text-secondary rounded"
                >
                  {{ group.name }}
                </span>
                <span
                  v-if="category.visible_to_groups.length > 3"
                  class="text-xs text-tertiary"
                >
                  +{{ category.visible_to_groups.length - 3 }} more
                </span>
              </div>
            </div>

            <!-- Actions -->
            <div class="flex items-center gap-2 flex-shrink-0">
              <!-- Toggle active -->
              <button
                @click="toggleActive(category)"
                class="p-2 rounded-lg transition-colors"
                :class="category.is_active ? 'text-status-success hover:bg-status-success/10' : 'text-tertiary hover:bg-surface-hover'"
                :title="category.is_active ? 'Deactivate' : 'Activate'"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path v-if="category.is_active" stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                  <path v-if="category.is_active" stroke-linecap="round" stroke-linejoin="round" d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" />
                  <path v-else stroke-linecap="round" stroke-linejoin="round" d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.88 9.88l-3.29-3.29m7.532 7.532l3.29 3.29M3 3l3.59 3.59m0 0A9.953 9.953 0 0112 5c4.478 0 8.268 2.943 9.543 7a10.025 10.025 0 01-4.132 5.411m0 0L21 21" />
                </svg>
              </button>
              <button
                @click="openEditModal(category)"
                class="p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
                title="Edit category"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z" />
                </svg>
              </button>
              <button
                @click="confirmDelete(category)"
                class="p-2 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-lg transition-colors"
                title="Delete category"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                </svg>
              </button>
            </div>
          </div>
        </div>

        <!-- Empty state -->
        <EmptyState
          v-if="categories.length === 0 && !isLoading"
          icon="folder"
          title="No categories yet"
          description="Create categories to organize tickets"
          action-label="Create Category"
          variant="card"
          @action="openCreateModal"
        />
      </div>
    </div>

    <!-- Create/Edit Category Modal -->
    <Modal
      :show="showCategoryModal"
      :title="editingCategory ? 'Edit Category' : 'Create Category'"
      size="md"
      @close="showCategoryModal = false"
    >
      <form @submit.prevent="saveCategory" class="flex flex-col gap-4">
        <!-- Name -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">Name</label>
          <input
            v-model="categoryForm.name"
            type="text"
            placeholder="Enter category name"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            required
          />
        </div>

        <!-- Description -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">Description</label>
          <textarea
            v-model="categoryForm.description"
            placeholder="Optional description"
            rows="2"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent resize-none"
          />
        </div>

        <!-- Icon -->
        <div>
          <label class="block text-sm font-medium text-primary mb-2">Icon</label>
          <div class="flex flex-wrap gap-2">
            <button
              v-for="icon in iconOptions"
              :key="icon.name"
              type="button"
              @click="categoryForm.icon = icon.name"
              class="w-10 h-10 rounded-lg border-2 flex items-center justify-center transition-all"
              :class="categoryForm.icon === icon.name ? 'border-accent bg-accent/10' : 'border-default hover:border-strong'"
              :title="icon.label"
            >
              <svg
                xmlns="http://www.w3.org/2000/svg"
                class="h-5 w-5"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
                stroke-width="2"
                :class="categoryForm.icon === icon.name ? 'text-accent' : 'text-secondary'"
              >
                <path stroke-linecap="round" stroke-linejoin="round" :d="icon.path" />
              </svg>
            </button>
          </div>
        </div>

        <!-- Color -->
        <ColorHueSlider v-model="categoryForm.color" label="Color" />

        <!-- Active status (only for editing) -->
        <div v-if="editingCategory" class="flex items-center gap-3">
          <label class="relative inline-flex items-center cursor-pointer">
            <input
              type="checkbox"
              v-model="categoryForm.is_active"
              class="sr-only peer"
            />
            <div class="w-11 h-6 bg-surface-alt peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-accent rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-accent"></div>
          </label>
          <span class="text-sm text-primary">Active</span>
        </div>

        <!-- Group visibility -->
        <div>
          <label class="block text-sm font-medium text-primary mb-2">
            Visible to Groups
            <span class="text-tertiary font-normal ml-1">(leave empty for public)</span>
          </label>
          <div v-if="availableGroups.length > 0" class="max-h-40 overflow-y-auto border border-default rounded-lg divide-y divide-default">
            <label
              v-for="group in availableGroups"
              :key="group.id"
              class="flex items-center gap-3 p-2.5 hover:bg-surface-hover cursor-pointer transition-colors"
            >
              <input
                type="checkbox"
                :checked="categoryForm.visible_to_group_ids.includes(group.id)"
                @change="toggleGroupVisibility(group.id)"
                class="w-4 h-4 text-accent bg-surface-alt border-default rounded focus:ring-accent focus:ring-offset-0"
              />
              <div
                class="w-3 h-3 rounded-full flex-shrink-0"
                :style="{ backgroundColor: group.color || '#6366f1' }"
              />
              <span class="text-sm text-primary">{{ group.name }}</span>
              <span class="text-xs text-tertiary ml-auto">{{ group.member_count }} members</span>
            </label>
          </div>
          <p v-else class="text-sm text-tertiary py-2">
            No groups available. <router-link to="/admin/groups" class="text-accent hover:underline">Create groups</router-link> first.
          </p>
        </div>

        <!-- Actions -->
        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="showCategoryModal = false"
            class="px-4 py-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
          >
            Cancel
          </button>
          <button
            type="submit"
            :disabled="isSaving"
            class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 flex items-center gap-2"
          >
            <svg v-if="isSaving" class="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
            {{ editingCategory ? 'Save Changes' : 'Create Category' }}
          </button>
        </div>
      </form>
    </Modal>

    <!-- Delete Confirmation Modal -->
    <Modal
      :show="showDeleteConfirm"
      title="Delete Category"
      size="sm"
      @close="showDeleteConfirm = false"
    >
      <div class="flex flex-col gap-4">
        <p class="text-secondary">
          Are you sure you want to delete the category <strong class="text-primary">{{ categoryToDelete?.name }}</strong>?
          Tickets using this category will have their category cleared.
        </p>

        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="showDeleteConfirm = false"
            class="px-4 py-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
          >
            Cancel
          </button>
          <button
            @click="deleteCategory"
            :disabled="isSaving"
            class="px-4 py-2 bg-status-error text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 flex items-center gap-2"
          >
            <svg v-if="isSaving" class="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
            Delete Category
          </button>
        </div>
      </div>
    </Modal>
  </div>
</template>
