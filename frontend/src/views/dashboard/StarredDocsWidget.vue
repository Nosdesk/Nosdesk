<!--
Starred documentation pages — a compact reference panel. Icon +
title only, 6 rows max.
-->
<script setup lang="ts">
import {
  getStarredPages,
  type StarredPageInfo,
} from '@/services/documentationService'
import { useAsyncResource } from '@/composables/useAsyncResource'
import DashboardWidgetShell from './DashboardWidgetShell.vue'

const { data: pages, loading, error } = useAsyncResource<StarredPageInfo[]>(
  async () => (await getStarredPages()).slice(0, 6),
  [],
  'Failed to load starred docs',
)
</script>

<template>
  <DashboardWidgetShell
    title="Starred Docs"
    action-to="/documentation"
    :loading="loading"
    :error="error"
    :empty="!error && pages.length === 0"
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
