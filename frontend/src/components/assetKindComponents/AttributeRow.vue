<script setup lang="ts">
/**
 * Single attribute row in the schema builder. v-model'd against
 * one `AttributeDef`; the parent (AttributeEditor) owns add /
 * remove / reorder. Per-type controls render conditionally so
 * the row stays compact when there's nothing to configure.
 *
 * `raw` is a degraded shape for properties the parser didn't
 * recognise (hand-edited schemas with unknown keywords); the row
 * surfaces them as read-only with a note so the admin doesn't
 * silently lose data by editing in this view.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';

import BaseDropdown from '@/components/common/BaseDropdown.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import FormInput from '@/components/common/FormInput.vue';
import FormNumber from '@/components/common/FormNumber.vue';
import Icon from '@/components/common/Icon.vue';
import {
  ATTRIBUTE_KINDS_ORDERED,
  isValidAttributeName,
  type AttributeDef,
  type AttributeKind,
} from './attributeSchema';
import { useAssetKindsQuery } from '@/composables/useAssetKindsQuery';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const props = defineProps<{
  modelValue: AttributeDef;
  /** Whether this row is the first / last in the list. Drives
   * the up/down move-button enabled state. */
  isFirst: boolean;
  isLast: boolean;
}>();

const emit = defineEmits<{
  'update:modelValue': [value: AttributeDef];
  remove: [];
  'move-up': [];
  'move-down': [];
}>();

function patch(part: Partial<AttributeDef>): void {
  emit('update:modelValue', { ...props.modelValue, ...part });
}

function onKindChange(kind: AttributeKind): void {
  // Switching kind clears per-type config that doesn't apply to
  // the new kind so stale `maxLength` on a `boolean` doesn't
  // leak into the serialised schema.
  const base: AttributeDef = {
    name: props.modelValue.name,
    kind,
    required: props.modelValue.required,
    description: props.modelValue.description,
  };
  if (kind === 'select' || kind === 'multi_select') base.enumValues = props.modelValue.enumValues ?? [];
  emit('update:modelValue', base);
}

const nameInvalid = computed(
  () => props.modelValue.name !== '' && !isValidAttributeName(props.modelValue.name),
);

// Enum editor: render values as a CSV-style chip row with a
// trailing input. Add commits on Enter or blur; chip click drops
// that value. Keeps the contract simple (no drag here either) and
// matches the picker pattern used elsewhere.
const enumInput = computed({
  get: () => '',
  set: (raw: string) => {
    const v = raw.trim();
    if (!v) return;
    const current = props.modelValue.enumValues ?? [];
    if (current.includes(v)) return;
    patch({ enumValues: [...current, v] });
  },
});

function removeEnumValue(value: string): void {
  const current = props.modelValue.enumValues ?? [];
  patch({ enumValues: current.filter((v) => v !== value) });
}

const numericFields = computed(
  () => props.modelValue.kind === 'number' || props.modelValue.kind === 'decimal',
);
const isEnumKind = computed(
  () => props.modelValue.kind === 'select' || props.modelValue.kind === 'multi_select',
);
const isText = computed(() => props.modelValue.kind === 'text');
const isRaw = computed(() => props.modelValue.kind === 'raw');
const isAssetRef = computed(() => props.modelValue.kind === 'asset');

// Pull the asset-kinds registry so the asset-ref scope dropdown
// can offer the actual slugs available. Cache-shared via the same
// useAssetKindsQuery the admin list + AssetView use, so opening
// the row is instant on a warm cache and an admin's edits to the
// registry land here without a manual refetch.
const { kinds: availableAssetKinds } = useAssetKindsQuery();

// Options for BaseDropdown — kind picker. Recomputes when locale
// changes so labels track the active language. The `raw` option
// only appears for rows currently of kind=raw (the row's kind
// dropdown is disabled in that case, so this just makes the
// current value visible rather than orphaned).
const kindOptions = computed<{ value: string; label: string }[]>(() => {
  const base: { value: string; label: string }[] = ATTRIBUTE_KINDS_ORDERED.map((k) => ({
    value: k,
    label: t(`asset-kind-attribute-kind-${k}`),
  }));
  if (props.modelValue.kind === 'raw') {
    base.push({ value: 'raw', label: t('asset-kind-attribute-kind-raw') });
  }
  return base;
});

// Asset-scope dropdown options: "Any kind" sentinel + every kind
// from the registry. Empty string maps to undefined on save.
const assetScopeOptions = computed(() => [
  { value: '', label: t('asset-kind-attribute-row-asset-scope-any') },
  ...availableAssetKinds.value.map((k) => ({
    value: k.slug,
    label: `${k.label} (${k.slug})`,
  })),
]);
</script>

<template>
  <div
    class="bg-surface border border-default rounded-lg p-3 flex flex-col gap-3"
    :class="{ 'border-status-warning/60': nameInvalid }"
  >
    <!-- Header row: drag-like up/down + remove on the right;
         name + kind dropdown + required toggle on the left. -->
    <div class="flex flex-wrap items-start gap-3">
      <div class="flex flex-col gap-1 w-32 shrink-0">
        <label class="text-xs font-medium text-secondary">
          {{ t('asset-kind-attribute-row-move') }}
        </label>
        <div class="flex items-center gap-1">
          <button
            type="button"
            class="p-1.5 rounded-md text-secondary hover:text-primary hover:bg-surface-hover transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
            :disabled="isFirst"
            :aria-label="t('asset-kind-attribute-row-move-up')"
            @click="emit('move-up')"
          >
            <Icon name="chevronUp" />
          </button>
          <button
            type="button"
            class="p-1.5 rounded-md text-secondary hover:text-primary hover:bg-surface-hover transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
            :disabled="isLast"
            :aria-label="t('asset-kind-attribute-row-move-down')"
            @click="emit('move-down')"
          >
            <Icon name="chevronDown" />
          </button>
          <button
            type="button"
            class="p-1.5 rounded-md text-secondary hover:text-status-error hover:bg-status-error/10 transition-colors ml-auto"
            :aria-label="t('asset-kind-attribute-row-remove')"
            @click="emit('remove')"
          >
            <Icon name="trash" />
          </button>
        </div>
      </div>

      <FormInput
        :model-value="modelValue.name"
        :label="t('asset-kind-attribute-row-name')"
        :placeholder="t('asset-kind-attribute-row-name-placeholder')"
        :hint="
          nameInvalid
            ? t('asset-kind-attribute-row-name-invalid')
            : t('asset-kind-attribute-row-name-hint')
        "
        class="flex-1 min-w-[160px]"
        size="sm"
        @update:model-value="(v) => patch({ name: v })"
      />

      <BaseDropdown
        :model-value="modelValue.kind"
        :options="kindOptions"
        :label="t('asset-kind-attribute-row-kind')"
        :disabled="isRaw"
        size="sm"
        class="w-44 shrink-0"
        @update:model-value="(v) => onKindChange(v as AttributeKind)"
      />

      <Checkbox
        class="self-end pb-1.5"
        :model-value="modelValue.required"
        :label="t('asset-kind-attribute-row-required')"
        @update:model-value="(v: boolean) => patch({ required: v })"
      />
    </div>

    <!-- `raw` kind: read-only JSON preview + hint. -->
    <div
      v-if="isRaw"
      class="text-xs text-tertiary p-2 bg-surface-alt border border-default rounded-md font-mono whitespace-pre-wrap"
    >
      <p class="text-status-warning mb-1 font-sans">
        {{ t('asset-kind-attribute-row-raw-warning') }}
      </p>
      {{ JSON.stringify(modelValue.raw, null, 2) }}
    </div>

    <!-- Optional description (all kinds except raw). -->
    <FormInput
      v-if="!isRaw"
      :model-value="modelValue.description ?? ''"
      :label="t('asset-kind-attribute-row-description')"
      :placeholder="t('asset-kind-attribute-row-description-placeholder')"
      @update:model-value="(v) => patch({ description: v || undefined })"
    />

    <!-- Text-only: maxLength + pattern. -->
    <div v-if="isText" class="grid grid-cols-1 sm:grid-cols-2 gap-3">
      <FormNumber
        :model-value="modelValue.maxLength ?? null"
        :label="t('asset-kind-attribute-row-max-length')"
        size="sm"
        integer
        :min="1"
        @update:model-value="(v) => patch({ maxLength: v ?? undefined })"
      />
      <FormInput
        :model-value="modelValue.pattern ?? ''"
        :label="t('asset-kind-attribute-row-pattern')"
        :placeholder="'^[A-Z0-9-]+$'"
        :hint="t('asset-kind-attribute-row-pattern-hint')"
        @update:model-value="(v) => patch({ pattern: v || undefined })"
      />
    </div>

    <!-- Numeric: minimum + maximum. Decimal kind allows fractions;
         integer kind enforces whole numbers via the `integer` prop. -->
    <div v-if="numericFields" class="grid grid-cols-1 sm:grid-cols-2 gap-3">
      <FormNumber
        :model-value="modelValue.minimum ?? null"
        :label="t('asset-kind-attribute-row-minimum')"
        size="sm"
        :integer="modelValue.kind === 'number'"
        @update:model-value="(v) => patch({ minimum: v ?? undefined })"
      />
      <FormNumber
        :model-value="modelValue.maximum ?? null"
        :label="t('asset-kind-attribute-row-maximum')"
        size="sm"
        :integer="modelValue.kind === 'number'"
        @update:model-value="(v) => patch({ maximum: v ?? undefined })"
      />
    </div>

    <!-- Asset reference: optional scope to a specific asset kind.
         Empty scope means the picker offers all assets across all
         kinds. Source list comes from the registry so admin renames
         flow through on next mount. -->
    <BaseDropdown
      v-if="isAssetRef"
      :model-value="modelValue.assetKindScope ?? ''"
      :options="assetScopeOptions"
      :label="t('asset-kind-attribute-row-asset-scope')"
      :description="t('asset-kind-attribute-row-asset-scope-hint')"
      size="sm"
      @update:model-value="(v) => patch({ assetKindScope: (v as string) || undefined })"
    />

    <!-- Select / Multi-select: enum value editor. -->
    <div v-if="isEnumKind" class="flex flex-col gap-2">
      <span class="text-sm font-medium text-primary">
        {{ t('asset-kind-attribute-row-enum-values') }}
      </span>
      <div class="flex flex-wrap items-center gap-2">
        <button
          v-for="value in modelValue.enumValues ?? []"
          :key="value"
          type="button"
          class="px-2 py-1 text-xs rounded-md bg-accent/10 text-accent border border-accent/30 flex items-center gap-1 hover:bg-accent/20 transition-colors"
          :aria-label="t('asset-kind-attribute-row-enum-remove', { value })"
          @click="removeEnumValue(value)"
        >
          <span>{{ value }}</span>
          <Icon name="close" class="h-3 w-3" />
        </button>
        <input
          v-model="enumInput"
          type="text"
          class="px-2 py-1 text-xs rounded-md bg-surface-alt border border-default text-primary placeholder-tertiary hover:border-strong focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent w-40"
          :placeholder="t('asset-kind-attribute-row-enum-add-placeholder')"
          @keydown.enter.prevent="(e) => { (e.target as HTMLInputElement).blur(); }"
        />
      </div>
      <p
        v-if="(modelValue.enumValues ?? []).length === 0"
        class="text-xs text-status-warning"
      >
        {{ t('asset-kind-attribute-row-enum-empty') }}
      </p>
    </div>
  </div>
</template>
