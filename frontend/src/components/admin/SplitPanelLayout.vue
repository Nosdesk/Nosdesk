<script setup lang="ts">
import { computed } from 'vue';
import { useMobileDetection } from '@/composables/useMobileDetection';

const props = withDefaults(defineProps<{
  /** Whether the detail panel is open */
  panelOpen: boolean;
  /** Width of the list when the panel is open (any CSS value) */
  listWidth?: string;
}>(), {
  listWidth: '40%'
});

const { isMobile } = useMobileDetection('xl');

/** On mobile, the panel never shows inline — the parent should navigate or use a modal instead */
const showPanel = computed(() => !isMobile.value && props.panelOpen);
</script>

<template>
  <div class="flex h-full">
    <!-- Left: list area -->
    <div
      class="min-w-0 overflow-y-auto transition-[flex] duration-200 ease-in-out"
      :style="showPanel
        ? { flex: `0 0 ${listWidth}` }
        : { flex: '1 1 0%' }"
      :class="{ 'border-r border-default': showPanel }"
    >
      <slot name="list" :is-mobile="isMobile" />
    </div>

    <!-- Right: detail panel (desktop only) -->
    <Transition name="panel-slide">
      <div v-if="showPanel" class="flex-1 min-w-0 flex">
        <slot name="panel" />
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.panel-slide-enter-active,
.panel-slide-leave-active {
  transition: opacity 200ms ease, transform 200ms ease;
}
.panel-slide-enter-from,
.panel-slide-leave-to {
  opacity: 0;
  transform: translateX(24px);
}
</style>
