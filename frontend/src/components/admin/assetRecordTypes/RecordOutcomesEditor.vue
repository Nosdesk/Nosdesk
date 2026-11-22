<script setup lang="ts">
/**
 * A record type's outcomes, in order. Each has a name and one of the four
 * categories behaviour keys off (a `replaced` outcome offers the
 * replacement link, `done` completes a schedule, reports group by them).
 *
 * A saved outcome's key is stored on records, so it stays as it is; a new
 * outcome takes its key from its name. On a built-in type, an outcome from
 * the catalogue may have no name of its own and shows the catalogue's.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';

import BaseDropdown from '@/components/common/BaseDropdown.vue';
import Button from '@/components/common/Button.vue';
import IconButton from '@/components/common/IconButton.vue';
import FormInput from '@/components/common/FormInput.vue';
import type { AssetRecordOutcomeCategory } from '@nosdesk/core/types/asset';
import {
  OUTCOME_CATEGORIES,
  catalogueOutcomeName,
  keyFrom,
  uniqueKey,
  type OutcomeRow,
} from './recordTypeForm';

const { $t: t } = useFluent();

const props = defineProps<{
  modelValue: OutcomeRow[];
  /** The type's slug, for catalogue names. */
  typeSlug: string;
  /** Keys of the built-in's catalogue outcomes; empty on a type of the
   *  workspace's own. */
  catalogueKeys: string[];
}>();

const emit = defineEmits<{
  'update:modelValue': [value: OutcomeRow[]];
}>();

const categoryOptions = computed(() =>
  OUTCOME_CATEGORIES.map((c) => ({
    value: c,
    label: t(`admin-asset-record-types-category-${c}`),
    description: t(`admin-asset-record-types-category-${c}-hint`),
  })),
);

function fromCatalogue(row: OutcomeRow): boolean {
  return !row.isNew && props.catalogueKeys.includes(row.key);
}

function placeholder(row: OutcomeRow): string {
  return fromCatalogue(row)
    ? catalogueOutcomeName(t, props.typeSlug, row.key)
    : t('admin-asset-record-types-outcome-name-placeholder');
}

function replaceAt(index: number, row: OutcomeRow): void {
  const next = props.modelValue.slice();
  next[index] = row;
  emit('update:modelValue', next);
}

function rename(index: number, label: string): void {
  const row = props.modelValue[index];
  if (!row.isNew) {
    replaceAt(index, { ...row, label });
    return;
  }
  const others = props.modelValue.filter((_, i) => i !== index).map((r) => r.key);
  replaceAt(index, { ...row, label, key: uniqueKey(keyFrom(label), others) });
}

function setCategory(index: number, category: AssetRecordOutcomeCategory): void {
  replaceAt(index, { ...props.modelValue[index], category });
}

function move(index: number, by: -1 | 1): void {
  const to = index + by;
  if (to < 0 || to >= props.modelValue.length) return;
  const next = props.modelValue.slice();
  [next[index], next[to]] = [next[to], next[index]];
  emit('update:modelValue', next);
}

function remove(index: number): void {
  emit(
    'update:modelValue',
    props.modelValue.filter((_, i) => i !== index),
  );
}

function add(): void {
  const key = uniqueKey('outcome', props.modelValue.map((r) => r.key));
  emit('update:modelValue', [
    ...props.modelValue,
    { key, category: 'done', label: '', isNew: true },
  ]);
}
</script>

<template>
  <div class="flex flex-col gap-3">
    <p v-if="modelValue.length === 0" class="text-sm text-tertiary">
      {{ t('admin-asset-record-types-outcomes-empty') }}
    </p>
    <ul v-else class="flex flex-col gap-2">
      <li
        v-for="(row, index) in modelValue"
        :key="row.isNew ? `new-${index}` : row.key"
        data-test="outcome-row"
        class="flex flex-col sm:flex-row sm:items-end gap-2 p-3 bg-surface border border-default rounded-lg"
      >
        <div class="flex-1 min-w-0 flex flex-col gap-1">
          <FormInput
            :model-value="row.label"
            :label="t('admin-asset-record-types-outcome-name')"
            :placeholder="placeholder(row)"
            :aria-label="t('admin-asset-record-types-outcome-name')"
            :required="!fromCatalogue(row)"
            maxlength="255"
            size="sm"
            @update:model-value="(v: string) => rename(index, v)"
          />
          <code class="text-xs text-tertiary truncate">{{ row.key }}</code>
        </div>
        <div class="flex items-end gap-2">
          <BaseDropdown
            :model-value="row.category"
            :options="categoryOptions"
            :label="t('admin-asset-record-types-outcome-category')"
            size="sm"
            class="flex-1 sm:flex-none sm:w-48"
            @update:model-value="(v) => setCategory(index, v as AssetRecordOutcomeCategory)"
          />
          <div class="flex items-center gap-1 pb-1">
            <IconButton
              icon="arrowUp"
              size="sm"
              variant="ghost"
              :label="t('admin-asset-record-types-outcome-move-up')"
              :disabled="index === 0"
              @click="move(index, -1)"
            />
            <IconButton
              icon="arrowDown"
              size="sm"
              variant="ghost"
              :label="t('admin-asset-record-types-outcome-move-down')"
              :disabled="index === modelValue.length - 1"
              @click="move(index, 1)"
            />
            <IconButton
              icon="trash"
              size="sm"
              variant="ghost-danger"
              :label="t('admin-asset-record-types-outcome-remove')"
              @click="remove(index)"
            />
          </div>
        </div>
      </li>
    </ul>
    <Button variant="secondary" size="sm" icon="add" class="self-start" @click="add">
      {{ t('admin-asset-record-types-outcome-add') }}
    </Button>
  </div>
</template>
