<script setup lang="ts">
/**
 * Data-entry-side renderer for attributes typed
 * `{ "type": "string", "format": "asset-ref", "assetKind"?: "slug" }`.
 * Emits the selected asset's id (stringified, matching the wire
 * shape the backend's `format: "asset-ref"` validator accepts).
 *
 * `assetKind` (when present in the schema) filters the picker to
 * assets of that kind only. Empty / undefined means "any kind".
 *
 * Backed by `SearchableDropdown` because the asset list grows
 * unboundedly in any real workspace. The typeahead match runs
 * against the asset name + the displayed id.
 */
import { computed, onMounted, ref } from 'vue';
import { useFluent } from 'fluent-vue';

import SearchableDropdown, {
  type DropdownOption,
} from '@/components/common/SearchableDropdown.vue';
import { getAssets } from '@/services/assetService';
import type { Asset } from '@nosdesk/core/types/asset';

const { $t } = useFluent();

const props = defineProps<{
  modelValue: string;
  /** Optional asset-kind slug. Filters the picker; undefined =
   * any kind. */
  assetKind?: string;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  'update:modelValue': [value: string];
}>();

const assets = ref<Asset[]>([]);
const isLoading = ref(false);
const loadError = ref('');

const filteredAssets = computed<Asset[]>(() =>
  props.assetKind
    ? assets.value.filter((a) => a.kind === props.assetKind)
    : assets.value,
);

const options = computed<DropdownOption[]>(() =>
  filteredAssets.value.map((a) => ({
    value: String(a.id),
    label: a.name,
    description: `#${a.id}`,
  })),
);

onMounted(async () => {
  isLoading.value = true;
  try {
    assets.value = await getAssets();
  } catch {
    loadError.value = $t('asset-kind-attribute-asset-load-error');
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
      :placeholder="isLoading ? $t('asset-kind-attribute-asset-loading') : $t('asset-kind-attribute-asset-none')"
      :disabled="disabled || isLoading"
      size="sm"
      @update:model-value="(v) => emit('update:modelValue', v)"
    />
    <p v-if="loadError" class="text-xs text-status-error">{{ loadError }}</p>
    <p
      v-else-if="!isLoading && assetKind && filteredAssets.length === 0"
      class="text-xs text-tertiary"
    >
      {{ $t('asset-kind-attribute-asset-empty-for-scope', { kind: assetKind }) }}
    </p>
  </div>
</template>
