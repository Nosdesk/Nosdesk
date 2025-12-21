<script setup lang="ts">
import { useRouter } from 'vue-router';
import BackButton from '@/components/common/BackButton.vue';
import { AdminIcons, isBrandIcon, getIconBgClass } from '@/components/admin/AdminIcons';

const router = useRouter();

// Data import integration options
const importOptions = [
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

// Navigate to selected integration
const navigateToOption = (route: string, status: string) => {
  if (status === 'available') {
    router.push(route);
  }
};

// Get status badge configuration
const getStatusBadge = (status: string) => {
  switch (status) {
    case 'available':
      return { text: 'Available', class: 'bg-status-success/20 text-status-success border-status-success/50' };
    case 'coming-soon':
      return { text: 'Coming Soon', class: 'bg-accent/20 text-accent border-accent/50' };
    case 'beta':
      return { text: 'Beta', class: 'bg-accent/20 text-accent border-accent/50' };
    default:
      return { text: status, class: 'bg-surface-alt text-tertiary border-default' };
  }
};

// Get icon content
const getIcon = (iconName: string) => {
  return AdminIcons[iconName as keyof typeof AdminIcons] || AdminIcons.plus;
};
</script>

<template>
  <div class="flex-1">
    <!-- Navigation and actions bar -->
    <div class="pt-4 px-6 flex justify-between items-center">
      <BackButton fallbackRoute="/admin/settings" label="Back to Administration" />
    </div>
    
    <div class="flex flex-col gap-4 px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-6">
        <h1 class="text-2xl font-bold text-primary">Data Import</h1>
        <p class="text-secondary mt-2">
          Import and synchronize data from external sources into your system
        </p>
      </div>

      <!-- Import options grid -->
      <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 mb-8">
        <div
          v-for="(item, index) in importOptions"
          :key="index"
          @click="navigateToOption(item.route, item.status)"
          :class="[
            'bg-surface border border-default rounded-lg p-4 transition duration-150 ease-in-out',
            item.status === 'available' ? 'hover:bg-surface-hover cursor-pointer' : 'opacity-70'
          ]"
        >
          <div class="flex items-center gap-3">
            <div
              class="flex-shrink-0 h-9 w-9 rounded-lg flex items-center justify-center"
              :class="getIconBgClass(item.icon)"
            >
              <span v-if="isBrandIcon(item.icon)" v-html="getIcon(item.icon)"></span>
              <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" v-html="getIcon(item.icon)"></svg>
            </div>
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2">
                <h3 class="text-primary font-medium">{{ item.title }}</h3>
                <span
                  :class="[
                    'px-1.5 py-0.5 text-xs rounded-full border',
                    getStatusBadge(item.status).class
                  ]"
                >
                  {{ getStatusBadge(item.status).text }}
                </span>
              </div>
              <p class="text-xs text-secondary truncate">{{ item.description }}</p>
            </div>
          </div>
        </div>
      </div>

      <!-- Import Guidelines -->
      <div class="flex flex-col gap-2 bg-surface border border-default rounded-lg p-6 mb-8">
        <h2 class="text-xl font-medium text-primary mb-4">Import Guidelines</h2>
        <div class="flex flex-col gap-4 text-sm text-secondary">
          <div class="bg-accent/10 border border-accent/30 rounded-md p-4">
            <h3 class="font-medium text-accent mb-2 flex items-center gap-1">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 mr-2" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
              Data Synchronization Information
            </h3>
            <ul class="list-disc list-inside flex flex-col gap-1 ml-2">
              <li>Data synchronization may take several minutes depending on the amount of data</li>
              <li>Newly imported data will be available immediately after synchronization completes</li>
              <li>Existing data will be updated based on matching criteria (typically unique IDs)</li>
              <li>Regular synchronization schedules can be configured for automatic updates</li>
            </ul>
          </div>
          
          <div class="bg-status-warning/20 border border-status-warning/50 rounded-md p-4">
            <h3 class="font-medium text-status-warning mb-2 flex items-center gap-1">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 mr-2" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
              </svg>
              Important Considerations
            </h3>
            <ul class="list-disc list-inside flex flex-col gap-1 ml-2">
              <li>Ensure you have the necessary access rights to the data source before importing</li>
              <li>Validate data formats and mappings before performing large imports</li>
              <li>Data imports may trigger notifications to affected users</li>
            </ul>
          </div>
        </div>
      </div>
    </div>
  </div>
</template> 