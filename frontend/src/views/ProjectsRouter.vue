<script setup lang="ts">
/**
 * Routes /projects to either the sync-engine V2 view or the legacy
 * REST view based on the `projects_v2` feature flag. This wrapper
 * stays so we don't have to mutate the router config every time
 * the flag flips — flipping the flag swaps the implementation
 * without touching the routes table.
 *
 * Both views are async-imported so the user pays the bundle cost
 * for whichever one they actually render, not both.
 */
import { computed, defineAsyncComponent } from 'vue'
import { useFeatureFlag } from '@/composables/useFeatureFlag'

const projectsV2 = useFeatureFlag('projects_v2')

const Legacy = defineAsyncComponent(() => import('./ProjectsView.vue'))
const V2 = defineAsyncComponent(() => import('./ProjectsViewV2.vue'))

const Component = computed(() => (projectsV2.value ? V2 : Legacy))
</script>

<template>
  <component :is="Component" />
</template>
