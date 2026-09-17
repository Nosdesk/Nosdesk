<!--
One-time-code entry on Reka's PinInput: one real input per digit inside a
named group, so each box is reachable, announced ("Digit 2 of 6") and
carries `autocomplete=one-time-code` for the SMS suggestion. Typing
advances, Backspace retreats, arrows and Home/End move, a paste or an
autofill of the whole code spreads across the boxes, and `complete`
fires once the last digit lands. Digits only (`type=number`).

The model stays the plain string the callers submit; a gap left by
deleting a middle digit closes up, which is the linear entry a code
wants anyway.
-->
<template>
  <PinInputRoot
    :model-value="digits"
    type="number"
    otp
    role="group"
    :aria-label="ariaLabel"
    class="flex w-full justify-center gap-1.5 sm:gap-2"
    @update:model-value="onUpdate"
    @complete="onComplete"
  >
    <PinInputInput v-for="(_, i) in length" :key="i" :index="i" as-child>
      <input
        :aria-label="fluent.$t('otp-digit-aria', { index: i + 1, count: length })"
        class="flex-1 min-w-0 max-w-[3.25rem] h-12 sm:h-14 bg-surface-alt border border-subtle rounded-lg text-center text-primary text-lg sm:text-xl font-mono transition-colors focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent/50 [&:not(:placeholder-shown)]:border-strong caret-accent"
        placeholder=" "
      />
    </PinInputInput>
  </PinInputRoot>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { PinInputInput, PinInputRoot } from 'reka-ui';
import { useFluent } from 'fluent-vue';

const props = withDefaults(defineProps<{
  modelValue: string;
  length?: number;
  ariaLabel?: string;
}>(), {
  length: 6,
  ariaLabel: undefined,
});

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
  (e: 'complete', value: string): void;
}>();

const fluent = useFluent();

const digits = computed<number[]>(() => props.modelValue.split('').map(Number));

function join(values: Array<number | undefined | null>): string {
  return values
    .filter((v): v is number => typeof v === 'number' && !Number.isNaN(v))
    .join('')
    .slice(0, props.length);
}

function onUpdate(values: number[]) {
  const next = join(values);
  if (next !== props.modelValue) emit('update:modelValue', next);
}

function onComplete(values: number[]) {
  emit('complete', join(values));
}
</script>
