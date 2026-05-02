<script setup lang="ts">
/**
 * Routes /tickets to the sync-engine V2 list view or the legacy
 * REST view based on the projects_v2 feature flag. Same
 * dispatcher pattern as ProjectsRouter / ProjectDetailRouter so
 * the flag flip swaps implementations without touching the
 * routes table.
 */
import { computed, defineAsyncComponent } from 'vue'
import { useFeatureFlag } from '@/composables/useFeatureFlag'

const projectsV2 = useFeatureFlag('projects_v2')

const Legacy = defineAsyncComponent(() => import('./TicketsListView.vue'))
const V2 = defineAsyncComponent(() => import('@/sync/views/TicketsListViewV2.vue'))

const Component = computed(() => (projectsV2.value ? V2 : Legacy))
</script>

<template>
  <component :is="Component" />
</template>
