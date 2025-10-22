<script setup lang="ts">
import { ref } from 'vue';
import { useRouter } from 'vue-router';
import BackButton from '@/components/common/BackButton.vue';

const router = useRouter();

// Navigation options for admin settings
const adminMenuItems = [
  {
    title: 'Authentication Providers',
    description: 'Configure SSO, Microsoft Entra, and local authentication settings',
    icon: 'lock',
    route: '/admin/auth-providers'
  },
  {
    title: 'System Settings',
    description: 'Manage storage, cleanup stale files, and system maintenance',
    icon: 'cog',
    route: '/admin/system-settings'
  },
  {
    title: 'Branding',
    description: 'Customize the appearance and branding of the application',
    icon: 'paint',
    route: '/admin/settings/branding'
  },
  {
    title: 'Email Configuration',
    description: 'Configure SMTP settings and send test emails',
    icon: 'mail',
    route: '/admin/email-settings'
  },
  {
    title: 'Data Import',
    description: 'Import data from Intune, CSV files, and other external sources',
    icon: 'database',
    route: '/admin/data-import'
  }
];

// Helper function to render the icon based on name
const renderIcon = (iconName: string) => {
  switch (iconName) {
    case 'lock':
      return `<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />`;
    case 'cog':
      return `<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" /><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />`;
    case 'paint':
      return `<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 21a4 4 0 01-4-4V5a2 2 0 012-2h4a2 2 0 012 2v12a4 4 0 01-4 4zm0 0h12a2 2 0 002-2v-4a2 2 0 00-2-2h-2.343M11 7.343l1.657-1.657a2 2 0 012.828 0l2.829 2.829a2 2 0 010 2.828l-8.486 8.485M7 17h.01" />`;
    case 'mail':
      return `<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />`;
    case 'database':
      return `<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4m0 5c0 2.21-3.582 4-8 4s-8-1.79-8-4" />`;
    default:
      return `<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6v6m0 0v6m0-6h6m-6 0H6" />`;
  }
};

const navigateToOption = (route: string) => {
  router.push(route);
};
</script>

<template>
  <div class="flex-1">
    <!-- Navigation and actions bar -->
    <div class="pt-4 px-6 flex justify-between items-center">
      <BackButton fallbackRoute="/" label="Back to Dashboard" />
    </div>
    
    <div class="flex flex-col gap-4 px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-6">
        <h1 class="text-2xl font-bold text-white">Administration</h1>
        <p class="text-slate-400 mt-2">
          Configure global settings and manage enterprise features
        </p>
      </div>

      <!-- Admin menu grid -->
      <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 mb-8">
        <div
          v-for="(item, index) in adminMenuItems"
          :key="index"
          @click="navigateToOption(item.route)"
          class="bg-slate-800 border border-slate-700 rounded-lg p-5 hover:bg-slate-750 cursor-pointer transition duration-150 ease-in-out"
        >
          <div class="flex items-start gap-2">
            <div class="flex-shrink-0 h-10 w-10 rounded-md bg-blue-600/20 flex items-center justify-center text-blue-400 mr-4">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor" v-html="renderIcon(item.icon)"></svg>
            </div>
            <div>
              <h3 class="text-white font-medium">{{ item.title }}</h3>
              <p class="mt-1 text-sm text-slate-400">{{ item.description }}</p>
            </div>
          </div>
        </div>
      </div>

      <!-- System Information -->
      <div class="bg-slate-800 border border-slate-700 rounded-lg p-6 mb-8">
        <h2 class="text-xl font-medium text-white mb-4">System Information</h2>
        <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          <div class="py-2">
            <div class="text-sm text-slate-400">Application Version</div>
            <div class="text-white">Nosdesk v1.0.0</div>
          </div>
          <div class="py-2">
            <div class="text-sm text-slate-400">Last Update</div>
            <div class="text-white">April 4, 2023</div>
          </div>
          <div class="py-2">
            <div class="text-sm text-slate-400">Environment</div>
            <div class="text-white">Production</div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template> 