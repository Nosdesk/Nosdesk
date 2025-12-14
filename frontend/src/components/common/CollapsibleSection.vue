<script setup lang="ts">
import { computed, ref } from 'vue'

const props = defineProps<{
  title: string
  isCollapsed: boolean
  accentColor?: string
}>()

const emit = defineEmits<{
  (e: 'toggle'): void
}>()

const accentColorStyle = computed(() => props.accentColor || '#2C80FF')

// Expose root element for parent components that need DOM access (e.g., resizable sections)
const rootRef = ref<HTMLElement | null>(null)
defineExpose({
  $el: rootRef
})
</script>

<template>
  <div
    ref="rootRef"
    class="flex flex-col overflow-hidden transition-opacity duration-200"
    :class="[isCollapsed ? 'opacity-90 hover:opacity-100' : '']"
  >
    <!-- Header -->
    <div
      class="flex items-center justify-between py-1.5 px-3 cursor-pointer transition-colors duration-200 group flex-shrink-0"
      :class="isCollapsed ? 'hover:bg-surface-hover' : 'bg-surface-hover/40'"
      @click="emit('toggle')"
    >
      <h3 class="text-xs font-medium text-secondary uppercase tracking-wider flex items-center gap-1">
        <span
          class="w-2 h-2 rounded-full"
          :style="{ backgroundColor: accentColorStyle }"
        ></span>
        {{ title }}
      </h3>
      <button
        class="text-tertiary group-hover:text-primary transition-colors duration-200 bg-surface-hover rounded p-0.5"
        :title="isCollapsed ? 'Expand section' : 'Collapse section'"
      >
        <svg
          xmlns="http://www.w3.org/2000/svg"
          class="h-3 w-3 transition-transform duration-200"
          :class="{ 'rotate-180': !isCollapsed }"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
        >
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M19 9l-7 7-7-7"
          />
        </svg>
      </button>
    </div>

    <!-- Content wrapper - parent controls height/flex behavior -->
    <div
      class="overflow-y-auto bg-surface/60 transition-opacity duration-200"
      :class="isCollapsed ? 'opacity-0 h-0' : 'opacity-100 flex-1'"
    >
      <slot v-if="!isCollapsed" />
    </div>
  </div>
</template>
