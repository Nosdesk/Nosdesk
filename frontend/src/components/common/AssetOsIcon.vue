<!-- OS badge for asset rows. Draws the platform marks from the central
     icon registry (the same set the sessions list uses) rather than
     keeping a private copy of the SVGs. -->
<script setup lang="ts">
import { computed } from 'vue';
import Icon from '@/components/common/Icon.vue';
import type { IconName } from '@/components/common/icons';

const props = defineProps<{
  os?: string | null;
}>();

const isOs = (os: string | null | undefined, ...names: string[]) =>
  os ? names.some(n => os.toLowerCase().includes(n)) : false;

const iconName = computed<IconName>(() => {
  if (isOs(props.os, 'windows')) return 'windows';
  if (isOs(props.os, 'mac', 'ios')) return 'apple';
  if (isOs(props.os, 'android')) return 'android';
  if (isOs(props.os, 'linux')) return 'linux';
  return 'device';
});
</script>

<template>
  <div class="w-8 h-8 rounded-lg bg-surface-alt flex items-center justify-center flex-shrink-0">
    <Icon :name="iconName" size="sm" class="text-tertiary" />
  </div>
</template>
