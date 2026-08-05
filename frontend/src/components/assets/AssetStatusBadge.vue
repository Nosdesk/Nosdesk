<script setup lang="ts">
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import Icon from '@/components/common/Icon.vue';
import { assetStatusTextClass, metaForAssetStatus } from '@/utils/assetStatusMeta';

const props = withDefaults(
  defineProps<{
    status: string;
    showIcon?: boolean;
    size?: 'sm' | 'md';
    /**
     * `pill` is the standalone badge (detail panel, mobile card).
     * `plain` drops the background, border and padding down to a
     * tinted icon plus a plain label — for dense table rows, where a
     * filled 26px pill on every line carries far more visual weight
     * than the mostly-identical value deserves. Same rationale as
     * `WorkflowStateGlyph` in the tickets table: the icon shape, not
     * just the hue, is what carries the meaning.
     */
    variant?: 'pill' | 'plain';
  }>(),
  {
    showIcon: true,
    size: 'sm',
    variant: 'pill',
  },
);

const fluent = useFluent();
const meta = computed(() => metaForAssetStatus(props.status));
const label = computed(() => fluent.$t(meta.value.labelKey));
const textClass = computed(() => assetStatusTextClass(props.status));

const sizeClass = computed(() =>
  props.size === 'md' ? 'text-sm px-2.5 py-1 gap-1.5' : 'text-xs px-2 py-1 gap-1',
);
</script>

<template>
  <span
    v-if="variant === 'plain'"
    class="inline-flex items-center gap-1.5 whitespace-nowrap min-w-0"
    :title="label"
  >
    <Icon :name="meta.icon" size="xs" class="flex-shrink-0" :class="textClass" />
    <span class="text-xs text-secondary truncate">{{ label }}</span>
  </span>

  <span
    v-else
    class="inline-flex items-center rounded-full whitespace-nowrap border font-medium"
    :class="[meta.colorClass, sizeClass]"
  >
    <Icon v-if="showIcon" :name="meta.icon" size="xs" class="flex-shrink-0" />
    {{ label }}
  </span>
</template>
