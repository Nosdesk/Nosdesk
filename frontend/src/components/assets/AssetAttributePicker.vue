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
 * Pragmatic v1 implementation, same trade-off as
 * UserAttributePicker: a `<select>` populated by `getAssets()`.
 * A scoped typeahead combobox is a follow-up polish.
 */
import { computed, onMounted, ref } from 'vue';
import { useFluent } from 'fluent-vue';

import { getAssets } from '@/services/assetService';
import type { Asset } from '@/types/asset';

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
  <select
    :value="modelValue"
    :disabled="disabled || isLoading"
    class="bg-surface-alt rounded-lg border border-default px-3 py-2 text-primary text-sm"
    @change="(e) => emit('update:modelValue', (e.target as HTMLSelectElement).value)"
  >
    <option value="">
      {{ isLoading ? $t('asset-kind-attribute-asset-loading') : $t('asset-kind-attribute-asset-none') }}
    </option>
    <option v-for="a in filteredAssets" :key="a.id" :value="String(a.id)">
      {{ a.name }} (#{{ a.id }})
    </option>
  </select>
  <p v-if="loadError" class="text-xs text-status-error">{{ loadError }}</p>
  <p
    v-else-if="!isLoading && assetKind && filteredAssets.length === 0"
    class="text-xs text-tertiary"
  >
    {{ $t('asset-kind-attribute-asset-empty-for-scope', { kind: assetKind }) }}
  </p>
</template>
