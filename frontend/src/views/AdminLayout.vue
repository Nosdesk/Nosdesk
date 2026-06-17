<script setup lang="ts">
import { computed } from 'vue';
import { useRoute } from 'vue-router';
import { useFluent } from 'fluent-vue';
import AdminSidebar from '@/components/admin/AdminSidebar.vue';
import { allAdminNavItems, isAdminRouteActive } from '@/components/admin/adminNavData';

const route = useRoute();
const fluent = useFluent();

// Index-page check via the named route, not a string compare.
// The router resolves trailing slashes, query strings, hashes, and
// future path renames into the same `route.name`, so a name check
// is more robust than `route.path === '/admin'`.
const isIndexPage = computed(() => route.name === 'admin-index');

// Current active nav item for the mobile breadcrumb
const activeItem = computed(() => {
  return allAdminNavItems.find(item => isAdminRouteActive(route.path, item.route));
});

const activeItemTitle = computed(() =>
  activeItem.value ? fluent.$t(activeItem.value.titleKey) : ''
);
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
          {{ $t('admin-heading') }}
        </RouterLink>
        <template v-if="activeItem">
          <svg xmlns="http://www.w3.org/2000/svg" class="h-3 w-3 text-tertiary flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
          </svg>
          <span class="text-sm text-primary font-medium truncate">{{ activeItemTitle }}</span>
        </template>
      </div>
    </div>

    <div class="flex-1 min-w-0 h-full overflow-auto">
      <!-- Inner page transition. The top-level RouterView in
           App.vue keys by the parent route's path, so AdminLayout
           (and its sidebar) stays mounted across admin sub-route
           navigations. The transition here lives on the inner
           RouterView so the *content area* still gets the same
           page fade. Sidebar persists, content swaps. -->
      <RouterView v-slot="{ Component, route: childRoute }">
        <Transition name="page" mode="out-in">
          <component
            :is="Component"
            :key="childRoute.fullPath"
          />
        </Transition>
      </RouterView>
    </div>
  </div>
</template>
