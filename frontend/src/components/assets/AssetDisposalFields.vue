<script lang="ts">
/** The disposal as the form holds it: text fields as typed. */
export interface DisposalDraft {
  sanitization_method: string;
  data_bearing: boolean;
  itad_vendor: string;
  notes: string;
}

export function emptyDisposal(): DisposalDraft {
  return { sanitization_method: 'clear', data_bearing: true, itad_vendor: '', notes: '' };
}

/** The draft as the server takes it. */
export function disposalBody(draft: DisposalDraft) {
  return {
    sanitization_method: draft.sanitization_method,
    data_bearing: draft.data_bearing,
    itad_vendor: draft.itad_vendor.trim() || null,
    notes: draft.notes.trim() || null,
  };
}
</script>

<script setup lang="ts">
/**
 * The disposal record a move to Disposed carries: how the data was
 * sanitized, whether the asset held data, the ITAD vendor and notes. Shared
 * by the status change dialog and the replacement carry-overs. The fields
 * are siblings, so the parent's grid or column lays them out.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery } from '@pinia/colada';
import Autocomplete from '@/components/common/Autocomplete.vue';
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import { assetLifecycleKeys, assetLifecycleService } from '@nosdesk/core/services/assetLifecycleService';

const props = withDefaults(
  defineProps<{
    modelValue: DisposalDraft;
    /** Read the vendor suggestions only while shown. */
    enabled?: boolean;
    size?: 'sm' | 'md';
    /** Classes for the vendor and notes, which want the full width. */
    wideClass?: string;
  }>(),
  { enabled: true, size: undefined, wideClass: '' },
);
const emit = defineEmits<{ 'update:modelValue': [value: DisposalDraft] }>();

const fluent = useFluent();
const t = (key: string) => fluent.$t(key);

// Prior ITAD vendors for the suggestions (cached workspace-wide; a new
// vendor can still be typed).
const itadVendorsQuery = useQuery({
  key: () => assetLifecycleKeys.itadVendors,
  query: () => assetLifecycleService.listItadVendors(),
  enabled: () => props.enabled,
});
const itadVendors = computed<string[]>(() =>
  Array.isArray(itadVendorsQuery.data.value) ? itadVendorsQuery.data.value : [],
);

const sanitizationOptions = computed(() =>
  ['clear', 'purge', 'destroy', 'none'].map((m) => ({
    value: m,
    label: t(`asset-disposal-method-${m}`),
  })),
);

function set<K extends keyof DisposalDraft>(key: K, value: DisposalDraft[K]) {
  emit('update:modelValue', { ...props.modelValue, [key]: value });
}
</script>

<template>
  <BaseDropdown
    :model-value="modelValue.sanitization_method"
    :options="sanitizationOptions"
    :label="t('asset-disposal-method')"
    size="sm"
    @update:model-value="set('sanitization_method', String($event))"
  />
  <Checkbox
    :model-value="modelValue.data_bearing"
    :label="t('asset-disposal-data-bearing')"
    :size="size"
    @update:model-value="set('data_bearing', !!$event)"
  />
  <Autocomplete
    :model-value="modelValue.itad_vendor"
    :options="itadVendors"
    :label="t('asset-disposal-itad-vendor')"
    :size="size"
    :class="wideClass"
    @update:model-value="set('itad_vendor', String($event ?? ''))"
  />
  <FormTextarea
    :model-value="modelValue.notes"
    :label="t('asset-disposal-notes')"
    :rows="2"
    :class="wideClass"
    @update:model-value="set('notes', String($event ?? ''))"
  />
</template>
