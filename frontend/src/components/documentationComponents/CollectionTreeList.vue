<script setup lang="ts">
import { computed } from 'vue'
import type { CollectionPage } from '@/services/collectionService'
import { buildCollectionTree } from '@/utils/treeUtils'
import CollectionTreeItem from './CollectionTreeItem.vue'
import { useStaggeredList } from '@/composables/useStaggeredList'

const props = defineProps<{
  pages: CollectionPage[]
  overridePageIds?: Set<number>
}>()

const tree = computed(() => buildCollectionTree(props.pages))

// Staggered animation for tree items
const { getStyle } = useStaggeredList({
  staggerDelay: 30,
  maxStaggerItems: 20
})
</script>

<template>
  <div class="collection-tree">
    <!-- Empty state -->
    <div v-if="tree.length === 0" class="text-center py-8 text-tertiary text-sm">
      No pages in this collection yet.
    </div>

    <div v-else>
      <ul class="flex flex-col">
        <li
          v-for="(node, index) in tree"
          :key="node.id"
          :style="getStyle(index)"
          class="tree-item-animate"
        >
          <CollectionTreeItem
            :node="node"
            :level="0"
            :overridePageIds="overridePageIds"
          />
        </li>
      </ul>
    </div>
  </div>
</template>

<style scoped>
/* Staggered item animation */
.tree-item-animate {
  animation: treeFadeIn var(--animation-duration, 150ms) ease-out forwards;
  animation-delay: var(--stagger-delay, 0ms);
  opacity: 0;
  transform: translateY(4px);
}

@keyframes treeFadeIn {
  from {
    opacity: 0;
    transform: translateY(4px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

@media (prefers-reduced-motion: reduce) {
  .tree-item-animate {
    animation: none;
    opacity: 1;
    transform: none;
  }
}
</style>
