<script setup lang="ts">
import { ref } from 'vue';
import { useRouter } from 'vue-router';
import BackButton from '@/components/common/BackButton.vue';

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
      return { text: 'Available', class: 'bg-green-900/50 text-green-400 border-green-700' };
    case 'coming-soon':
      return { text: 'Coming Soon', class: 'bg-blue-900/50 text-blue-400 border-blue-700' };
    case 'beta':
      return { text: 'Beta', class: 'bg-purple-900/50 text-purple-400 border-purple-700' };
    default:
      return { text: status, class: 'bg-slate-900/50 text-slate-400 border-slate-700' };
  }
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
            'bg-surface border border-default rounded-lg p-5 transition duration-150 ease-in-out',
            item.status === 'available' ? 'hover:bg-surface-hover cursor-pointer' : 'opacity-80'
          ]"
        >
          <div class="flex items-start gap-2">
            <div class="flex-shrink-0 h-10 w-10 rounded-md bg-blue-600/20 flex items-center justify-center text-blue-400 mr-4">
              <svg v-if="item.icon === 'microsoft'" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 21 21" fill="none" class="h-6 w-6">
                <rect x="1" y="1" width="9" height="9" fill="#f25022"/>
                <rect x="1" y="11" width="9" height="9" fill="#00a4ef"/>
                <rect x="11" y="1" width="9" height="9" fill="#7fba00"/>
                <rect x="11" y="11" width="9" height="9" fill="#ffb900"/>
              </svg>
              <svg v-else-if="item.icon === 'file'" xmlns="http://www.w3.org/2000/svg" class="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
              </svg>
              <svg v-else-if="item.icon === 'api'" xmlns="http://www.w3.org/2000/svg" class="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 9l3 3-3 3m5 0h3M5 20h14a2 2 0 002-2V6a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z" />
              </svg>
              <svg v-else-if="item.icon === 'directory'" xmlns="http://www.w3.org/2000/svg" class="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 19a2 2 0 01-2-2V7a2 2 0 012-2h4l2 2h4a2 2 0 012 2v1M5 19h14a2 2 0 002-2v-5a2 2 0 00-2-2H9a2 2 0 00-2 2v5a2 2 0 01-2 2z" />
              </svg>
              <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6v6m0 0v6m0-6h6m-6 0H6" />
              </svg>
            </div>
            <div class="flex-1">
              <div class="flex items-center justify-between">
                <h3 class="text-primary font-medium">{{ item.title }}</h3>
                <span 
                  :class="[
                    'ml-2 px-2 py-0.5 text-xs rounded-full border inline-block',
                    getStatusBadge(item.status).class
                  ]"
                >
                  {{ getStatusBadge(item.status).text }}
                </span>
              </div>
              <p class="mt-1 text-sm text-secondary">{{ item.description }}</p>
            </div>
          </div>
        </div>
      </div>

      <!-- Import Guidelines -->
      <div class="flex flex-col gap-2 bg-surface border border-default rounded-lg p-6 mb-8">
        <h2 class="text-xl font-medium text-primary mb-4">Import Guidelines</h2>
        <div class="flex flex-col gap-4 text-sm text-secondary">
          <div class="bg-blue-900/20 border border-blue-800/50 rounded-md p-4">
            <h3 class="font-medium text-blue-300 mb-2 flex items-center gap-1">
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
          
          <div class="bg-amber-900/20 border border-amber-800/50 rounded-md p-4">
            <h3 class="font-medium text-amber-300 mb-2 flex items-center gap-1">
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