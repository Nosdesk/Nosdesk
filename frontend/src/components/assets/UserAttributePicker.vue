<script setup lang="ts">
/**
 * Data-entry-side renderer for attributes typed
 * `{ "type": "string", "format": "user-uuid" }`. Emits the
 * selected user's UUID as the v-model value.
 *
 * Pragmatic v1 implementation: a `<select>` populated with every
 * user fetched via `userService.getAllUsers()`. For workspaces
 * with hundreds of users this is awkward but functional; a
 * typeahead combobox replacement (mirroring the ticket-sidebar
 * `UserPicker`) is a follow-up polish.
 *
 * Loading + empty + error states are all degenerate to a plain
 * disabled select rendering the existing value (if any) so the
 * form still renders during the fetch and never traps the admin.
 */
import { onMounted, ref } from 'vue';
import { useFluent } from 'fluent-vue';

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
  <select
    :value="modelValue"
    :disabled="disabled || isLoading"
    class="bg-surface-alt rounded-lg border border-default px-3 py-2 text-primary text-sm"
    @change="(e) => emit('update:modelValue', (e.target as HTMLSelectElement).value)"
  >
    <option value="">
      {{ isLoading ? $t('asset-kind-attribute-user-loading') : $t('asset-kind-attribute-user-none') }}
    </option>
    <option v-for="u in users" :key="u.uuid" :value="u.uuid">
      {{ u.name }} <span v-if="u.email">({{ u.email }})</span>
    </option>
  </select>
  <p v-if="loadError" class="text-xs text-status-error">{{ loadError }}</p>
</template>
