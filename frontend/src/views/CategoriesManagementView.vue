<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';

import AlertMessage from '@/components/common/AlertMessage.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Icon from '@/components/common/Icon.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import DebouncedSearchInput from '@/components/common/DebouncedSearchInput.vue';
import ColorHueSlider from '@/components/common/ColorHueSlider.vue';
import Modal from '@/components/Modal.vue';
import CategoryEditPanel from '@/components/admin/CategoryEditPanel.vue';
import SplitPanelLayout from '@/components/admin/SplitPanelLayout.vue';
import { categoryService } from '@/services/categoryService';
import { groupService } from '@/services/groupService';
import { useListReorder } from '@/composables/useListReorder';
import { useMobileDetection } from '@/composables/useMobileDetection';
import type { CategoryWithVisibility, CreateCategoryRequest, UpdateCategoryRequest } from '@/types/category';
import type { GroupWithMemberCount } from '@/types/group';
import { extractErrorMessage } from '@/utils/errors';

const { isMobile } = useMobileDetection('xl');

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

// The category list is cached by Pinia Colada keyed here, so navigating
// away and back renders it instantly from cache and revalidates in the
// background. A skeleton shows only on the genuine first load (empty
// cache); see `isFirstLoad`.
//
// `categories` stays a writable local ref (not a computed) because the
// drag-reorder composable and the optimistic active-toggle mutate it in
// place. A watch keeps it in sync with the query's cached data; mutations
// invalidate the query, which refetches and re-syncs through the watch.
const CATEGORIES_KEY = ['categories'] as const;
const queryCache = useQueryCache();
const categoriesQuery = useQuery({
  key: CATEGORIES_KEY,
  query: () => categoryService.getAllCategoriesAdmin(),
});
const categories = ref<CategoryWithVisibility[]>([]);
watch(
  categoriesQuery.data,
  (data) => {
    categories.value = Array.isArray(data) ? data : [];
  },
  { immediate: true },
);
const isFirstLoad = computed(
  () => categoriesQuery.status.value === 'pending' && categoriesQuery.data.value === undefined,
);
const loadError = computed(() =>
  categoriesQuery.error.value ? t('admin-categories-error-load') : '',
);

// Mutation feedback stays in local refs.
const isSaving = ref(false);
const errorMessage = ref('');
const successMessage = ref('');
const availableGroups = ref<GroupWithMemberCount[]>([]);

// Drag-and-drop reordering
const { dragState, listRef, handleGripDown } = useListReorder(categories, {
  getId: (c) => c.id,
  onReorder: (reordered, previous) => {
    const orders = reordered.map((c, i) => ({ id: c.id, display_order: i }));
    categoryService.reorderCategories({ orders }).catch((error) => {
      categories.value = previous;
      errorMessage.value = extractErrorMessage(error, t('admin-categories-error-reorder'));
    });
  },
});

const draggedCategory = computed(() => {
  if (!dragState.value.isDragging || dragState.value.draggedId === null) return null;
  return categories.value.find(c => c.id === dragState.value.draggedId) ?? null;
});

// Search, filter & sort
const searchQuery = ref('');
const sortField = ref('custom');
const sortAsc = ref(true);
const filterBy = ref('all');

const sortFieldOptions = computed(() => [
  { value: 'custom', label: t('admin-categories-sort-custom') },
  { value: 'name', label: t('admin-categories-sort-name') },
]);

const filterOptions = computed(() => [
  { value: 'all', label: t('admin-categories-filter-all') },
  { value: 'active', label: t('admin-categories-filter-active') },
  { value: 'inactive', label: t('admin-categories-filter-inactive') },
  { value: 'public', label: t('admin-categories-filter-public') },
  { value: 'restricted', label: t('admin-categories-filter-restricted') },
]);

const onSortFieldChange = (value: string | string[]) => {
  const field = Array.isArray(value) ? value[0] : value;
  sortField.value = field;
  sortAsc.value = true; // Name defaults to A-Z
};

const toggleSortDirection = () => {
  sortAsc.value = !sortAsc.value;
};

const canDrag = computed(() => sortField.value === 'custom' && filterBy.value === 'all' && !searchQuery.value.trim());

const filteredCategories = computed(() => {
  let result = [...categories.value];

  // Search
  if (searchQuery.value.trim()) {
    const q = searchQuery.value.toLowerCase();
    result = result.filter(c =>
      c.name.toLowerCase().includes(q) ||
      (c.description && c.description.toLowerCase().includes(q))
    );
  }

  // Filter
  switch (filterBy.value) {
    case 'active': result = result.filter(c => c.is_active); break;
    case 'inactive': result = result.filter(c => !c.is_active); break;
    case 'public': result = result.filter(c => c.is_public); break;
    case 'restricted': result = result.filter(c => !c.is_public); break;
  }

  // Sort (custom = display_order, already the default from API)
  if (sortField.value !== 'custom') {
    const dir = sortAsc.value ? 1 : -1;
    result.sort((a, b) => a.name.localeCompare(b.name) * dir);
  }

  return result;
});

// Modal states (for mobile)
const showCategoryModal = ref(false);
const showDeleteConfirm = ref(false);
const editingCategory = ref<CategoryWithVisibility | null>(null);
const categoryToDelete = ref<CategoryWithVisibility | null>(null);

// Panel states (for desktop)
const panelCategory = ref<CategoryWithVisibility | null | undefined>(undefined); // undefined = closed, null = create, object = edit
const isPanelOpen = computed(() => panelCategory.value !== undefined);

// Get icon path by name
const iconOptions = [
  { name: 'folder', path: 'M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z' },
  { name: 'tag', path: 'M7 7h.01M7 3h5c.512 0 1.024.195 1.414.586l7 7a2 2 0 010 2.828l-7 7a2 2 0 01-2.828 0l-7-7A1.994 1.994 0 013 12V7a4 4 0 014-4z' },
  { name: 'bug', path: 'M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z' },
  { name: 'cog', path: 'M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z M15 12a3 3 0 11-6 0 3 3 0 016 0z' },
  { name: 'lightbulb', path: 'M9.663 17h4.673M12 3v1m6.364 1.636l-.707.707M21 12h-1M4 12H3m3.343-5.657l-.707-.707m2.828 9.9a5 5 0 117.072 0l-.548.547A3.374 3.374 0 0014 18.469V19a2 2 0 11-4 0v-.531c0-.895-.356-1.754-.988-2.386l-.548-.547z' },
  { name: 'question', path: 'M8.228 9c.549-1.165 2.03-2 3.772-2 2.21 0 4 1.343 4 3 0 1.4-1.278 2.575-3.006 2.907-.542.104-.994.54-.994 1.093m0 3h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z' },
  { name: 'exclamation', path: 'M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z' },
  { name: 'star', path: 'M11.049 2.927c.3-.921 1.603-.921 1.902 0l1.519 4.674a1 1 0 00.95.69h4.915c.969 0 1.371 1.24.588 1.81l-3.976 2.888a1 1 0 00-.363 1.118l1.518 4.674c.3.922-.755 1.688-1.538 1.118l-3.976-2.888a1 1 0 00-1.176 0l-3.976 2.888c-.783.57-1.838-.197-1.538-1.118l1.518-4.674a1 1 0 00-.363-1.118l-3.976-2.888c-.784-.57-.38-1.81.588-1.81h4.914a1 1 0 00.951-.69l1.519-4.674z' }
];

const getIconPath = (iconName: string) => {
  const icon = iconOptions.find(i => i.name === iconName);
  return icon?.path || iconOptions[0].path;
};

// Default swatch colour for categories/groups without one set. These are
// arbitrary user-data colours (rendered as hex + alpha), so a named
// default rather than a theme token.
const DEFAULT_COLOR = '#6366f1';

// Form state for mobile modal
const categoryForm = ref({
  name: '',
  description: '',
  color: DEFAULT_COLOR,
  icon: 'folder',
  is_active: true,
  visible_to_group_ids: [] as number[]
});

// Load groups for visibility selection
const loadGroups = async () => {
  try {
    availableGroups.value = await groupService.getGroups();
  } catch (error) {
    console.error('Failed to load groups:', error);
  }
};

// Open create modal/panel
const openCreateModal = () => {
  if (isMobile.value) {
    editingCategory.value = null;
    categoryForm.value = {
      name: '',
      description: '',
      color: DEFAULT_COLOR,
      icon: 'folder',
      is_active: true,
      visible_to_group_ids: []
    };
    showCategoryModal.value = true;
  } else {
    panelCategory.value = null;
  }
};

// Open edit modal/panel
const openEditModal = (category: CategoryWithVisibility) => {
  if (isMobile.value) {
    editingCategory.value = category;
    categoryForm.value = {
      name: category.name,
      description: category.description || '',
      color: category.color || DEFAULT_COLOR,
      icon: category.icon || 'folder',
      is_active: category.is_active,
      visible_to_group_ids: category.visible_to_groups.map(g => g.id)
    };
    showCategoryModal.value = true;
  } else {
    panelCategory.value = category;
  }
};

// Toggle group visibility (for mobile modal)
const toggleGroupVisibility = (groupId: number) => {
  const index = categoryForm.value.visible_to_group_ids.indexOf(groupId);
  if (index === -1) {
    categoryForm.value.visible_to_group_ids.push(groupId);
  } else {
    categoryForm.value.visible_to_group_ids.splice(index, 1);
  }
};

// Save category (shared between modal and panel)
const saveCategoryFromForm = async (formData: {
  name: string;
  description: string;
  color: string;
  icon: string;
  is_active: boolean;
  visible_to_group_ids: number[];
}) => {
  if (!formData.name.trim()) {
    errorMessage.value = t('admin-categories-error-name-required');
    return;
  }

  isSaving.value = true;
  errorMessage.value = '';

  // Determine if editing (check both panel and modal sources)
  const editing = isMobile.value ? editingCategory.value : (panelCategory.value || null);

  try {
    if (editing) {
      const updateData: UpdateCategoryRequest = {
        name: formData.name,
        description: formData.description || undefined,
        color: formData.color,
        icon: formData.icon,
        is_active: formData.is_active,
        visible_to_group_ids: formData.visible_to_group_ids.length > 0
          ? formData.visible_to_group_ids
          : undefined
      };
      await categoryService.updateCategory(editing.id, updateData);
      successMessage.value = t('admin-categories-success-update');
    } else {
      const createData: CreateCategoryRequest = {
        name: formData.name,
        description: formData.description || undefined,
        color: formData.color,
        icon: formData.icon,
        visible_to_group_ids: formData.visible_to_group_ids.length > 0
          ? formData.visible_to_group_ids
          : undefined
      };
      await categoryService.createCategory(createData);
      successMessage.value = t('admin-categories-success-create');
    }

    showCategoryModal.value = false;
    await queryCache.invalidateQueries({ key: CATEGORIES_KEY });

    // Update the panel's reference to the fresh category data if editing on desktop
    if (!isMobile.value && editing) {
      const updated = categories.value.find(c => c.id === editing.id);
      if (updated) {
        panelCategory.value = updated;
      }
    } else if (!isMobile.value && !editing) {
      // After creating, close the panel
      panelCategory.value = undefined;
    }

    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    errorMessage.value = extractErrorMessage(error, t('admin-categories-error-save'));
  } finally {
    isSaving.value = false;
  }
};

// Save from mobile modal
const saveCategory = async () => {
  await saveCategoryFromForm(categoryForm.value);
};

// Save from desktop panel
const onPanelSave = async (formData: {
  name: string;
  description: string;
  color: string;
  icon: string;
  is_active: boolean;
  visible_to_group_ids: number[];
}) => {
  await saveCategoryFromForm(formData);
};

// Toggle category active status (optimistic update)
const toggleActive = async (category: CategoryWithVisibility) => {
  const previousState = category.is_active;
  category.is_active = !previousState;

  try {
    await categoryService.updateCategory(category.id, {
      is_active: category.is_active
    });
  } catch (error) {
    category.is_active = previousState;
    errorMessage.value = extractErrorMessage(error, t('admin-categories-error-update'));
  }
};

// Confirm delete
const confirmDelete = (category: CategoryWithVisibility) => {
  categoryToDelete.value = category;
  showDeleteConfirm.value = true;
};

// Handle delete from panel
const onPanelDelete = (category: CategoryWithVisibility) => {
  confirmDelete(category);
};

// Close panel
const onPanelClose = () => {
  panelCategory.value = undefined;
};

// Delete category
const deleteCategory = async () => {
  if (!categoryToDelete.value) return;

  isSaving.value = true;
  errorMessage.value = '';

  try {
    await categoryService.deleteCategory(categoryToDelete.value.id);
    successMessage.value = t('admin-categories-success-delete');
    showDeleteConfirm.value = false;

    // Close panel if the deleted category was selected
    if (panelCategory.value && 'id' in panelCategory.value && panelCategory.value.id === categoryToDelete.value.id) {
      panelCategory.value = undefined;
    }

    categoryToDelete.value = null;
    await queryCache.invalidateQueries({ key: CATEGORIES_KEY });

    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    errorMessage.value = extractErrorMessage(error, t('admin-categories-error-delete'));
  } finally {
    isSaving.value = false;
  }
};


onMounted(() => {
  // The category list auto-fetches via useQuery; only the group
  // visibility list needs an explicit load.
  loadGroups();
});
</script>

<template>
  <!-- Single root so App.vue's <Transition mode="out-in"> can attach
       leave/enter classes. Modals + the floating drag preview stay
       inside the wrapper; Modal and Teleport relocate themselves at
       render time. -->
  <div class="h-full">
  <SplitPanelLayout :panelOpen="isPanelOpen">
    <template #list>
      <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full" :class="isPanelOpen && !isMobile ? '' : 'max-w-8xl'">
        <div class="mb-2 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
          <div>
            <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('admin-categories-title') }}</h1>
            <p class="text-secondary mt-1">{{ $t('admin-categories-description') }}</p>
          </div>
          <Button size="sm" icon="add" class="self-start sm:self-auto" @click="openCreateModal">
            {{ $t('admin-categories-new') }}
          </Button>
        </div>

        <!-- Info notice -->
        <div class="bg-accent/10 border border-accent/30 rounded-lg p-3 text-sm text-accent flex items-start gap-2">
          <Icon name="info" size="md" class="flex-shrink-0" />
          <span>{{ $t('admin-categories-info') }}</span>
        </div>

        <!-- Success message -->
        <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

        <!-- Error message -->
        <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

        <!-- Load error (initial fetch failed with no cached data) -->
        <AlertMessage v-if="loadError && categories.length === 0" type="error" :message="loadError" />

        <!-- First-load skeleton: mirrors the category-row layout so the
             shell doesn't shift when data arrives. Only shown on a cold
             cache; remounts render cached rows instantly and revalidate
             silently in the background. -->
        <Skeleton
          v-if="isFirstLoad"
          :label="$t('admin-categories-loading')"
          class="flex flex-col gap-1"
        >
          <div
            v-for="n in 3"
            :key="n"
            class="bg-surface border border-default rounded-xl p-4 flex items-center gap-4"
          >
            <SkeletonBar class="w-10 h-10 rounded-lg shrink-0" />
            <div class="flex-1 flex flex-col gap-2">
              <SkeletonBar class="h-4 w-40 max-w-full" />
              <SkeletonBar class="h-3 w-3/4" />
            </div>
          </div>
        </Skeleton>

        <!-- Search, filter & sort toolbar -->
        <div v-if="!isFirstLoad && categories.length > 0" class="flex items-center gap-2">
          <DebouncedSearchInput
            v-model="searchQuery"
            :placeholder="$t('admin-categories-search-placeholder')"
            :debounce-ms="0"
            class="max-w-xs"
          />
          <BaseDropdown
            v-model="filterBy"
            :options="filterOptions"
            size="sm"
          />
          <BaseDropdown
            :model-value="sortField"
            :options="sortFieldOptions"
            size="sm"
            @update:model-value="onSortFieldChange"
          />
          <button
            v-if="sortField !== 'custom'"
            type="button"
            @click="toggleSortDirection"
            class="p-1.5 border border-default rounded-lg bg-surface-alt hover:border-strong hover:bg-surface-hover transition-colors text-secondary hover:text-primary focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            :title="sortAsc ? $t('admin-categories-sort-ascending') : $t('admin-categories-sort-descending')"
            :aria-label="sortAsc ? $t('admin-categories-sort-ascending') : $t('admin-categories-sort-descending')"
          >
            <Icon
              name="chevronUp"
              class="transition-transform duration-200"
              :class="{ 'rotate-180': !sortAsc }"
            />
          </button>
        </div>

        <!-- Categories list -->
        <div v-if="!isFirstLoad" ref="listRef" class="flex flex-col gap-1" :class="{ 'select-none cursor-grabbing': dragState.isDragging }">
          <template v-for="(category, index) in filteredCategories" :key="category.id">
            <!-- Drop indicator line (before item) -->
            <div
              v-if="canDrag && dragState.isDragging && dragState.insertIndex === index && dragState.draggedIndex !== index && dragState.draggedIndex !== index - 1"
              class="h-0.5 bg-accent rounded-full mx-4 transition-all"
            />

            <!-- Collapsed placeholder for the dragged item -->
            <div
              v-if="canDrag && dragState.isDragging && dragState.draggedId === category.id"
              :data-item-id="category.id"
              class="h-12 bg-surface-alt/50 border-2 border-dashed border-default rounded-xl"
            />

            <!-- Normal category card (hidden when being dragged) -->
            <div
              v-else
              :data-item-id="category.id"
              class="bg-surface border rounded-xl transition-colors cursor-pointer"
              :class="[
                { 'opacity-60': !category.is_active },
                panelCategory && 'id' in panelCategory && panelCategory.id === category.id && !isMobile
                  ? 'border-accent bg-accent/5'
                  : 'border-default hover:border-strong'
              ]"
              @click="openEditModal(category)"
            >
              <div class="p-4 flex items-center gap-4">
                <!-- Drag grip handle (only in custom order mode) -->
                <button
                  v-if="canDrag"
                  @pointerdown="handleGripDown(category.id, index, $event)"
                  class="flex-shrink-0 p-1 text-tertiary hover:text-secondary cursor-grab active:cursor-grabbing touch-none"
                  :title="$t('admin-categories-drag-handle')"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
                    <path d="M7 2a2 2 0 1 0 0 4 2 2 0 0 0 0-4zM13 2a2 2 0 1 0 0 4 2 2 0 0 0 0-4zM7 8a2 2 0 1 0 0 4 2 2 0 0 0 0-4zM13 8a2 2 0 1 0 0 4 2 2 0 0 0 0-4zM7 14a2 2 0 1 0 0 4 2 2 0 0 0 0-4zM13 14a2 2 0 1 0 0 4 2 2 0 0 0 0-4z" />
                  </svg>
                </button>

                <!-- Icon -->
                <div
                  class="w-10 h-10 rounded-lg flex items-center justify-center flex-shrink-0"
                  :style="{ backgroundColor: (category.color || DEFAULT_COLOR) + '20' }"
                >
                  <svg
                    xmlns="http://www.w3.org/2000/svg"
                    class="h-5 w-5"
                    fill="none"
                    viewBox="0 0 24 24"
                    stroke="currentColor"
                    stroke-width="2"
                    :style="{ color: category.color || DEFAULT_COLOR }"
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
                      {{ $t('admin-categories-badge-public') }}
                    </span>
                    <span
                      v-else
                      class="px-2 py-0.5 text-xs bg-status-warning/20 text-status-warning rounded-full"
                    >
                      {{ $t('admin-categories-badge-groups', { count: category.visible_to_groups.length }) }}
                    </span>
                    <span
                      v-if="!category.is_active"
                      class="px-2 py-0.5 text-xs bg-surface-alt text-tertiary rounded-full"
                    >
                      {{ $t('admin-categories-badge-inactive') }}
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
                      {{ $t('admin-categories-groups-more', { count: category.visible_to_groups.length - 3 }) }}
                    </span>
                  </div>
                </div>

                <!-- Actions -->
                <div class="flex items-center gap-2 flex-shrink-0">
                  <!-- Toggle active -->
                  <button
                    type="button"
                    @click.stop="toggleActive(category)"
                    class="p-2 rounded-lg transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                    :class="category.is_active ? 'text-status-success hover:bg-status-success/10' : 'text-tertiary hover:bg-surface-hover'"
                    :title="category.is_active ? $t('admin-categories-action-deactivate') : $t('admin-categories-action-activate')"
                    :aria-label="category.is_active ? $t('admin-categories-action-deactivate') : $t('admin-categories-action-activate')"
                  >
                    <Icon :name="category.is_active ? 'eye' : 'eyeOff'" />
                  </button>
                  <button
                    type="button"
                    @click.stop="openEditModal(category)"
                    class="p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                    :title="$t('admin-categories-action-edit')"
                    :aria-label="$t('admin-categories-action-edit')"
                  >
                    <Icon name="rename" />
                  </button>
                  <button
                    type="button"
                    @click.stop="confirmDelete(category)"
                    class="p-2 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-lg transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                    :title="$t('admin-categories-action-delete')"
                    :aria-label="$t('admin-categories-action-delete')"
                  >
                    <Icon name="trash" />
                  </button>
                </div>
              </div>
            </div>
          </template>

          <!-- Drop indicator line (after last item) -->
          <div
            v-if="canDrag && dragState.isDragging && dragState.insertIndex === filteredCategories.length && dragState.draggedIndex !== filteredCategories.length - 1"
            class="h-0.5 bg-accent rounded-full mx-4 transition-all"
          />

          <!-- No results -->
          <div v-if="filteredCategories.length === 0 && categories.length > 0" class="text-center py-8 text-secondary text-sm">
            <template v-if="searchQuery.trim()">{{ $t('admin-categories-no-search-results', { query: searchQuery }) }}</template>
            <template v-else>{{ $t('admin-categories-no-filter-results') }}</template>
          </div>

          <!-- Empty state -->
          <EmptyState
            v-if="categories.length === 0 && !isFirstLoad"
            icon="folder"
            :title="$t('empty-categories-title')"
            :description="$t('empty-categories-description')"
            :action-label="$t('admin-categories-empty-action')"
            variant="card"
            @action="openCreateModal"
          />
        </div>
      </div>
    </template>

    <template #panel>
      <CategoryEditPanel
        v-if="isPanelOpen"
        :category="panelCategory"
        :availableGroups="availableGroups"
        class="flex-1"
        @save="onPanelSave"
        @close="onPanelClose"
        @delete="onPanelDelete"
      />
    </template>
  </SplitPanelLayout>

  <!-- Create/Edit Category Modal (mobile only) -->
  <Modal
    v-if="isMobile"
    :show="showCategoryModal"
    :title="editingCategory ? $t('admin-categories-modal-edit-title') : $t('admin-categories-modal-create-title')"
    size="md"
    @close="showCategoryModal = false"
  >
    <form @submit.prevent="saveCategory" class="flex flex-col gap-4">
      <!-- Name -->
      <div>
        <label class="block text-sm font-medium text-primary mb-1">{{ $t('admin-categories-modal-name-label') }}</label>
        <FormInput
          v-model="categoryForm.name"
          :placeholder="$t('admin-categories-modal-name-placeholder')"
          required
        />
      </div>

      <!-- Description -->
      <div>
        <label class="block text-sm font-medium text-primary mb-1">{{ $t('admin-categories-modal-description-label') }}</label>
        <FormTextarea
          v-model="categoryForm.description"
          :placeholder="$t('admin-categories-modal-description-placeholder')"
          :rows="2"
        />
      </div>

      <!-- Icon -->
      <div>
        <label class="block text-sm font-medium text-primary mb-2">{{ $t('admin-categories-modal-icon-label') }}</label>
        <div class="flex flex-wrap gap-2">
          <button
            v-for="icon in iconOptions"
            :key="icon.name"
            type="button"
            @click="categoryForm.icon = icon.name"
            class="w-10 h-10 rounded-lg border-2 flex items-center justify-center transition-all"
            :class="categoryForm.icon === icon.name ? 'border-accent bg-accent/10' : 'border-default hover:border-strong'"
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
      <ColorHueSlider v-model="categoryForm.color" :label="$t('admin-categories-modal-color-label')" />

      <!-- Active status (only for editing) -->
      <ToggleSwitch
        v-if="editingCategory"
        v-model="categoryForm.is_active"
        size="sm"
        :label="$t('admin-categories-modal-active-label')"
      />

      <!-- Group visibility -->
      <div>
        <label class="block text-sm font-medium text-primary mb-2">
          {{ $t('admin-categories-modal-visibility-label') }}
          <span class="text-tertiary font-normal ml-1">{{ $t('admin-categories-modal-visibility-hint') }}</span>
        </label>
        <div v-if="availableGroups.length > 0" class="max-h-40 overflow-y-auto border border-default rounded-lg divide-y divide-default">
          <div
            v-for="group in availableGroups"
            :key="group.id"
            class="flex items-center gap-3 p-2.5 hover:bg-surface-hover cursor-pointer transition-colors"
            @click="toggleGroupVisibility(group.id)"
          >
            <Checkbox
              :model-value="categoryForm.visible_to_group_ids.includes(group.id)"
              size="sm"
              :aria-label="$t('admin-categories-modal-visibility-toggle-aria', { name: group.name })"
              @change="toggleGroupVisibility(group.id)"
              @click.stop
            />
            <div
              class="w-3 h-3 rounded-full flex-shrink-0"
              :style="{ backgroundColor: group.color || DEFAULT_COLOR }"
            />
            <span class="text-sm text-primary">{{ group.name }}</span>
            <span class="text-xs text-tertiary ml-auto">{{ $t('admin-categories-modal-group-members', { count: group.member_count }) }}</span>
          </div>
        </div>
        <p v-else class="text-sm text-tertiary py-2">
          {{ $t('admin-categories-modal-no-groups') }} <router-link to="/admin/groups" class="text-accent hover:underline">{{ $t('admin-categories-modal-create-groups-link') }}</router-link> {{ $t('admin-categories-modal-create-groups-suffix') }}
        </p>
      </div>

      <!-- Actions -->
      <div class="flex justify-end gap-2 pt-2">
        <Button variant="ghost" @click="showCategoryModal = false">
          {{ $t('admin-categories-modal-cancel') }}
        </Button>
        <Button type="submit" :loading="isSaving">
          {{ editingCategory ? $t('admin-categories-modal-save') : $t('admin-categories-modal-create') }}
        </Button>
      </div>
    </form>
  </Modal>

  <!-- Delete Confirmation Modal -->
  <Modal
    :show="showDeleteConfirm"
    :title="$t('admin-categories-delete-title')"
    size="sm"
    @close="showDeleteConfirm = false"
  >
    <div class="flex flex-col gap-4">
      <p class="text-secondary">
        {{ $t('admin-categories-delete-message', { name: categoryToDelete?.name ?? '' }) }}
      </p>

      <div class="flex justify-end gap-2 pt-2">
        <Button variant="ghost" @click="showDeleteConfirm = false">
          {{ $t('admin-categories-delete-cancel') }}
        </Button>
        <Button variant="danger" :loading="isSaving" @click="deleteCategory">
          {{ $t('admin-categories-delete-confirm') }}
        </Button>
      </div>
    </div>
  </Modal>

  <!-- Floating drag preview -->
  <Teleport to="body">
    <div
      v-if="canDrag && draggedCategory"
      class="fixed pointer-events-none z-cursor"
      :style="{
        left: (dragState.pointerX - dragState.offsetX) + 'px',
        top: (dragState.pointerY - dragState.offsetY) + 'px',
        width: listRef ? (listRef.offsetWidth - 8) + 'px' : 'auto',
      }"
    >
      <div class="bg-surface border border-accent rounded-xl shadow-lg shadow-black/10 opacity-90">
        <div class="p-4 flex items-center gap-4">
          <!-- Grip icon -->
          <div class="flex-shrink-0 p-1 text-tertiary">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
              <path d="M7 2a2 2 0 1 0 0 4 2 2 0 0 0 0-4zM13 2a2 2 0 1 0 0 4 2 2 0 0 0 0-4zM7 8a2 2 0 1 0 0 4 2 2 0 0 0 0-4zM13 8a2 2 0 1 0 0 4 2 2 0 0 0 0-4zM7 14a2 2 0 1 0 0 4 2 2 0 0 0 0-4zM13 14a2 2 0 1 0 0 4 2 2 0 0 0 0-4z" />
            </svg>
          </div>
          <!-- Icon -->
          <div
            class="w-10 h-10 rounded-lg flex items-center justify-center flex-shrink-0"
            :style="{ backgroundColor: (draggedCategory.color || DEFAULT_COLOR) + '20' }"
          >
            <svg
              xmlns="http://www.w3.org/2000/svg"
              class="h-5 w-5"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              stroke-width="2"
              :style="{ color: draggedCategory.color || DEFAULT_COLOR }"
            >
              <path stroke-linecap="round" stroke-linejoin="round" :d="getIconPath(draggedCategory.icon || 'folder')" />
            </svg>
          </div>
          <!-- Name -->
          <div class="flex-1 min-w-0">
            <h3 class="font-medium text-primary">{{ draggedCategory.name }}</h3>
            <p v-if="draggedCategory.description" class="text-sm text-secondary mt-0.5 truncate">
              {{ draggedCategory.description }}
            </p>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
  </div>
</template>
