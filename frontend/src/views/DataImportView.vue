<script setup lang="ts">
import { AdminIcons, isBrandIcon, getIconBgClass } from '@/components/admin/AdminIcons';
import { getAdminIconHtml } from '@/components/admin/adminNavData';

interface ImportOption {
  title: string;
  description: string;
  icon: string;
  route: string;
  status: 'available' | 'coming-soon' | 'beta';
}

const importOptions: ImportOption[] = [
  {
    title: 'Microsoft Graph',
    description: 'Import data from Microsoft 365, including Azure AD, Intune, and other Microsoft services',
    icon: 'microsoft',
    route: '/admin/data-import/microsoft-graph',
    status: 'available'
  },
  {
    title: 'CSV Import',
    description: 'Import data from CSV files, including devices, users, and other resources',
    icon: 'file',
    route: '/admin/data-import/csv',
    status: 'available'
  },
  {
    title: 'API Integrations',
    description: 'Connect to third-party APIs to import and synchronize data',
    icon: 'api',
    route: '/admin/data-import/api',
    status: 'coming-soon'
  },
  {
    title: 'Active Directory',
    description: 'Import data from on-premises Active Directory servers',
    icon: 'directory',
    route: '/admin/data-import/active-directory',
    status: 'coming-soon'
  }
];

const statusBadges: Record<string, { text: string; class: string }> = {
  'available': { text: 'Available', class: 'bg-status-success/15 text-status-success' },
  'coming-soon': { text: 'Coming Soon', class: 'bg-accent/15 text-accent' },
  'beta': { text: 'Beta', class: 'bg-accent/15 text-accent' }
};
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <div>
        <h1 class="text-xl sm:text-2xl font-bold text-primary">Data Import</h1>
        <p class="text-secondary text-sm sm:text-base mt-1">Import and synchronize data from external sources</p>
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
            class="flex-shrink-0 h-10 w-10 rounded-lg flex items-center justify-center"
            :class="getIconBgClass(item.icon)"
          >
            <span v-if="isBrandIcon(item.icon)" v-html="AdminIcons[item.icon as keyof typeof AdminIcons]"></span>
            <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" v-html="getAdminIconHtml(item.icon)"></svg>
          </div>
          <div class="flex-1 min-w-0">
            <div class="flex items-center gap-2">
              <h3 class="text-sm font-medium text-primary">{{ item.title }}</h3>
              <span
                class="px-1.5 py-0.5 text-[11px] font-medium rounded-full"
                :class="statusBadges[item.status]?.class"
              >
                {{ statusBadges[item.status]?.text }}
              </span>
            </div>
            <p class="text-xs text-secondary mt-0.5 line-clamp-2 sm:line-clamp-1">{{ item.description }}</p>
          </div>
          <svg
            v-if="item.status === 'available'"
            xmlns="http://www.w3.org/2000/svg"
            class="h-4 w-4 text-tertiary group-hover:text-secondary transition-colors flex-shrink-0"
            fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"
          >
            <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
          </svg>
        </component>
      </div>

      <!-- Notice -->
      <div class="flex items-start gap-2.5 px-1 text-xs text-tertiary">
        <svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5 flex-shrink-0 mt-0.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <p>Data imports may trigger notifications to affected users. Existing records are updated based on matching IDs.</p>
      </div>
    </div>
  </div>
</template>
