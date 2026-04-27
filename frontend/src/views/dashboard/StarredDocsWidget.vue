<!--
Starred documentation pages, a compact reference panel. Icon +
title only, 6 rows max.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'
import {
  getStarredPages,
  type StarredPageInfo,
} from '@/services/documentationService'
import DashboardWidgetShell from './DashboardWidgetShell.vue'

// `isPending` is the first-fetch signal, `isLoading` flips on every
// in-flight request. The shell wants the first-only signal so the
// widget body doesn't blank to a skeleton when a background refetch
// runs over already-rendered cache, see DashboardWidgetShell.
const { data, isPending, isLoading, error } = useQuery({
  key: ['documentation', 'starred'],
  query: async () => (await getStarredPages()).slice(0, 6),
})

const pages = computed<StarredPageInfo[]>(() => data.value ?? [])
const isRefreshing = computed(() => isLoading.value && data.value !== undefined)
const errorMessage = computed(() =>
  error.value ? 'Failed to load starred docs' : null,
)
</script>

<template>
  <DashboardWidgetShell
    title="Starred Docs"
    action-to="/documentation"
    :loading="isPending"
    :refreshing="isRefreshing"
    :error="errorMessage"
    :empty="!errorMessage && pages.length === 0"
    empty-title="No starred pages"
    empty-description="Star a doc to keep it handy."
    min-body-height="200px"
  >
    <ul class="divide-y divide-default">
      <li v-for="p in pages" :key="p.page_id">
        <router-link
          :to="`/documentation/${p.slug}`"
          class="flex items-center gap-2.5 px-4 py-2 hover:bg-surface-hover transition-colors group"
        >
          <span class="text-sm leading-none flex-shrink-0 w-4 text-center">{{ p.icon || '📄' }}</span>
          <span class="text-sm text-primary truncate flex-1 min-w-0 group-hover:text-accent transition-colors">{{ p.title }}</span>
        </router-link>
      </li>
    </ul>
  </DashboardWidgetShell>
</template>
