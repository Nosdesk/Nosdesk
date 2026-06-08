<script setup lang="ts">
import { RouterLink } from 'vue-router'
import CollectionIcon from '@/components/documentationComponents/CollectionIcon.vue'

interface Collection {
  id: number
  name: string
  slug: string
  icon: string | null
  color: string | null
}

defineProps<{
  collections: Collection[]
}>()
</script>

<template>
  <div v-if="collections.length > 0" class="flex flex-wrap items-center gap-1.5">
    <RouterLink
      v-for="collection in collections"
      :key="collection.id"
      :to="`/documentation/collections/${collection.slug}`"
      class="collection-badge"
      :style="collection.color ? { '--badge-color': collection.color } : {}"
    >
      <CollectionIcon
        :icon="collection.icon"
        :color="collection.color"
        size="xs"
      />
      <span class="truncate">{{ collection.name }}</span>
    </RouterLink>
  </div>
</template>

<style scoped>
.collection-badge {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
  padding: 0.125rem 0.5rem 0.125rem 0.25rem;
  border-radius: 9999px;
  font-size: 0.75rem;
  background: var(--color-surface-alt);
  color: var(--color-secondary);
  transition: color 0.15s, background-color 0.15s;
  cursor: pointer;
  max-width: 150px;
  border: 1px solid var(--badge-color, transparent);
}

.collection-badge:hover {
  color: var(--color-primary);
  background: var(--color-surface-hover);
  border-color: var(--badge-color, var(--color-border-default));
}
</style>
