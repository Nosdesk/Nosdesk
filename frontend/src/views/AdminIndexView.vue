<script setup lang="ts">
import { ref, computed } from 'vue';
import { getIconBgClass } from '@/components/admin/AdminIcons';
import {
  adminNavGroups,
  getAdminIconHtml,
  filterAdminNavGroups
} from '@/components/admin/adminNavData';
import Icon from '@/components/common/Icon.vue';

const searchQuery = ref('');
const filteredGroups = computed(() => filterAdminNavGroups(adminNavGroups, searchQuery.value));
</script>

<template>
  <div class="flex-1 flex flex-col">
    <div class="flex-1 flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-4xl">
      <!-- Header -->
      <div class="mb-2">
        <!-- Phone/small tablet: back link (hidden at md+ where there's enough context) -->
        <RouterLink
          to="/"
          class="md:hidden flex items-center gap-2 text-sm text-secondary hover:text-primary transition-colors mb-3"
        >
          <Icon name="chevronLeft" />
          Back to Dashboard
        </RouterLink>
        <div class="flex flex-col md:flex-row md:items-center md:justify-between gap-3">
          <div>
            <h1 class="text-xl lg:text-2xl font-bold text-primary">Administration</h1>
            <p class="text-secondary text-sm lg:text-base mt-1">Manage your system settings, integrations, and workspace configuration</p>
          </div>
          <!-- Tablet: inline search (hidden on phone where it's sticky-bottom, hidden on desktop where sidebar has it) -->
          <div class="hidden md:block lg:hidden w-64 flex-shrink-0">
            <div class="relative">
              <span class="absolute left-3 top-1/2 -translate-y-1/2 text-tertiary inline-flex">
                <Icon name="search" />
              </span>
              <input
                v-model="searchQuery"
                type="text"
                placeholder="Search settings..."
                class="w-full pl-9 pr-9 py-2 text-sm bg-surface-alt text-primary rounded-lg border border-default focus:ring-1 focus:ring-accent focus:border-accent focus:outline-none placeholder:text-tertiary"
              />
              <button
                v-if="searchQuery"
                @click="searchQuery = ''"
                class="absolute right-3 top-1/2 -translate-y-1/2 text-tertiary hover:text-secondary"
              >
                <Icon name="close" />
              </button>
            </div>
          </div>
        </div>
      </div>

      <!-- Grouped sections -->
      <div class="flex flex-col gap-5">
        <div v-for="group in filteredGroups" :key="group.label">
          <h2 class="text-[11px] font-semibold uppercase tracking-wider text-tertiary mb-2 px-1">
            {{ group.label }}
          </h2>
          <div class="bg-surface border border-default rounded-xl overflow-hidden divide-y divide-default">
            <RouterLink
              v-for="item in group.items"
              :key="item.route"
              :to="item.route"
              class="flex items-center gap-3 px-4 py-3.5 hover:bg-surface-hover transition-colors group"
            >
              <div
                class="flex-shrink-0 h-9 w-9 rounded-lg flex items-center justify-center"
                :class="getIconBgClass(item.icon)"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" v-html="getAdminIconHtml(item.icon)"></svg>
              </div>
              <div class="flex-1 min-w-0">
                <h3 class="text-sm font-medium text-primary">{{ item.title }}</h3>
                <p class="text-xs text-secondary mt-0.5 line-clamp-1">{{ item.description }}</p>
              </div>
              <span class="text-tertiary group-hover:text-secondary transition-colors flex-shrink-0 inline-flex">
                <Icon name="chevronRight" />
              </span>
            </RouterLink>
          </div>
        </div>

        <!-- No results -->
        <div v-if="filteredGroups.length === 0" class="py-8 text-center">
          <p class="text-sm text-tertiary">No settings match "{{ searchQuery }}"</p>
          <button @click="searchQuery = ''" class="mt-2 text-sm text-accent hover:underline">Clear search</button>
        </div>
      </div>
    </div>

    <!-- Search — pinned to bottom on phone only; tablet has inline search, desktop has sidebar search -->
    <div class="sticky bottom-0 bg-app border-t border-default px-4 sm:px-6 py-3 md:hidden">
      <div class="relative max-w-4xl mx-auto">
        <span class="absolute left-3 top-1/2 -translate-y-1/2 text-tertiary inline-flex">
          <Icon name="search" />
        </span>
        <input
          v-model="searchQuery"
          type="text"
          placeholder="Search settings..."
          class="w-full pl-9 pr-9 py-2.5 text-sm bg-surface text-primary rounded-xl border border-default focus:ring-1 focus:ring-accent focus:border-accent focus:outline-none placeholder:text-tertiary"
        />
        <button
          v-if="searchQuery"
          @click="searchQuery = ''"
          class="absolute right-3 top-1/2 -translate-y-1/2 text-tertiary hover:text-secondary"
        >
          <Icon name="close" />
        </button>
      </div>
    </div>
  </div>
</template>
