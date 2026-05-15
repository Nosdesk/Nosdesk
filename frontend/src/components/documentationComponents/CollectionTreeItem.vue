<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink, useRoute } from 'vue-router'
import { useFluent } from 'fluent-vue'
import type { CollectionPageTreeNode } from '@/services/collectionService'
import { docUrl } from '@/utils/docUrl'

useFluent()

const props = defineProps<{
  node: CollectionPageTreeNode
  level: number
  overridePageIds?: Set<number>
}>()

const route = useRoute()

const hasChildren = computed(() => props.node.children && props.node.children.length > 0)
const isActive = computed(() => route.path === docUrl(props.node))
const currentLevel = computed(() => props.level)
const hasOverride = computed(() => props.overridePageIds?.has(props.node.id) ?? false)
</script>

<template>
  <li
    class="tree-item relative select-none"
    :style="{ '--level': currentLevel }"
  >
    <!-- Main Item -->
    <div
      class="group relative flex items-center py-1.5 sm:py-2 pr-3 rounded-lg text-sm transition-all duration-150"
      :class="[
        isActive
          ? 'bg-accent/8 text-primary font-medium ring-1 ring-accent/20'
          : 'text-secondary hover:text-primary hover:bg-surface-hover',
      ]"
    >
      <!-- Indent spacing -->
      <span class="flex-shrink-0" :style="{ width: `${12 + currentLevel * 20}px` }"></span>

      <!-- Page icon -->
      <span class="flex-shrink-0 w-6 h-6 flex items-center justify-center">
        <span class="text-base leading-none">{{ node.icon || '📄' }}</span>
      </span>

      <!-- Page title link -->
      <RouterLink
        :to="docUrl(node)"
        class="flex-1 min-w-0 truncate ml-1.5"
        :class="isActive ? 'text-accent font-semibold' : 'hover:text-accent'"
      >
        {{ node.title || $t('docs-collection-tree-item-untitled') }}
      </RouterLink>

      <!-- Child count badge (on hover) -->
      <span
        v-if="hasChildren"
        class="flex-shrink-0 text-[10px] text-tertiary ml-1.5 tabular-nums opacity-0 group-hover:opacity-100 transition-opacity"
      >
        {{ node.children.length }}
      </span>

      <!-- Draft badge -->
      <span
        v-if="node.status === 'draft'"
        class="flex-shrink-0 text-[9px] px-1.5 py-0.5 rounded-full bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-300 font-medium ml-1.5"
      >
        {{ $t('docs-collection-tree-item-draft') }}
      </span>

      <!-- Override lock icon -->
      <svg
        v-if="hasOverride"
        class="flex-shrink-0 w-3.5 h-3.5 text-status-warning ml-1"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
        :title="$t('docs-collection-tree-item-override-title')"
      >
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
      </svg>
    </div>

    <!-- Children (recursive, with tree guide line) -->
    <ul
      v-if="hasChildren"
      class="tree-children flex flex-col"
      :style="{ '--parent-level': currentLevel }"
    >
      <CollectionTreeItem
        v-for="child in node.children"
        :key="child.id"
        :node="child"
        :level="level + 1"
        :overridePageIds="overridePageIds"
      />
    </ul>
  </li>
</template>

<style scoped>
/* Tree guide line connecting parent to children */
.tree-children {
  position: relative;
}

.tree-children::before {
  content: '';
  position: absolute;
  top: 2px;
  bottom: 10px;
  /* Position: 12px base + (parent-level * 20px) + 12px to center under icon */
  left: calc(12px + var(--parent-level, 0) * 20px + 12px);
  width: 1px;
  background-color: currentColor;
  opacity: 0.1;
  border-radius: 1px;
}
</style>
