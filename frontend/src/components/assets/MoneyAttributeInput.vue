<script setup lang="ts">
/**
 * An amount of money for an attribute or a record field, drawn like the
 * record form's cost: the amount typed in the reader's locale, the
 * currency's code in the field.
 *
 * By default it reports on blur or Enter, and only an amount that reads, so
 * a page that saves on every change never sends half an amount, and the
 * server's canonical amount (`12.50` for `12.5`) never rewrites what is
 * being typed. `eager` reports each amount as it reads, for a form with a
 * Save button, which a tap on a phone may reach without a blur. Either way
 * `update:invalid` says when the text doesn't read, so a form can hold its
 * save.
 */
import { computed, onBeforeUnmount, ref, useId, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import {
  isStoredMoney,
  localAmount,
  moneyAmountOf,
  moneyErrorText,
  readMoneyInput,
  zeroAmount,
  type MoneyWrite,
} from '@/utils/moneyAttribute'

const props = defineProps<{
  modelValue: unknown
  /** The code the property pins its values to: sent with the amount. */
  pinned?: string | null
  /** The workspace currency: shown for a value that names none, and left
   *  for the server to fill, so a change of workspace currency is never
   *  written with the old one. */
  currency?: string | null
  minimum?: number
  maximum?: number
  /** Report each amount as it reads, for a form with a Save button. */
  eager?: boolean
  label?: string
  /** The input's id, for a label outside the component. */
  id?: string
  required?: boolean
  disabled?: boolean
}>()

const emit = defineEmits<{
  /** null: the field was emptied. */
  'update:modelValue': [value: MoneyWrite | null]
  'update:invalid': [invalid: boolean]
}>()

const fluent = useFluent()
const generatedId = useId()
const inputId = computed(() => props.id ?? generatedId)

/** The code the amount is sent in, when it names one. */
const sent = computed(() => props.pinned || moneyAmountOf(props.modelValue)?.currency || null)
/** The code shown and checked against. */
const shown = computed(() => sent.value ?? (props.currency || null))

const seeded = computed(() => {
  const money = moneyAmountOf(props.modelValue)
  return money ? localAmount(money.amount) : ''
})
const text = ref(seeded.value)
/** The text last reported or seeded, so an unchanged field reports nothing. */
let reported = seeded.value
const focused = ref(false)
/** Why the text doesn't read, once it is said. */
const error = ref<string | undefined>()
const invalid = ref(false)

watch(invalid, (next) => emit('update:invalid', next))
onBeforeUnmount(() => {
  if (invalid.value) emit('update:invalid', false)
})

watch(seeded, (next) => {
  if (focused.value) return
  text.value = next
  reported = next
  error.value = undefined
  invalid.value = false
})

/** A value that isn't money as the server stores it (a bare amount from
 *  before the field held money, or text) is written over on leaving the
 *  field, so the next save of the whole set doesn't fail on it. */
const needsRewrite = computed(() => props.modelValue != null && !isStoredMoney(props.modelValue))

/** Read the text and report it when it reads and is new; the error, if not. */
function read(force = false): string | undefined {
  const result = readMoneyInput(text.value, {
    currency: sent.value,
    checkCurrency: shown.value,
    minimum: props.minimum,
    maximum: props.maximum,
  })
  if (!result.ok) {
    invalid.value = true
    return moneyErrorText(fluent.$t, result.error, {
      currency: shown.value,
      minimum: props.minimum,
      maximum: props.maximum,
    })
  }
  invalid.value = false
  if (text.value !== reported || force) {
    reported = text.value
    emit('update:modelValue', result.value)
  }
  return undefined
}

function commit() {
  error.value = read(needsRewrite.value)
}

function onInput(event: Event) {
  text.value = (event.target as HTMLInputElement).value
  if (props.eager) error.value = read()
  else error.value = undefined
}

function onBlur() {
  focused.value = false
  // Reported with Enter and saved while focused: show the amount as the
  // server wrote it.
  if (!needsRewrite.value && text.value === reported && seeded.value !== reported) {
    text.value = seeded.value
    reported = seeded.value
    return
  }
  commit()
}

const placeholder = computed(() => zeroAmount(shown.value))
const describedBy = computed(
  () =>
    [shown.value ? `${inputId.value}-code` : '', error.value ? `${inputId.value}-error` : '']
      .filter(Boolean)
      .join(' ') || undefined,
)
</script>

<template>
  <div class="flex flex-col">
    <label
      v-if="label"
      :for="inputId"
      class="pb-1.5 text-xs font-medium text-tertiary uppercase tracking-wide"
    >
      {{ label }}<span v-if="required" class="text-status-error ml-0.5" aria-hidden="true">*</span>
    </label>
    <!-- The code sits in the field, so the currency is never a guess. -->
    <div class="relative">
      <input
        :id="inputId"
        :value="text"
        type="text"
        inputmode="decimal"
        :placeholder="placeholder"
        :required="required"
        :disabled="disabled"
        :aria-invalid="error ? 'true' : undefined"
        :aria-describedby="describedBy"
        class="w-full bg-surface-alt border rounded-lg text-primary placeholder-tertiary transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:border-accent disabled:opacity-50 disabled:cursor-not-allowed px-3 py-1.5 pr-12 text-sm pointer-coarse:text-base tabular-nums"
        :class="error ? 'border-status-error' : 'border-field'"
        @input="onInput"
        @focus="focused = true"
        @blur="onBlur"
        @keydown.enter="commit"
      />
      <span
        v-if="shown"
        :id="`${inputId}-code`"
        class="absolute inset-y-0 right-2.5 flex items-center text-2xs font-medium text-tertiary pointer-events-none"
        data-testid="money-currency"
      >
        {{ shown }}
      </span>
    </div>
    <!-- Always present, so a screen reader hears the error when it comes. -->
    <p
      :id="`${inputId}-error`"
      class="text-xs text-status-error"
      :class="error ? 'pt-1.5' : ''"
      aria-live="polite"
    >
      {{ error }}
    </p>
  </div>
</template>
