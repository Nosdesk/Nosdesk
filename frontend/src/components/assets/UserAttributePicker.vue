<script setup lang="ts">
/**
 * Data-entry-side renderer for attributes typed
 * `{ "type": "string", "format": "user-uuid" }`. Emits the
 * selected user's UUID as the v-model value.
 *
 * Backed by `SearchableDropdown` because the user list grows
 * unboundedly in larger workspaces and a scroll-only `<select>`
 * stops being usable past ~30 entries. The typeahead match runs
 * against name + email so admins can search by either.
 *
 * Loading + error states render as a disabled dropdown so the
 * surrounding form layout never shifts.
 */
import { computed, onMounted, ref } from 'vue';
import { useFluent } from 'fluent-vue';

import SearchableDropdown, {
  type DropdownOption,
} from '@/components/common/SearchableDropdown.vue';
import userService from '@/services/userService';
import type { User } from '@/types/user';

const { $t } = useFluent();

defineProps<{
  modelValue: string;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  'update:modelValue': [value: string];
}>();

const users = ref<User[]>([]);
const isLoading = ref(false);
const loadError = ref('');

const options = computed<DropdownOption[]>(() =>
  users.value.map((u) => ({
    value: u.uuid,
    label: u.name,
    // SearchableDropdown matches against label + description, so
    // putting the email here makes "alex@" find Alex's row even
    // when the displayed label is just the name.
    description: u.email ?? undefined,
  })),
);

onMounted(async () => {
  isLoading.value = true;
  try {
    users.value = await userService.getAllUsers();
  } catch {
    loadError.value = $t('asset-kind-attribute-user-load-error');
  } finally {
    isLoading.value = false;
  }
});
</script>

<template>
  <div class="flex flex-col gap-1">
    <SearchableDropdown
      :model-value="modelValue"
      :options="options"
      :placeholder="isLoading ? $t('asset-kind-attribute-user-loading') : $t('asset-kind-attribute-user-none')"
      :disabled="disabled || isLoading"
      size="sm"
      @update:model-value="(v) => emit('update:modelValue', v)"
    />
    <p v-if="loadError" class="text-xs text-status-error">{{ loadError }}</p>
  </div>
</template>
