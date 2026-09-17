<!--
Segmented-pill control: a small group of mutually-exclusive options
rendered as connected pills, for choosing one value from a short set
(group-by axis, view mode, etc.). The on-brand alternative to a
dropdown when there are only a handful of options.

Exactly one option is always selected, so this is a radio group, on
Reka's RadioGroup: `role=radiogroup` with `role=radio` items, roving
focus with arrows selecting as they move (like native radios), looping
at the ends, and a hidden input for forms when `name` is given. Visual
matches the pill tab bars: a `bg-surface-alt` track with the active
pill on `bg-surface shadow-sm`.

  <SegmentedControl v-model="axis" :options="axisOptions" aria-label="Group by" />
-->
<script setup lang="ts" generic="T extends string">
import { RadioGroupItem, RadioGroupRoot } from 'reka-ui'

const props = withDefaults(
  defineProps<{
    modelValue: T
    options: { value: T; label: string }[]
    /** Accessible name for the group (e.g. "Group by"). */
    ariaLabel?: string
    /** `sm` tightens the pill height for dense toolbars. */
    size?: 'sm' | 'md'
    name?: string
  }>(),
  { ariaLabel: undefined, size: 'md' },
)

const emit = defineEmits<{
  (e: 'update:modelValue', value: T): void
}>()

function onUpdate(value: unknown): void {
  if (value !== props.modelValue) emit('update:modelValue', value as T)
}
</script>

<template>
  <RadioGroupRoot
    :model-value="modelValue"
    :aria-label="ariaLabel"
    :name="name"
    orientation="horizontal"
    loop
    class="inline-flex items-center gap-0.5 rounded-md bg-surface-alt p-0.5"
    @update:model-value="onUpdate"
  >
    <RadioGroupItem v-for="opt in options" :key="opt.value" :value="opt.value" as-child>
      <button
        type="button"
        class="inline-flex items-center justify-center rounded font-medium whitespace-nowrap shrink-0 transition-colors focus:outline-none focus-visible:ring-1 focus-visible:ring-accent text-secondary hover:text-primary hover:bg-surface/60 data-[state=checked]:bg-surface data-[state=checked]:text-primary data-[state=checked]:shadow-sm"
        :class="size === 'sm' ? 'h-6 px-2 text-xs' : 'h-7 px-2.5 text-sm'"
      >
        {{ opt.label }}
      </button>
    </RadioGroupItem>
  </RadioGroupRoot>
</template>
