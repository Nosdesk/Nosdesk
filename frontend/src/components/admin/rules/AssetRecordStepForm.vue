<script setup lang="ts">
/**
 * Settings for the rule steps that write work records on the ticket's
 * linked assets: opening one of a type, with an optional summary, and
 * completing the ticket's open ones with an outcome. A completion may name
 * no type, and then offers every live type's outcomes; writing a record
 * where there is none needs a type. A type archived since the rule was
 * saved stays in the list, so the step still reads. A workspace with no
 * live types is pointed at where they are added.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';

import BaseDropdown from '@/components/common/BaseDropdown.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import FormInput from '@/components/common/FormInput.vue';
import { catalogueOutcomeName, recordTypeName } from '@/components/admin/assetRecordTypes/recordTypeForm';
import type { AssetRecordType } from '@nosdesk/core/types/asset';

const props = withDefaults(
  defineProps<{
    kind: 'open_asset_record' | 'complete_asset_records';
    config: Record<string, unknown> | undefined;
    /** The workspace's record types, archived ones too. */
    types: AssetRecordType[];
    /** Whether `types` has loaded, so an empty list means there are none. */
    loaded?: boolean;
  }>(),
  { loaded: true },
);

const emit = defineEmits<{
  (e: 'update', field: string, value: unknown): void;
}>();

const { $t: t } = useFluent();

const ANY = '';

const slug = computed(() => String(props.config?.record_type ?? ''));
const outcome = computed(() => String(props.config?.outcome ?? ''));
const creates = computed(() => props.config?.create_if_none === true);
const chosen = computed(() => props.types.find((x) => x.slug === slug.value) ?? null);
const noTypes = computed(() => props.loaded && !props.types.some((x) => !x.archived_at));

const typeOptions = computed(() => {
  const offered = props.types.filter((x) => !x.archived_at || x.slug === slug.value);
  const options = offered.map((x) => ({ value: x.slug, label: recordTypeName(t, x) }));
  return props.kind === 'complete_asset_records'
    ? [{ value: ANY, label: t('admin-rule-editor-records-type-any') }, ...options]
    : options;
});

function outcomeName(type: AssetRecordType, key: string): string {
  const entry = type.outcomes.find((o) => o.key === key);
  return entry?.label ?? catalogueOutcomeName(t, type.slug, key);
}

/** The chosen type's outcomes or, with no type, each live type's, named
 *  by the first type that has it. */
const outcomeOptions = computed(() => {
  const types = chosen.value ? [chosen.value] : props.types.filter((x) => !x.archived_at);
  const seen = new Map<string, string>();
  for (const type of types) {
    for (const o of type.outcomes) {
      if (!seen.has(o.key)) seen.set(o.key, outcomeName(type, o.key));
    }
  }
  return [...seen].map(([value, label]) => ({ value, label }));
});

function setType(next: string): void {
  emit('update', 'record_type', next === ANY ? null : next);
  if (props.kind !== 'complete_asset_records') return;
  const type = props.types.find((x) => x.slug === next);
  if (outcome.value && type && !type.outcomes.some((o) => o.key === outcome.value)) {
    emit('update', 'outcome', '');
  }
  if (next === ANY && creates.value) emit('update', 'create_if_none', false);
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <BaseDropdown
      :model-value="slug"
      :options="typeOptions"
      :label="t('admin-rule-editor-records-type-label')"
      :placeholder="t('admin-rule-editor-records-type-placeholder')"
      size="sm"
      data-test="record-step-type"
      @update:model-value="setType(String($event))"
    />
    <FormInput
      v-if="kind === 'open_asset_record'"
      :model-value="String(config?.summary ?? '')"
      :label="t('admin-rule-editor-records-summary-label')"
      :description="t('admin-rule-editor-records-summary-hint')"
      maxlength="255"
      size="sm"
      data-test="record-step-summary"
      @update:model-value="emit('update', 'summary', $event)"
    />
    <template v-else>
      <BaseDropdown
        :model-value="outcome"
        :options="outcomeOptions"
        :label="t('admin-rule-editor-records-outcome-label')"
        :placeholder="t('admin-rule-editor-records-outcome-placeholder')"
        size="sm"
        data-test="record-step-outcome"
        @update:model-value="emit('update', 'outcome', String($event))"
      />
      <Checkbox
        :model-value="creates"
        :disabled="!slug"
        :label="t('admin-rule-editor-records-create-label')"
        data-test="record-step-create"
        @update:model-value="emit('update', 'create_if_none', $event)"
      />
    </template>
    <p v-if="noTypes" class="text-xs text-secondary" data-test="record-step-no-types">
      {{ t('admin-rule-editor-records-no-types') }}
      <RouterLink :to="{ name: 'admin-asset-record-types' }" class="text-accent hover:underline">
        {{ t('admin-rule-editor-records-no-types-link') }}
      </RouterLink>
    </p>
    <p class="text-xs text-secondary">
      {{ t(kind === 'open_asset_record' ? 'admin-rule-editor-records-open-hint' : 'admin-rule-editor-records-complete-hint') }}
    </p>
  </div>
</template>
