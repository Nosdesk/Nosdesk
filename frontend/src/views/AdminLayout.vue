<script setup lang="ts">
import { computed } from 'vue';
import { useRoute } from 'vue-router';
import AdminSidebar from '@/components/admin/AdminSidebar.vue';
import { allAdminNavItems, isAdminRouteActive } from '@/components/admin/adminNavData';

const route = useRoute();

// Whether we're on the admin index page (not a sub-page)
const isIndexPage = computed(() => route.path === '/admin' || route.path === '/admin/');

// Current active nav item for the mobile breadcrumb
const activeItem = computed(() => {
  return allAdminNavItems.find(item => isAdminRouteActive(route.path, item.route));
});
</script>

<template>
  <div class="flex flex-col lg:flex-row h-full">
    <AdminSidebar />

    <!-- Mobile: back-to-admin bar on sub-pages -->
    <div v-if="!isIndexPage" class="lg:hidden border-b border-default bg-app">
      <div class="px-4 sm:px-6 py-2.5 flex items-center gap-2.5">
        <RouterLink
          to="/admin"
          class="flex items-center gap-1.5 text-sm text-secondary hover:text-primary transition-colors"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M10 19l-7-7m0 0l7-7m-7 7h18" />
          </svg>
          Administration
        </RouterLink>
        <template v-if="activeItem">
          <svg xmlns="http://www.w3.org/2000/svg" class="h-3 w-3 text-tertiary flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
          </svg>
          <span class="text-sm text-primary font-medium truncate">{{ activeItem.title }}</span>
        </template>
      </div>
    </div>

    <div class="flex-1 min-w-0 h-full overflow-auto">
      <RouterView />
    </div>
  </div>
</template>
