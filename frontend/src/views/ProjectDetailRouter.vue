<script setup lang="ts">
/**
 * Routes /projects/:id to either the sync-engine V2 detail view
 * or the legacy REST view based on the `projects_v2` feature flag.
 * Same async-import dispatcher pattern as ProjectsRouter so the
 * user only pays the bundle cost for whichever view they actually
 * render.
 */
import { computed, defineAsyncComponent } from 'vue'
import { useFeatureFlag } from '@/composables/useFeatureFlag'

const props = defineProps<{ id: string }>()

const projectsV2 = useFeatureFlag('projects_v2')

const Legacy = defineAsyncComponent(() => import('./ProjectDetailView.vue'))
const V2 = defineAsyncComponent(() => import('./ProjectDetailViewV2.vue'))

const Component = computed(() => (projectsV2.value ? V2 : Legacy))
</script>

<template>
  <component :is="Component" :id="props.id" />
</template>
