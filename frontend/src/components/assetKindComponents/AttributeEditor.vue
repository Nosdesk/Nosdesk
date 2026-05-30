<script setup lang="ts">
/**
 * Typed schema builder for an asset kind. v-model'd against the
 * raw JSON Schema (the format the backend stores) so the parent
 * can save it as-is; round-trips through `parseSchema` /
 * `serializeSchema` in `attributeSchema.ts`.
 *
 * The parent (AssetKindEditView) keeps the JSON-textarea path
 * available as an escape hatch via its "View JSON" toggle so an
 * admin who wants to hand-edit can; the editor here is the
 * default surface for the common case (Text / Number / Date /
 * Boolean / Select / Multi-select / Email / URL).
 *
 * Reference attribute types (User, Asset) are scaffolded by the
 * backend validator already (commit 20eba42f) but the builder
 * doesn't expose them yet; that lands in commits 5 and 6.
 */
import { computed, ref } from 'vue';
import { useFluent } from 'fluent-vue';

import Button from '@/components/common/Button.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import AttributeRow from './AttributeRow.vue';
import {
  ATTRIBUTE_KINDS_ORDERED,
  blankAttribute,
  parseSchema,
  serializeSchema,
  type AttributeDef,
  type AttributeKind,
  type ParsedSchema,
} from './attributeSchema';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const props = defineProps<{
  /** The stored JSON schema (subset). v-model'd; emits a fresh
   * object on every builder edit. */
  modelValue: Record<string, unknown>;
}>();

const emit = defineEmits<{
  'update:modelValue': [value: Record<string, unknown>];
}>();

// Local state: parsed lazily from the prop on every read so a
// reset-to-cached behaves correctly. The parse error is derived
// as a separate computed against the same input rather than set
// as a side effect of `parsed` (which would violate Vue's pure-
// computed contract and the eslint vue/no-side-effects-in-
// computed-properties rule).
const parsed = computed<ParsedSchema>(() => {
  try {
    return parseSchema(props.modelValue);
  } catch {
    return { defs: [], extras: {} };
  }
});
const parseError = computed<string>(() => {
  try {
    parseSchema(props.modelValue);
    return '';
  } catch (e) {
    return e instanceof Error ? e.message : String(e);
  }
});

function pushUp(next: AttributeDef[]): void {
  emit(
    'update:modelValue',
    serializeSchema({ defs: next, extras: parsed.value.extras }),
  );
}

function updateAt(index: number, def: AttributeDef): void {
  const next = parsed.value.defs.slice();
  next[index] = def;
  pushUp(next);
}

function removeAt(index: number): void {
  const next = parsed.value.defs.slice();
  next.splice(index, 1);
  pushUp(next);
}

function moveUp(index: number): void {
  if (index <= 0) return;
  const next = parsed.value.defs.slice();
  [next[index - 1], next[index]] = [next[index], next[index - 1]];
  pushUp(next);
}

function moveDown(index: number): void {
  if (index >= parsed.value.defs.length - 1) return;
  const next = parsed.value.defs.slice();
  [next[index + 1], next[index]] = [next[index], next[index + 1]];
  pushUp(next);
}

const showAddMenu = ref(false);
function addAttribute(kind: Exclude<AttributeKind, 'raw'>): void {
  showAddMenu.value = false;
  const next = parsed.value.defs.slice();
  next.push(blankAttribute(kind));
  pushUp(next);
}
</script>

<template>
  <div class="flex flex-col gap-3">
    <AlertMessage
      v-if="parseError"
      type="error"
      :message="t('asset-kind-attribute-editor-parse-error', { error: parseError })"
    />

    <EmptyState
      v-if="!parseError && parsed.defs.length === 0"
      icon="document"
      :title="t('asset-kind-attribute-editor-empty-title')"
      :description="t('asset-kind-attribute-editor-empty-description')"
      variant="card"
    />

    <div v-if="parsed.defs.length > 0" class="flex flex-col gap-3">
      <AttributeRow
        v-for="(def, index) in parsed.defs"
        :key="`${index}-${def.kind}`"
        :model-value="def"
        :is-first="index === 0"
        :is-last="index === parsed.defs.length - 1"
        @update:model-value="(d) => updateAt(index, d)"
        @remove="removeAt(index)"
        @move-up="moveUp(index)"
        @move-down="moveDown(index)"
      />
    </div>

    <!-- "Add attribute" with a typed dropdown so the admin sees
         what's possible without having to pick "Text" and then
         change the type. Click outside / re-click trigger to close.
         For accessibility the menu is a real <details> so keyboard
         users get the disclosure behaviour for free. -->
    <details
      class="self-start group"
      :open="showAddMenu"
      @toggle="(e) => (showAddMenu = (e.target as HTMLDetailsElement).open)"
    >
      <summary class="list-none cursor-pointer">
        <Button variant="secondary" icon="add" type="button">
          {{ t('asset-kind-attribute-editor-add') }}
        </Button>
      </summary>
      <div class="mt-2 flex flex-wrap gap-1 p-2 bg-surface border border-default rounded-lg">
        <button
          v-for="k in ATTRIBUTE_KINDS_ORDERED"
          :key="k"
          type="button"
          class="px-2 py-1 text-xs rounded-md bg-surface-alt text-secondary hover:text-primary hover:bg-surface-hover border border-default transition-colors"
          @click="addAttribute(k)"
        >
          {{ t(`asset-kind-attribute-kind-${k}`) }}
        </button>
      </div>
    </details>
  </div>
</template>

<style scoped>
/* Hide the default disclosure triangle on the <summary> wrapper
   so the button inside stays the only visible affordance. */
summary::-webkit-details-marker {
  display: none;
}
summary::marker {
  display: none;
}
</style>
