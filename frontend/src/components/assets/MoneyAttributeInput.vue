<script setup lang="ts">
/**
 * An amount of money for an attribute or a record field, drawn like the
 * record form's cost: the amount typed in the reader's locale, the
 * currency's code in the field. It reports on blur or Enter rather than on
 * each key, and only an amount that reads, so a form that saves on every
 * change never sends half an amount, and the server's canonical amount
 * (`12.50` for `12.5`) never rewrites what is being typed.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import FormInput from '@/components/common/FormInput.vue'
import {
  AMOUNT_ERROR_KEYS,
  localAmount,
  moneyValueOf,
  readMoneyInput,
  type MoneyWrite,
} from '@/utils/moneyAttribute'

const props = defineProps<{
  modelValue: unknown
  /** The code the amount is in, shown in the field and sent with it.
   *  Without one the server fills the workspace's. */
  currency?: string | null
  label?: string
  required?: boolean
  disabled?: boolean
}>()

/** null: the field was emptied. */
const emit = defineEmits<{ 'update:modelValue': [value: MoneyWrite | null] }>()

const fluent = useFluent()

const seeded = computed(() => {
  const money = moneyValueOf(props.modelValue)
  return money ? localAmount(money.amount) : ''
})
const text = ref(seeded.value)
/** The text last reported or seeded, so an unchanged field reports nothing. */
let reported = seeded.value
const focused = ref(false)
const error = ref<string | undefined>()

watch(seeded, (next) => {
  if (focused.value) return
  text.value = next
  reported = next
  error.value = undefined
})

function commit() {
  if (text.value === reported) {
    error.value = undefined
    return
  }
  const read = readMoneyInput(text.value, props.currency)
  if (!read.ok) {
    error.value = fluent.$t(AMOUNT_ERROR_KEYS[read.error])
    return
  }
  error.value = undefined
  reported = text.value
  emit('update:modelValue', read.value)
}

function onBlur() {
  focused.value = false
  // Reported with Enter and saved while focused: show the amount as the
  // server wrote it.
  if (text.value === reported && seeded.value !== reported) {
    text.value = seeded.value
    reported = seeded.value
    return
  }
  commit()
}
</script>

<template>
  <div class="relative">
    <FormInput
      v-model="text"
      size="sm"
      :label="label"
      :required="required"
      :disabled="disabled"
      :error="error"
      placeholder="0.00"
      inputmode="decimal"
      class="[&_input]:pr-12 [&_input]:tabular-nums"
      @focus="focused = true"
      @blur="onBlur"
      @keydown.enter="commit"
    />
    <!-- The code sits in the field, so the currency is never a guess. -->
    <span
      v-if="currency"
      class="absolute right-2.5 h-[2.125rem] flex items-center text-2xs font-medium text-tertiary pointer-events-none"
      :class="label ? 'top-[1.375rem]' : 'top-0'"
      data-testid="money-currency"
    >
      {{ currency }}
    </span>
  </div>
</template>
