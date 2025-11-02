<script setup lang="ts">
import { defineProps, computed } from "vue";
import type { Page } from "@/services/documentationService";

// Define props for the component
const props = defineProps<{
  page: Page;
  level: number;
}>();

// Create safe computed properties to handle potential data issues
const safeTitle = computed(() => {
  return props.page?.title || "Untitled Page";
});

const safeDescription = computed(() => {
  return props.page?.description || "";
});

const safeIcon = computed(() => {
  return props.page?.icon || "📄";
});
</script>

<template>
  <!-- Single page item -->
  <div class="flex items-center gap-3">
    <!-- Indentation spacer -->
    <div
      v-if="level > 0"
      :style="{ width: `${level * 24}px` }"
      class="flex-shrink-0"
    ></div>

    <!-- Icon -->
    <div
      class="flex-shrink-0 text-center"
      :class="[
        level === 0 ? 'text-2xl text-brand-blue' : 'text-base text-tertiary',
      ]"
    >
      {{ safeIcon }}
    </div>

    <!-- Title and description -->
    <div class="flex-1 min-w-0">
      <div class="flex items-center gap-2">
        <span
          :class="[
            'transition-colors group-hover:text-brand-blue',
            level === 0 ? 'text-primary font-semibold text-xl' : 'text-secondary',
          ]"
        >
          {{ safeTitle }}
        </span>
      </div>
      <div v-if="safeDescription" class="text-tertiary text-xs mt-1">
        {{ safeDescription }}
      </div>
    </div>
  </div>
</template>

<style scoped>
.doc-toc-item {
  transition: all 0.2s ease;
}
</style>
