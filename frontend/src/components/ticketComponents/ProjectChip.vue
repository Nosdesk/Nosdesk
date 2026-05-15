<script setup lang="ts">
/**
 * ProjectChip — chip wrapper that resolves a project id to its
 * name for the property-list surface. Falls back to `Project
 * #{id}` while loading.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import projectService from '@/services/projectService'
import PropertyChip from '@/components/ticketComponents/PropertyChip.vue'

const props = defineProps<{
  projectId: string
}>()

const emit = defineEmits<{
  (e: 'remove', id: string): void
}>()

const name = ref<string | null>(null)
const loading = ref(true)

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const chipLabel = computed(() => name.value || t('ticket-field-projects-fallback', { id: props.projectId }))
const removeTitle = computed(() => t('ticket-field-projects-remove'))

watch(
  () => props.projectId,
  async (id) => {
    loading.value = true
    name.value = null
    try {
      const fetched = await projectService.getProject(Number(id))
      name.value = fetched?.name ?? null
    } catch {
      name.value = null
    } finally {
      loading.value = false
    }
  },
  { immediate: true },
)
</script>

<template>
  <PropertyChip
    :label="chipLabel"
    :title="chipLabel"
    :to="`/projects/${projectId}`"
    :loading="loading"
    removable
    :remove-title="removeTitle"
    @remove="emit('remove', projectId)"
  />
</template>
