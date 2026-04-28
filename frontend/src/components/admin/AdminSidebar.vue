<script setup lang="ts">
import { ref, computed } from 'vue';
import { useRoute } from 'vue-router';
import {
  adminNavGroups,
  isAdminRouteActive,
  getAdminIconHtml,
  filterAdminNavGroups
} from '@/components/admin/adminNavData';
import Icon from '@/components/common/Icon.vue';

const route = useRoute();
const searchQuery = ref('');

const filteredGroups = computed(() => filterAdminNavGroups(adminNavGroups, searchQuery.value));

const isActive = (itemRoute: string) => isAdminRouteActive(route.path, itemRoute);
</script>

<template>
  <aside class="hidden lg:flex lg:flex-col lg:w-64 flex-shrink-0 h-full overflow-y-auto border-r border-default bg-surface">
    <!-- Back to Dashboard -->
    <div class="px-4 pt-4 pb-2">
      <RouterLink
        to="/"
        class="flex items-center gap-2 text-sm text-secondary hover:text-primary transition-colors"
      >
        <Icon name="chevronLeft" />
        Back to Dashboard
      </RouterLink>
    </div>

    <!-- Heading -->
    <div class="px-4 pb-3">
      <h2 class="text-lg font-bold text-primary">Administration</h2>
    </div>

    <!-- Search -->
    <div class="px-3 pb-3">
      <div class="relative">
        <span class="absolute left-2.5 top-1/2 -translate-y-1/2 text-tertiary inline-flex">
          <Icon name="search" />
        </span>
        <input
          v-model="searchQuery"
          type="text"
          placeholder="Search settings..."
          class="w-full pl-8 pr-3 py-1.5 text-sm bg-surface-alt text-primary rounded-lg border border-default focus:ring-1 focus:ring-accent focus:border-accent focus:outline-none placeholder:text-tertiary"
        />
        <button
          v-if="searchQuery"
          @click="searchQuery = ''"
          class="absolute right-2 top-1/2 -translate-y-1/2 text-tertiary hover:text-secondary"
        >
          <Icon name="close" />
        </button>
      </div>
    </div>

    <!-- Nav Groups -->
    <nav class="flex-1 px-2 pb-4 flex flex-col gap-4 overflow-y-auto">
      <div v-for="group in filteredGroups" :key="group.label">
        <h3 class="px-2 mb-1 text-[11px] font-semibold uppercase tracking-wider text-tertiary">
          {{ group.label }}
        </h3>
        <div class="flex flex-col gap-0.5">
          <RouterLink
            v-for="item in group.items"
            :key="item.route"
            :to="item.route"
            class="flex items-center gap-2.5 px-2.5 py-2 rounded-lg transition-colors duration-150 relative overflow-hidden text-sm"
            :class="[
              isActive(item.route)
                ? 'bg-accent/10 border border-accent text-accent font-medium'
                : 'text-secondary hover:bg-surface-hover hover:text-primary border border-transparent'
            ]"
          >
            <div
              v-if="isActive(item.route)"
              class="absolute left-0 top-0 bottom-0 w-1 bg-accent rounded-r"
            ></div>
            <div
              class="flex-shrink-0 h-6 w-6 rounded flex items-center justify-center"
              :class="isActive(item.route) ? 'text-accent' : 'text-secondary'"
            >
              <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" v-html="getAdminIconHtml(item.icon)"></svg>
            </div>
            <span class="truncate">{{ item.title }}</span>
          </RouterLink>
        </div>
      </div>

      <div v-if="filteredGroups.length === 0" class="px-3 py-4 text-center">
        <p class="text-sm text-tertiary">No settings match "{{ searchQuery }}"</p>
      </div>
    </nav>
  </aside>
</template>
