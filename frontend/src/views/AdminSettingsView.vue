<script setup lang="ts">
import { useRouter } from 'vue-router';
import BackButton from '@/components/common/BackButton.vue';
import SystemInfoCard from '@/components/admin/SystemInfoCard.vue';
import { AdminIcons, getIconBgClass } from '@/components/admin/AdminIcons';

const router = useRouter();

// Navigation options for admin settings
const adminMenuItems = [
  {
    title: 'Groups',
    description: 'Manage user groups and memberships',
    icon: 'users',
    route: '/admin/groups'
  },
  {
    title: 'Categories',
    description: 'Configure ticket categories and group visibility',
    icon: 'tag',
    route: '/admin/categories'
  },
  {
    title: 'Assignment Rules',
    description: 'Configure automatic ticket assignment based on rules',
    icon: 'lightning',
    route: '/admin/assignment-rules'
  },
  {
    title: 'API Tokens',
    description: 'Manage API tokens for programmatic access',
    icon: 'key',
    route: '/admin/api-tokens'
  },
  {
    title: 'Webhooks',
    description: 'Configure webhooks to send events to external services',
    icon: 'link',
    route: '/admin/webhooks'
  },
  {
    title: 'Plugins',
    description: 'Manage installed plugins and integrations',
    icon: 'puzzle',
    route: '/admin/plugins'
  },
  {
    title: 'Authentication Providers',
    description: 'Configure SSO, Microsoft Entra, and local authentication settings',
    icon: 'lock',
    route: '/admin/auth-providers'
  },
  {
    title: 'Search',
    description: 'Manage the search index and view indexing statistics',
    icon: 'search',
    route: '/admin/search'
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
  },
  {
    title: 'Backup & Restore',
    description: 'Export and restore system data and attachments',
    icon: 'archive',
    route: '/admin/backup-restore'
  }
];

// Helper function to render the icon based on name
const renderIcon = (iconName: string) => {
  return AdminIcons[iconName as keyof typeof AdminIcons] || AdminIcons.plus;
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
        <h1 class="text-2xl font-bold text-primary">Administration</h1>
        <p class="text-secondary mt-2">
          Configure global settings and manage enterprise features
        </p>
      </div>

      <!-- Admin menu grid -->
      <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 mb-8">
        <div
          v-for="(item, index) in adminMenuItems"
          :key="index"
          @click="navigateToOption(item.route)"
          class="bg-surface border border-default rounded-lg p-4 hover:bg-surface-hover cursor-pointer transition duration-150 ease-in-out"
        >
          <div class="flex items-center gap-3">
            <div
              class="flex-shrink-0 h-9 w-9 rounded-lg flex items-center justify-center"
              :class="getIconBgClass(item.icon)"
            >
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" v-html="renderIcon(item.icon)"></svg>
            </div>
            <div class="flex-1 min-w-0">
              <h3 class="text-primary font-medium">{{ item.title }}</h3>
              <p class="text-xs text-secondary">{{ item.description }}</p>
            </div>
          </div>
        </div>
      </div>

      <!-- System Information -->
      <SystemInfoCard />
    </div>
  </div>
</template> 