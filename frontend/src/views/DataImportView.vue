<script setup lang="ts">
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import Icon from '@/components/common/Icon.vue';
import type { IconName } from '@/components/common/icons';

interface ImportOption {
  titleKey: string;
  descriptionKey: string;
  icon: IconName;
  route: string;
  status: 'available' | 'coming-soon' | 'beta';
}

const fluent = useFluent();
const t = (key: string) => fluent.$t(key);

const importOptions: ImportOption[] = [
  {
    titleKey: 'admin-data-import-microsoft-title',
    descriptionKey: 'admin-data-import-microsoft-description',
    icon: 'microsoft',
    route: '/admin/data-import/microsoft-graph',
    status: 'available'
  },
  {
    titleKey: 'admin-data-import-csv-title',
    descriptionKey: 'admin-data-import-csv-description',
    icon: 'document',
    route: '/admin/data-import/csv',
    status: 'available'
  },
  {
    titleKey: 'admin-data-import-api-title',
    descriptionKey: 'admin-data-import-api-description',
    icon: 'api',
    route: '/admin/data-import/api',
    status: 'coming-soon'
  },
  {
    titleKey: 'admin-data-import-ad-title',
    descriptionKey: 'admin-data-import-ad-description',
    icon: 'directory',
    route: '/admin/data-import/active-directory',
    status: 'coming-soon'
  }
];

const statusBadges = computed<Record<string, { text: string; class: string }>>(() => ({
  'available': { text: t('admin-data-import-status-available'), class: 'bg-status-success/15 text-status-success' },
  'coming-soon': { text: t('admin-data-import-status-coming-soon'), class: 'bg-accent/15 text-accent' },
  'beta': { text: t('admin-data-import-status-beta'), class: 'bg-accent/15 text-accent' }
}));
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <div>
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('admin-data-import-title') }}</h1>
        <p class="text-secondary text-sm sm:text-base mt-1">{{ $t('admin-data-import-description') }}</p>
      </div>

      <!-- Import options -->
      <div class="bg-surface border border-default rounded-xl overflow-hidden divide-y divide-default">
        <component
          v-for="item in importOptions"
          :key="item.route"
          :is="item.status === 'available' ? 'RouterLink' : 'div'"
          :to="item.status === 'available' ? item.route : undefined"
          class="flex items-center gap-3 sm:gap-4 px-4 py-4 transition-colors"
          :class="item.status === 'available'
            ? 'hover:bg-surface-hover cursor-pointer group'
            : 'opacity-60'"
        >
          <div
            class="flex-shrink-0 h-10 w-10 rounded-lg flex items-center justify-center bg-accent/15 text-accent"
          >
            <Icon :name="item.icon" size="md" />
          </div>
          <div class="flex-1 min-w-0">
            <div class="flex items-center gap-2">
              <h3 class="text-sm font-medium text-primary">{{ $t(item.titleKey) }}</h3>
              <span
                class="px-1.5 py-0.5 text-[11px] font-medium rounded-full"
                :class="statusBadges[item.status]?.class"
              >
                {{ statusBadges[item.status]?.text }}
              </span>
            </div>
            <p class="text-xs text-secondary mt-0.5 line-clamp-2 sm:line-clamp-1">{{ $t(item.descriptionKey) }}</p>
          </div>
          <Icon
            v-if="item.status === 'available'"
            name="chevronRight"
            class="text-tertiary group-hover:text-secondary transition-colors flex-shrink-0"
          />
        </component>
      </div>

      <!-- Notice -->
      <div class="flex items-start gap-2.5 px-1 text-xs text-tertiary">
        <svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5 flex-shrink-0 mt-0.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <p>{{ $t('admin-data-import-notice') }}</p>
      </div>
    </div>
  </div>
</template>
