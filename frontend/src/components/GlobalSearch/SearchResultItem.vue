<script setup lang="ts">
import { computed } from 'vue';
import type { SearchResult, SearchEntityType } from '@/types/search';
import { ENTITY_TYPE_CONFIG } from '@/types/search';

const props = defineProps<{
  result: SearchResult;
  isSelected: boolean;
}>();

const emit = defineEmits<{
  select: [result: SearchResult];
}>();

// Icon path from centralized config
const iconPath = computed(() => {
  return ENTITY_TYPE_CONFIG[props.result.entity_type]?.icon ?? 'M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z';
});

// Background and text color classes using the design system
const iconClasses = computed(() => {
  const styles: Record<SearchEntityType, { bg: string; text: string; ring: string }> = {
    ticket: {
      bg: 'bg-status-info-muted',
      text: 'text-status-info',
      ring: 'ring-status-info/20'
    },
    comment: {
      bg: 'bg-status-success-muted',
      text: 'text-status-success',
      ring: 'ring-status-success/20'
    },
    documentation: {
      bg: 'bg-[rgba(139,92,246,0.15)]',
      text: 'text-brand-purple',
      ring: 'ring-brand-purple/20'
    },
    attachment: {
      bg: 'bg-status-warning-muted',
      text: 'text-status-warning',
      ring: 'ring-status-warning/20'
    },
    device: {
      bg: 'bg-[rgba(44,128,255,0.15)]',
      text: 'text-brand-blue',
      ring: 'ring-brand-blue/20'
    },
    user: {
      bg: 'bg-[rgba(255,102,179,0.15)]',
      text: 'text-brand-pink',
      ring: 'ring-brand-pink/20'
    },
  };
  return styles[props.result.entity_type] || { bg: 'bg-surface-alt', text: 'text-tertiary', ring: 'ring-default' };
});

// Format relative time if available
const formattedTime = computed(() => {
  if (!props.result.updated_at) return null;
  const date = new Date(props.result.updated_at);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));

  if (diffDays === 0) return 'Today';
  if (diffDays === 1) return 'Yesterday';
  if (diffDays < 7) return `${diffDays}d ago`;
  if (diffDays < 30) return `${Math.floor(diffDays / 7)}w ago`;
  if (diffDays < 365) return `${Math.floor(diffDays / 30)}mo ago`;
  return `${Math.floor(diffDays / 365)}y ago`;
});
</script>

<template>
  <button
    type="button"
    :data-selected="isSelected"
    @click="emit('select', result)"
    :class="[
      'w-full px-3 py-2.5 flex items-center gap-3 text-left transition-all duration-150 group',
      'focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset',
      isSelected
        ? 'bg-accent-muted'
        : 'hover:bg-surface-hover'
    ]"
  >
    <!-- Icon with colored background -->
    <div
      :class="[
        'flex-shrink-0 w-9 h-9 rounded-lg flex items-center justify-center',
        'ring-1 transition-all duration-150',
        iconClasses.bg,
        iconClasses.ring,
        isSelected && 'ring-2 ring-accent/30'
      ]"
    >
      <svg
        class="w-[18px] h-[18px]"
        :class="iconClasses.text"
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
        stroke-width="1.75"
      >
        <path stroke-linecap="round" stroke-linejoin="round" :d="iconPath" />
      </svg>
    </div>

    <!-- Content -->
    <div class="flex-1 min-w-0 py-0.5">
      <div class="flex items-center gap-2">
        <span
          :class="[
            'font-medium text-sm truncate transition-colors duration-150',
            isSelected ? 'text-accent' : 'text-primary group-hover:text-primary'
          ]"
        >
          {{ result.title }}
        </span>
        <span
          v-if="formattedTime"
          class="flex-shrink-0 text-[10px] text-tertiary font-medium"
        >
          {{ formattedTime }}
        </span>
      </div>
      <div
        v-if="result.preview"
        class="text-xs text-secondary truncate mt-0.5 leading-relaxed"
      >
        {{ result.preview }}
      </div>
    </div>

    <!-- Action hint when selected -->
    <div
      :class="[
        'flex-shrink-0 transition-all duration-150',
        isSelected ? 'opacity-100 translate-x-0' : 'opacity-0 -translate-x-1'
      ]"
    >
      <div class="flex items-center gap-1.5 text-accent">
        <span class="text-[10px] font-medium uppercase tracking-wide">Open</span>
        <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
          <path stroke-linecap="round" stroke-linejoin="round" d="M13 7l5 5m0 0l-5 5m5-5H6" />
        </svg>
      </div>
    </div>
  </button>
</template>
