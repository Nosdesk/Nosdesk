<script setup lang="ts">
/**
 * A record type's own fields, from its `field_schema` in `field_order`,
 * as cells of the record form's two-column grid. Short values (a number,
 * a date, a yes or no) take one column; text and lists take both. Same
 * mapping as `DynamicAttributeForm`, drawn with the form's own inputs.
 */
import { computed, reactive } from 'vue'
import { useFluent } from 'fluent-vue'
import Checkbox from '@/components/common/Checkbox.vue'
import DatePicker from '@/components/common/DatePicker.vue'
import FormInput from '@/components/common/FormInput.vue'
import FormNumber from '@/components/common/FormNumber.vue'
import MoneyAttributeInput from '@/components/assets/MoneyAttributeInput.vue'
import { isMoneyProp } from '@/utils/moneyAttribute'
import type { AssetRecordType } from '@nosdesk/core/types/asset'
import { fieldProps, fieldTitle } from './assetTimeline'

const props = defineProps<{
  type: AssetRecordType
  modelValue: Record<string, unknown>
  /** The workspace currency, for a money field that pins none and has no
   *  value yet. */
  currency?: string | null
}>()

const emit = defineEmits<{
  'update:modelValue': [value: Record<string, unknown>]
  /** A money field holds text that doesn't read: hold the save. */
  'update:invalid': [invalid: boolean]
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const fields = computed(() =>
  fieldProps(props.type).map(([key, prop]) => ({ key, prop, title: fieldTitle(t, props.type, key, prop) })),
)

const invalidKeys = reactive(new Set<string>())
function onMoneyInvalid(key: string, invalid: boolean) {
  if (invalid) invalidKeys.add(key)
  else invalidKeys.delete(key)
  emit('update:invalid', invalidKeys.size > 0)
}

function set(key: string, value: unknown) {
  const next = { ...props.modelValue }
  if (value === '' || value === null || value === undefined || (Array.isArray(value) && value.length === 0)) {
    delete next[key]
  } else {
    next[key] = value
  }
  emit('update:modelValue', next)
}

function text(key: string): string {
  const value = props.modelValue[key]
  if (Array.isArray(value)) return value.join(', ')
  return value === undefined || value === null ? '' : String(value)
}

function numberOf(key: string): number | null {
  const value = props.modelValue[key]
  return typeof value === 'number' ? value : null
}

function setList(key: string, raw: string) {
  set(
    key,
    raw
      .split(',')
      .map((part) => part.trim())
      .filter(Boolean),
  )
}
</script>

<template>
  <template v-for="field in fields" :key="field.key">
    <div v-if="field.prop.type === 'boolean'" class="col-span-2 flex items-center min-h-8">
      <Checkbox
        :model-value="modelValue[field.key] === true"
        size="sm"
        :label="field.title"
        @update:model-value="(v: boolean) => set(field.key, v || undefined)"
      />
    </div>
    <DatePicker
      v-else-if="field.prop.format === 'date'"
      :model-value="text(field.key)"
      :label="field.title"
      block
      @update:model-value="(v: string) => set(field.key, v)"
    />
    <MoneyAttributeInput
      v-else-if="isMoneyProp(field.prop)"
      :model-value="modelValue[field.key]"
      :label="field.title"
      :pinned="field.prop.currency"
      :currency="currency"
      :minimum="field.prop.minimum"
      :maximum="field.prop.maximum"
      eager
      @update:model-value="(v) => set(field.key, v)"
      @update:invalid="(v: boolean) => onMoneyInvalid(field.key, v)"
    />
    <FormNumber
      v-else-if="field.prop.type === 'number' || field.prop.type === 'integer'"
      :model-value="numberOf(field.key)"
      :label="field.title"
      size="sm"
      :integer="field.prop.type === 'integer'"
      :step-snapping="false"
      :min="0"
      @update:model-value="(v: number | null) => set(field.key, v)"
    />
    <FormInput
      v-else-if="field.prop.type === 'array'"
      class="col-span-2"
      size="sm"
      :model-value="text(field.key)"
      :label="field.title"
      :description="t('asset-record-form-list-hint')"
      @update:model-value="(v: string) => setList(field.key, v)"
    />
    <FormInput
      v-else
      class="col-span-2"
      size="sm"
      :model-value="text(field.key)"
      :label="field.title"
      @update:model-value="(v: string) => set(field.key, v)"
    />
  </template>
</template>
