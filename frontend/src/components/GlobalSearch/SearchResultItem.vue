<script setup lang="ts">
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import type { SearchResult, SearchEntityType } from '@/types/search';
import { ENTITY_TYPE_CONFIG } from '@/types/search';
import Icon from '@/components/common/Icon.vue';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const props = defineProps<{
  result: SearchResult;
  isSelected: boolean;
}>();

const emit = defineEmits<{
  select: [result: SearchResult];
}>();

const iconName = computed(() => ENTITY_TYPE_CONFIG[props.result.entity_type]?.icon ?? 'search');

// Per-type colour for the leading icon. The selected row's full-row
// accent background is enough on its own — no ring on the icon, no
// colour shift on selection. Restraint reads as more confident than
// the previous "bg + ring + ring-on-select" stack.
const iconClasses = computed(() => {
  const styles: Record<SearchEntityType, { bg: string; text: string }> = {
    ticket:        { bg: 'bg-status-info-muted',     text: 'text-status-info' },
    comment:       { bg: 'bg-status-success-muted',  text: 'text-status-success' },
    documentation: { bg: 'bg-[rgba(139,92,246,0.15)]', text: 'text-brand-purple' },
    attachment:    { bg: 'bg-status-warning-muted',  text: 'text-status-warning' },
    device:        { bg: 'bg-[rgba(44,128,255,0.15)]', text: 'text-brand-blue' },
    user:          { bg: 'bg-[rgba(255,102,179,0.15)]', text: 'text-brand-pink' },
    project:       { bg: 'bg-accent-muted',          text: 'text-accent' },
  };
  return styles[props.result.entity_type] ?? { bg: 'bg-surface-alt', text: 'text-tertiary' };
});

const formattedTime = computed(() => {
  if (!props.result.updated_at) return null;
  const date = new Date(props.result.updated_at);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));

  if (diffDays === 0) return t('search-result-item-today');
  if (diffDays === 1) return t('search-result-item-yesterday');
  if (diffDays < 7) return t('search-result-item-days-ago', { count: diffDays });
  if (diffDays < 30) return t('search-result-item-weeks-ago', { count: Math.floor(diffDays / 7) });
  if (diffDays < 365) return t('search-result-item-months-ago', { count: Math.floor(diffDays / 30) });
  return t('search-result-item-years-ago', { count: Math.floor(diffDays / 365) });
});
</script>

<template>
  <button
    type="button"
    :data-selected="isSelected"
    @click="emit('select', result)"
    :class="[
      'w-full px-2 py-1.5 flex items-center gap-2.5 text-left rounded-md transition-colors',
      'focus:outline-none',
      isSelected ? 'bg-accent/10' : 'hover:bg-surface-hover/60',
    ]"
  >
    <span
      :class="[
        'flex-shrink-0 inline-flex w-7 h-7 rounded-md items-center justify-center',
        iconClasses.bg,
        iconClasses.text,
      ]"
    >
      <Icon :name="iconName" size="xs" />
    </span>

    <div class="flex-1 min-w-0">
      <div class="flex items-center gap-2">
        <span
          class="font-medium text-[13px] truncate"
          :class="isSelected ? 'text-primary' : 'text-primary'"
        >
          {{ result.title }}
        </span>
        <!-- Internal-note badge. Only rendered for comment hits;
             the backend filters internal notes out of non-staff
             results entirely, so this badge always implies the
             current user has staff access. -->
        <span
          v-if="result.is_internal"
          class="flex-shrink-0 inline-flex items-center px-1.5 h-[15px] rounded text-[9px] font-semibold uppercase tracking-wide bg-status-warning-muted text-status-warning"
          :title="t('search-result-item-internal-title')"
        >
          {{ t('search-result-item-internal-badge') }}
        </span>
        <span
          v-if="formattedTime"
          class="flex-shrink-0 text-[10px] text-tertiary tabular-nums"
        >
          {{ formattedTime }}
        </span>
      </div>
      <div
        v-if="result.preview"
        class="text-[11px] text-secondary truncate mt-0.5"
      >
        {{ result.preview }}
      </div>
    </div>

    <!-- Action hint on the selected row only. A single ↵ is the
         Raycast convention — title says what; key says how. -->
    <kbd
      v-if="isSelected"
      class="flex-shrink-0 inline-flex items-center justify-center w-5 h-5 rounded border border-default bg-surface text-[10px] font-medium text-secondary"
    >
      ↵
    </kbd>
  </button>
</template>
