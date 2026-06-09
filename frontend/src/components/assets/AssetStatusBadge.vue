<script setup lang="ts">
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import Icon from '@/components/common/Icon.vue';
import { metaForAssetStatus } from '@/utils/assetStatusMeta';

const props = withDefaults(
  defineProps<{
    status: string;
    showIcon?: boolean;
    size?: 'sm' | 'md';
  }>(),
  {
    showIcon: true,
    size: 'sm',
  },
);

const fluent = useFluent();
const meta = computed(() => metaForAssetStatus(props.status));
const label = computed(() => fluent.$t(meta.value.labelKey));

const sizeClass = computed(() =>
  props.size === 'md' ? 'text-sm px-2.5 py-1 gap-1.5' : 'text-xs px-2 py-1 gap-1',
);
</script>

<template>
  <span
    class="inline-flex items-center rounded-full whitespace-nowrap border font-medium"
    :class="[meta.colorClass, sizeClass]"
  >
    <Icon v-if="showIcon" :name="meta.icon" size="xs" class="flex-shrink-0" />
    {{ label }}
  </span>
</template>
