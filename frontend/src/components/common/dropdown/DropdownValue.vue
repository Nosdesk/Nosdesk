<!--
The trigger's displayed value for BaseDropdown: the selected option's
tone dots and the text, or the placeholder in tertiary. Shared by the
desktop (Reka Select) and phone (sheet) triggers so the two never drift.
-->
<script setup lang="ts">
import type { DropdownOption } from '@/components/common/dropdownOption';

defineProps<{
  text: string;
  hasSelection: boolean;
  option?: DropdownOption<string | number>;
}>();
</script>

<template>
  <span
    class="truncate flex items-center gap-2 min-w-0"
    :class="hasSelection ? 'text-primary' : 'text-tertiary'"
  >
    <span v-if="option?.tones?.length" aria-hidden="true" class="flex items-center gap-0.5 flex-shrink-0">
      <span
        v-for="(t, i) in option.tones"
        :key="i"
        :class="[t, 'rounded-full', option.tones.length === 1 ? 'w-2 h-2' : 'w-1 h-1']"
      />
    </span>
    <span class="truncate">{{ text }}</span>
  </span>
</template>
