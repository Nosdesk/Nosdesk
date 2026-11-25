<script setup lang="ts">
/**
 * The text size slider: six stops between a small A and a large one.
 *
 * Drawn at a fixed size on purpose (`type-fixed` on the two A's): it is the
 * scale's legend, and a ruler that grew with the thing it measures would say
 * nothing. Its touch height is 44px at every size.
 *
 * While the device sets the size the thumb is hollow: it shows where the
 * device has put the text, and it is still a slider. Moving it is how a person
 * takes the size over (see `TextSizeSetting.vue`).
 */
import { SliderRange, SliderRoot, SliderThumb, SliderTrack } from 'reka-ui'
import { TEXT_SIZE_STOPS } from '@/utils/textScale'

defineProps<{
  /** Index into `TEXT_SIZE_STOPS`. */
  modelValue: number
  /** The device is setting the size. */
  following: boolean
  label: string
  valueText: string
  /** The element that describes the size in force (the status sentence). */
  describedBy?: string
}>()

const emit = defineEmits<{ (e: 'update:modelValue', index: number): void }>()
</script>

<template>
  <div class="type-fixed flex items-center gap-3" data-text-size-slider>
    <span class="w-4 shrink-0 text-center text-xs-plus leading-none font-medium text-tertiary" aria-hidden="true">A</span>
    <SliderRoot
      class="relative flex h-11 min-w-0 flex-1 touch-none items-center select-none"
      :model-value="[modelValue]"
      :min="0"
      :max="TEXT_SIZE_STOPS.length - 1"
      :step="1"
      @update:model-value="(value) => value && emit('update:modelValue', value[0])"
    >
      <SliderTrack class="relative h-1 flex-1 rounded-full bg-surface-alt">
        <SliderRange class="absolute h-full rounded-full" :class="following ? 'bg-strong' : 'bg-accent'" />
        <span
          v-for="(stop, i) in TEXT_SIZE_STOPS"
          :key="stop"
          class="absolute top-1/2 h-2 w-0.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-strong"
          :style="{ left: `${(i / (TEXT_SIZE_STOPS.length - 1)) * 100}%` }"
          aria-hidden="true"
        />
      </SliderTrack>
      <SliderThumb
        class="block h-7 w-7 rounded-full border-2 shadow-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
        :class="following ? 'border-strong bg-surface' : 'border-accent bg-accent'"
        :aria-label="label"
        :aria-valuetext="valueText"
        :aria-describedby="describedBy"
      />
    </SliderRoot>
    <span class="w-5 shrink-0 text-center text-2xl leading-none font-medium text-tertiary" aria-hidden="true">A</span>
  </div>
</template>
