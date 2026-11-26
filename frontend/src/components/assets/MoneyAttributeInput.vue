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
 * `update:invalid` follows each key, so a form can hold its save, while the
 * reason shows only on blur, Enter or `showErrors` (a save was tried), not
 * halfway through typing `1,250`.
 *
 * It reports only what someone typed. A stored value that isn't an amount
 * (`N/A` from before the field held money) shows as it is, with its error,
 * and stays until someone changes it.
 */
import { computed, onBeforeUnmount, ref, useId, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import {
  localAmount,
  moneyAmountOf,
  moneyErrorText,
  readMoneyInput,
  zeroAmount,
  type MoneyInput,
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
  /** A save was tried: show why the amount doesn't read, if it doesn't. */
  showErrors?: boolean
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

/** The stored value as text: an amount in the reader's locale, or text
 *  that was never one, as it is. */
const seeded = computed(() => {
  const money = moneyAmountOf(props.modelValue)
  if (money) return localAmount(money.amount)
  return typeof props.modelValue === 'string' ? props.modelValue : ''
})
const text = ref(seeded.value)
/** The text last reported or seeded, so an unchanged field reports nothing. */
let reported = seeded.value
const focused = ref(false)
/** Why the text doesn't read, kept current with each key. */
const problem = ref<string | undefined>()
/** The reason is shown: after a blur, Enter or a stored value. */
const revealed = ref(true)
const error = computed(() => (revealed.value || props.showErrors ? problem.value : undefined))
const invalid = ref(false)

watch(invalid, (next) => emit('update:invalid', next))
onBeforeUnmount(() => {
  if (invalid.value) emit('update:invalid', false)
})

/** Read the text, keeping `invalid` and the reason current. */
function check(): MoneyInput {
  const result = readMoneyInput(text.value, {
    currency: sent.value,
    checkCurrency: shown.value,
    minimum: props.minimum,
    maximum: props.maximum,
  })
  invalid.value = !result.ok
  problem.value = result.ok
    ? undefined
    : moneyErrorText(fluent.$t, result.error, {
        currency: shown.value,
        minimum: props.minimum,
        maximum: props.maximum,
      })
  return result
}

/** Report what was typed, when it reads and isn't what was last reported. */
function report(result: MoneyInput) {
  if (!result.ok || text.value === reported) return
  reported = text.value
  emit('update:modelValue', result.value)
}

function seed(next: string) {
  text.value = next
  reported = next
  revealed.value = true
  check()
}
seed(seeded.value)

watch(seeded, (next) => {
  if (!focused.value) seed(next)
})

function commit() {
  revealed.value = true
  report(check())
}

function onInput(event: Event) {
  text.value = (event.target as HTMLInputElement).value
  revealed.value = false
  const result = check()
  if (props.eager) report(result)
}

function onBlur() {
  focused.value = false
  // Reported with Enter and saved while focused: show the amount as the
  // server wrote it.
  if (text.value === reported && seeded.value !== reported) {
    seed(seeded.value)
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
