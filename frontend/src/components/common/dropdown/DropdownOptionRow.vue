<!--
The inside of one BaseDropdown option row: the multi-select checkbox or
the single-select gutter (check when selected, tone dots otherwise),
then the label and optional description. The parent supplies the row
element (a Reka SelectItem on desktop, a plain option button on phones).
-->
<script setup lang="ts">
import type { DropdownOption } from '@/components/common/dropdownOption';

defineProps<{
  option: DropdownOption<string | number>;
  multiple: boolean;
  checked: boolean;
}>();
</script>

<template>
  <template v-if="multiple">
    <span
      class="w-4 h-4 border rounded flex-shrink-0 flex items-center justify-center transition-colors"
      :class="checked ? 'bg-accent border-accent' : 'border-default'"
    >
      <svg v-if="checked" class="w-3 h-3 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="3" d="M5 13l4 4L19 7" />
      </svg>
    </span>
  </template>
  <!-- Single-select gutter: a fixed box keeps every label column
       aligned whatever the gutter shows. -->
  <span v-else class="w-4 h-4 flex items-center justify-center flex-shrink-0">
    <svg v-if="checked" class="w-4 h-4 text-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24">
      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
    </svg>
    <span v-else-if="option.tones?.length" aria-hidden="true" class="flex items-center gap-0.5">
      <span
        v-for="(t, i) in option.tones"
        :key="i"
        :class="[t, 'rounded-full', option.tones.length === 1 ? 'w-2 h-2' : 'w-1 h-1']"
      />
    </span>
  </span>

  <span class="flex-1 min-w-0">
    <span class="block" :class="checked ? 'font-medium' : ''">
      <slot name="label">{{ option.label }}</slot>
    </span>
    <span v-if="option.description" class="block text-xs text-tertiary mt-0.5 leading-snug">
      {{ option.description }}
    </span>
  </span>
</template>
