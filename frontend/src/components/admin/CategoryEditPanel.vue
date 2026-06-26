<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import ColorHueSlider from '@/components/common/ColorHueSlider.vue';
import Icon from '@/components/common/Icon.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import type { CategoryWithVisibility } from '@nosdesk/core/types/category';
import type { GroupWithMemberCount } from '@nosdesk/core/types/group';

const fluent = useFluent();

const props = defineProps<{
  category?: CategoryWithVisibility | null;
  availableGroups: GroupWithMemberCount[];
}>();

const emit = defineEmits<{
  save: [formData: {
    name: string;
    description: string;
    color: string;
    icon: string;
    is_active: boolean;
    visible_to_group_ids: number[];
  }];
  close: [];
  delete: [category: CategoryWithVisibility];
}>();

const isEditMode = () => !!props.category;

// Form state
const categoryForm = ref({
  name: '',
  description: '',
  color: '#6366f1',
  icon: 'folder',
  is_active: true,
  visible_to_group_ids: [] as number[]
});

// Available icons. SVG paths stay literal; labels resolve through
// Fluent so the icon tooltip is localised.
const iconOptions = computed(() => [
  { name: 'folder', label: fluent.$t('admin-categories-edit-icon-folder'), path: 'M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z' },
  { name: 'tag', label: fluent.$t('admin-categories-edit-icon-tag'), path: 'M7 7h.01M7 3h5c.512 0 1.024.195 1.414.586l7 7a2 2 0 010 2.828l-7 7a2 2 0 01-2.828 0l-7-7A1.994 1.994 0 013 12V7a4 4 0 014-4z' },
  { name: 'bug', label: fluent.$t('admin-categories-edit-icon-bug'), path: 'M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z' },
  { name: 'cog', label: fluent.$t('admin-categories-edit-icon-settings'), path: 'M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z M15 12a3 3 0 11-6 0 3 3 0 016 0z' },
  { name: 'lightbulb', label: fluent.$t('admin-categories-edit-icon-idea'), path: 'M9.663 17h4.673M12 3v1m6.364 1.636l-.707.707M21 12h-1M4 12H3m3.343-5.657l-.707-.707m2.828 9.9a5 5 0 117.072 0l-.548.547A3.374 3.374 0 0014 18.469V19a2 2 0 11-4 0v-.531c0-.895-.356-1.754-.988-2.386l-.548-.547z' },
  { name: 'question', label: fluent.$t('admin-categories-edit-icon-question'), path: 'M8.228 9c.549-1.165 2.03-2 3.772-2 2.21 0 4 1.343 4 3 0 1.4-1.278 2.575-3.006 2.907-.542.104-.994.54-.994 1.093m0 3h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z' },
  { name: 'exclamation', label: fluent.$t('admin-categories-edit-icon-alert'), path: 'M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z' },
  { name: 'star', label: fluent.$t('admin-categories-edit-icon-star'), path: 'M11.049 2.927c.3-.921 1.603-.921 1.902 0l1.519 4.674a1 1 0 00.95.69h4.915c.969 0 1.371 1.24.588 1.81l-3.976 2.888a1 1 0 00-.363 1.118l1.518 4.674c.3.922-.755 1.688-1.538 1.118l-3.976-2.888a1 1 0 00-1.176 0l-3.976 2.888c-.783.57-1.838-.197-1.538-1.118l1.518-4.674a1 1 0 00-.363-1.118l-3.976-2.888c-.784-.57-.38-1.81.588-1.81h4.914a1 1 0 00.951-.69l1.519-4.674z' },
]);

// Toggle group visibility
const toggleGroupVisibility = (groupId: number) => {
  const index = categoryForm.value.visible_to_group_ids.indexOf(groupId);
  if (index === -1) {
    categoryForm.value.visible_to_group_ids.push(groupId);
  } else {
    categoryForm.value.visible_to_group_ids.splice(index, 1);
  }
};

// Populate form when category prop changes
const populateForm = () => {
  if (props.category) {
    categoryForm.value = {
      name: props.category.name,
      description: props.category.description || '',
      color: props.category.color || '#6366f1',
      icon: props.category.icon || 'folder',
      is_active: props.category.is_active,
      visible_to_group_ids: props.category.visible_to_groups.map(g => g.id)
    };
  } else {
    categoryForm.value = {
      name: '',
      description: '',
      color: '#6366f1',
      icon: 'folder',
      is_active: true,
      visible_to_group_ids: []
    };
  }
};

// Watch for category changes
watch(() => props.category, populateForm, { immediate: true });

const handleSubmit = () => {
  emit('save', { ...categoryForm.value });
};
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- Panel Header -->
    <div class="sticky top-0 z-10 bg-app border-b border-default px-4 sm:px-6 py-3 flex items-center justify-between gap-3 flex-shrink-0">
      <h2 class="text-lg font-semibold text-primary truncate">
        {{ isEditMode() ? $t('admin-categories-edit-title-edit') : $t('admin-categories-edit-title-create') }}
      </h2>
      <div class="flex items-center gap-2 flex-shrink-0">
        <button
          v-if="isEditMode() && category"
          @click="emit('delete', category)"
          class="p-1.5 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-lg transition-colors"
          :title="$t('admin-categories-edit-delete-tooltip')"
        >
          <Icon name="trash" />
        </button>
        <button
          @click="emit('close')"
          class="p-1.5 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
          :title="$t('admin-categories-edit-close-tooltip')"
        >
          <Icon name="close" size="md" />
        </button>
      </div>
    </div>

    <!-- Form Content -->
    <div class="flex-1 overflow-y-auto">
      <form @submit.prevent="handleSubmit" class="flex flex-col gap-4 px-4 sm:px-6 py-4">
        <!-- Name -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">{{ $t('admin-categories-edit-name-label') }}</label>
          <input
            v-model="categoryForm.name"
            type="text"
            :placeholder="$t('admin-categories-edit-name-placeholder')"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            required
          />
        </div>

        <!-- Description -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">{{ $t('admin-categories-edit-description-label') }}</label>
          <textarea
            v-model="categoryForm.description"
            :placeholder="$t('admin-categories-edit-description-placeholder')"
            rows="2"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent resize-none"
          />
        </div>

        <!-- Icon -->
        <div>
          <label class="block text-sm font-medium text-primary mb-2">{{ $t('admin-categories-edit-icon-label') }}</label>
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
        <ColorHueSlider v-model="categoryForm.color" :label="$t('admin-categories-edit-color-label')" />

        <!-- Active status (only for editing) -->
        <ToggleSwitch
          v-if="isEditMode()"
          v-model="categoryForm.is_active"
          size="sm"
          :label="$t('admin-categories-edit-active-label')"
        />

        <!-- Group visibility -->
        <div>
          <label class="block text-sm font-medium text-primary mb-2">
            {{ $t('admin-categories-edit-visibility-label') }}
            <span class="text-tertiary font-normal ml-1">{{ $t('admin-categories-edit-visibility-hint') }}</span>
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
                :aria-label="$t('admin-categories-edit-visibility-toggle-aria', { name: group.name })"
                @change="toggleGroupVisibility(group.id)"
                @click.stop
              />
              <div
                class="w-3 h-3 rounded-full flex-shrink-0"
                :style="{ backgroundColor: group.color || '#6366f1' }"
              />
              <span class="text-sm text-primary">{{ group.name }}</span>
              <span class="text-xs text-tertiary ml-auto">{{ $t('admin-categories-edit-member-count', { count: group.member_count }) }}</span>
            </div>
          </div>
          <p v-else class="text-sm text-tertiary py-2">
            {{ $t('admin-categories-edit-no-groups') }}
          </p>
        </div>

        <!-- Actions -->
        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="emit('close')"
            class="px-4 py-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
          >
            {{ $t('admin-categories-edit-cancel') }}
          </button>
          <button
            type="submit"
            class="px-4 py-2 bg-accent text-on-accent rounded-lg hover:opacity-90 transition-colors disabled:opacity-50"
          >
            {{ isEditMode() ? $t('admin-categories-edit-save') : $t('admin-categories-edit-create') }}
          </button>
        </div>
      </form>
    </div>
  </div>
</template>
