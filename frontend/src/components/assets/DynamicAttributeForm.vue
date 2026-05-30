<script setup lang="ts">
import { computed } from 'vue';
import UserAttributePicker from '@/components/assets/UserAttributePicker.vue';

/**
 * Render form inputs for a constrained JSON Schema (the subset
 * defined in `backend/src/services/assets/kinds.rs`). Supported
 * property types map to inputs as follows:
 *
 * - string + format user-uuid                       -> user picker
 * - string + format date / date-time / email / uri  -> typed input
 * - string + enum                                   -> select
 * - string                                          -> text input
 * - number / integer                                -> number input
 * - boolean                                         -> checkbox
 * - array (with items)                              -> comma-separated text
 *
 * Anything outside the subset (which the backend validator would
 * reject anyway) falls through to a plain text input.
 */

type SchemaProperty = {
  type?: string;
  enum?: unknown[];
  title?: string;
  description?: string;
  format?: string;
  minLength?: number;
  maxLength?: number;
  minimum?: number;
  maximum?: number;
  multipleOf?: number;
  pattern?: string;
  items?: SchemaProperty;
  default?: unknown;
};

type Schema = {
  type?: string;
  properties?: Record<string, SchemaProperty>;
  required?: string[];
};

const props = defineProps<{
  schema: Schema | null;
  modelValue: Record<string, unknown>;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', value: Record<string, unknown>): void;
}>();

const properties = computed(() =>
  props.schema?.properties ? Object.entries(props.schema.properties) : [],
);

const requiredSet = computed(() => new Set(props.schema?.required ?? []));

function inputTypeFor(prop: SchemaProperty): 'text' | 'number' | 'date' | 'datetime-local' | 'email' | 'url' {
  if (prop.type === 'integer' || prop.type === 'number') return 'number';
  if (prop.format === 'date') return 'date';
  if (prop.format === 'date-time') return 'datetime-local';
  if (prop.format === 'email') return 'email';
  if (prop.format === 'uri') return 'url';
  return 'text';
}

function updateField(key: string, raw: unknown) {
  const next = { ...props.modelValue, [key]: raw };
  emit('update:modelValue', next);
}

/**
 * Number inputs come back as strings from the DOM; coerce while
 * keeping empty strings as null (so a cleared field doesn't
 * silently become 0 and pass minimum=1 validation).
 */
function onNumberInput(key: string, value: string, prop: SchemaProperty) {
  const trimmed = value.trim();
  if (trimmed === '') {
    const next = { ...props.modelValue };
    delete next[key];
    emit('update:modelValue', next);
    return;
  }
  const parsed = prop.type === 'integer' ? parseInt(trimmed, 10) : Number(trimmed);
  if (Number.isNaN(parsed)) return;
  updateField(key, parsed);
}

function onArrayInput(key: string, value: string, items: SchemaProperty | undefined) {
  const trimmed = value.trim();
  if (trimmed === '') {
    const next = { ...props.modelValue };
    delete next[key];
    emit('update:modelValue', next);
    return;
  }
  const parts = trimmed.split(',').map((s) => s.trim()).filter((s) => s.length > 0);
  const itemType = items?.type;
  const coerced = parts.map((p) => {
    if (itemType === 'integer') return parseInt(p, 10);
    if (itemType === 'number') return Number(p);
    if (itemType === 'boolean') return p === 'true';
    return p;
  });
  updateField(key, coerced);
}

function stringValue(key: string): string {
  const v = props.modelValue[key];
  if (v === null || v === undefined) return '';
  if (Array.isArray(v)) return v.join(', ');
  return String(v);
}

function boolValue(key: string): boolean {
  return Boolean(props.modelValue[key]);
}
</script>

<template>
  <div v-if="properties.length > 0" class="flex flex-col gap-3">
    <div
      v-for="[key, prop] in properties"
      :key="key"
      class="flex flex-col gap-1"
    >
      <label class="text-xs font-medium text-secondary uppercase tracking-wide">
        {{ prop.title || key }}
        <span v-if="requiredSet.has(key)" class="text-status-error">*</span>
      </label>
      <p v-if="prop.description" class="text-xs text-tertiary">{{ prop.description }}</p>

      <!-- user reference -> user picker. Must come before the
           enum / plain-string cases so format-driven rendering
           wins. -->
      <UserAttributePicker
        v-if="prop.format === 'user-uuid'"
        :model-value="stringValue(key)"
        :disabled="disabled"
        @update:model-value="(v) => updateField(key, v)"
      />

      <!-- enum -> select -->
      <select
        v-else-if="Array.isArray(prop.enum) && prop.enum.length > 0"
        :disabled="disabled"
        :value="stringValue(key)"
        class="bg-surface-alt rounded-lg border border-default px-3 py-2 text-primary text-sm"
        @change="(e) => updateField(key, (e.target as HTMLSelectElement).value)"
      >
        <option value="">--</option>
        <option v-for="opt in prop.enum" :key="String(opt)" :value="String(opt)">
          {{ String(opt) }}
        </option>
      </select>

      <!-- boolean -> checkbox -->
      <label
        v-else-if="prop.type === 'boolean'"
        class="inline-flex items-center gap-2 text-sm text-primary"
      >
        <input
          type="checkbox"
          :disabled="disabled"
          :checked="boolValue(key)"
          @change="(e) => updateField(key, (e.target as HTMLInputElement).checked)"
        />
        <span>{{ prop.title || key }}</span>
      </label>

      <!-- array -> comma-separated text -->
      <input
        v-else-if="prop.type === 'array'"
        type="text"
        :disabled="disabled"
        :value="stringValue(key)"
        placeholder="value1, value2, ..."
        class="bg-surface-alt rounded-lg border border-default px-3 py-2 text-primary text-sm"
        @input="(e) => onArrayInput(key, (e.target as HTMLInputElement).value, prop.items)"
      />

      <!-- number / integer -->
      <input
        v-else-if="prop.type === 'integer' || prop.type === 'number'"
        type="number"
        :disabled="disabled"
        :value="stringValue(key)"
        :min="prop.minimum"
        :max="prop.maximum"
        :step="prop.multipleOf ?? (prop.type === 'integer' ? 1 : 'any')"
        class="bg-surface-alt rounded-lg border border-default px-3 py-2 text-primary text-sm"
        @input="(e) => onNumberInput(key, (e.target as HTMLInputElement).value, prop)"
      />

      <!-- string with format / default text -->
      <input
        v-else
        :type="inputTypeFor(prop)"
        :disabled="disabled"
        :value="stringValue(key)"
        :pattern="prop.pattern"
        :minlength="prop.minLength"
        :maxlength="prop.maxLength"
        class="bg-surface-alt rounded-lg border border-default px-3 py-2 text-primary text-sm"
        @input="(e) => updateField(key, (e.target as HTMLInputElement).value)"
      />
    </div>
  </div>
</template>
