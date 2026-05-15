<script setup lang="ts">
/**
 * SidebarCard - Shared card shell for ticket sidebar items
 *
 * Wraps SectionCard so every ticket sidebar panel inherits the same
 * compact h-9 header pill as the dashboard widgets and other cards.
 * The `#header` slot collapses into the title row; the only piece of
 * chrome SidebarCard adds is a remove button in the header actions.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import SectionCard from '@/components/common/SectionCard.vue';
import Icon from '@/components/common/Icon.vue';

interface Props {
  /** Tooltip for the remove button */
  removeTitle?: string;
  /** Make the entire card clickable */
  clickable?: boolean;
  /** Disable the remove button */
  removeDisabled?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  removeTitle: '',
  clickable: false,
  removeDisabled: false,
});

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const resolvedRemoveTitle = computed(() => props.removeTitle || t('ticket-chip-sidebar-remove'));

defineEmits<{
  (e: 'remove'): void;
  (e: 'click'): void;
}>();
</script>

<template>
  <!-- Screen layout -->
  <div
    class="print:hidden"
    :class="{ 'cursor-pointer': clickable }"
    @click="clickable && $emit('click')"
  >
    <SectionCard content-padding="p-4">
      <template #title>
        <div class="flex items-center gap-2 min-w-0">
          <slot name="header" />
        </div>
      </template>
      <template #headerActions>
        <button
          @click.stop="$emit('remove')"
          :disabled="removeDisabled"
          class="p-1 flex-shrink-0 text-tertiary hover:text-status-error hover:bg-status-error/20 rounded transition-colors disabled:opacity-50"
          :title="resolvedRemoveTitle"
        >
          <Icon name="close" :label="resolvedRemoveTitle" />
        </button>
      </template>

      <slot />
    </SectionCard>
  </div>

  <!-- Print layout -->
  <slot name="print" />
</template>
