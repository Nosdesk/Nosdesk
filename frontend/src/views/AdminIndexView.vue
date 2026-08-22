<script setup lang="ts">
import { ref, computed } from 'vue';
import { useFluent } from 'fluent-vue';
import {
  adminNavGroups,
  filterAdminNavGroups,
  filterAdminNavGroupsForRole
} from '@/components/admin/adminNavData';
import { useAuthStore } from '@/stores/auth';
import Icon from '@/components/common/Icon.vue';

const fluent = useFluent();
const authStore = useAuthStore();
const searchQuery = ref('');
// Role-filter first (so platform-operator-only tiles stay hidden from
// per-workspace admins), then apply the search filter.
const roleGroups = computed(() =>
  filterAdminNavGroupsForRole(adminNavGroups, {
    isAdmin: authStore.isAdmin,
    isAuditReviewer: authStore.isAuditReviewer,
    isPlatformAdmin: authStore.isPlatformAdmin
  })
);
const filteredGroups = computed(() =>
  filterAdminNavGroups(roleGroups.value, searchQuery.value, (key) => fluent.$t(key))
);
</script>

<template>
  <div class="flex-1 flex flex-col">
    <div class="flex-1 flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-4xl">
      <!-- Header -->
      <div class="mb-2">
        <div class="flex flex-col md:flex-row md:items-center md:justify-between gap-3">
          <div>
            <h1 class="text-xl lg:text-2xl font-bold text-primary">{{ $t('admin-heading') }}</h1>
            <p class="text-secondary text-sm lg:text-base mt-1">{{ $t('admin-index-subtitle') }}</p>
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
                :placeholder="$t('admin-search-placeholder')"
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
        <div v-for="group in filteredGroups" :key="group.labelKey">
          <h2 class="text-[11px] font-semibold uppercase tracking-wider text-tertiary mb-2 px-1">
            {{ $t(group.labelKey) }}
          </h2>
          <div class="bg-surface border border-default rounded-xl overflow-hidden divide-y divide-default">
            <RouterLink
              v-for="item in group.items"
              :key="item.route"
              :to="item.route"
              class="flex items-center gap-3 px-4 py-3.5 hover:bg-surface-hover transition-colors group"
            >
              <div
                class="flex-shrink-0 h-9 w-9 rounded-lg flex items-center justify-center bg-accent/15 text-accent"
              >
                <Icon :name="item.icon" size="md" />
              </div>
              <div class="flex-1 min-w-0">
                <h3 class="text-sm font-medium text-primary">{{ $t(item.titleKey) }}</h3>
                <p class="text-xs text-secondary mt-0.5 line-clamp-1">{{ $t(item.descriptionKey) }}</p>
              </div>
              <span class="text-tertiary group-hover:text-secondary transition-colors flex-shrink-0 inline-flex">
                <Icon name="chevronRight" />
              </span>
            </RouterLink>
          </div>
        </div>

        <!-- No results -->
        <div v-if="filteredGroups.length === 0" class="py-8 text-center">
          <p class="text-sm text-tertiary">{{ $t('admin-search-empty', { query: searchQuery }) }}</p>
          <button @click="searchQuery = ''" class="mt-2 text-sm text-accent hover:underline">{{ $t('admin-clear-search') }}</button>
        </div>
      </div>
    </div>

    <!-- Search pinned to bottom on phone only; tablet has inline search, desktop has sidebar search -->
    <div class="sticky bottom-0 bg-app border-t border-default px-4 sm:px-6 py-3 md:hidden">
      <div class="relative max-w-4xl mx-auto">
        <span class="absolute left-3 top-1/2 -translate-y-1/2 text-tertiary inline-flex">
          <Icon name="search" />
        </span>
        <input
          v-model="searchQuery"
          type="text"
          :placeholder="$t('admin-search-placeholder')"
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
