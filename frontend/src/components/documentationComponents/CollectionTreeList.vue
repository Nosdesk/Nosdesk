<script setup lang="ts">
/**
 * The pages of a collection as a tree, on Reka's Tree. Reka owns the
 * `role=tree` of `treeitem` rows with `aria-level`, `aria-setsize` and
 * `aria-posinset`, roving focus with Up/Down/Home/End, Left/Right to
 * collapse and expand (and to step to the parent or first child),
 * Enter/Space to activate, and typeahead on the page title.
 *
 * Each row is a link (the treeitem is the anchor), so plain clicks
 * navigate through the router and modifier or middle clicks open a
 * new tab; the chevron on a parent row is the pointer affordance for
 * collapsing. Selection is the current route, never Reka's own model.
 * Every parent opens the first time it is seen (the tree reads as
 * fully open, as before), and a collapse the user makes persists while
 * the view lives.
 */
import { computed, ref, watch } from 'vue'
import { RouterLink, useRoute } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { TreeItem, TreeRoot, type TreeItemSelectEvent, type TreeItemToggleEvent } from 'reka-ui'
import type { CollectionPage, CollectionPageTreeNode } from '@nosdesk/core/services/collectionService'
import { docUrl } from '@nosdesk/core/utils/docUrl'
import { buildCollectionTree } from '@/utils/treeUtils'
import Icon from '@/components/common/Icon.vue'

useFluent()

const props = defineProps<{
  pages: CollectionPage[]
  overridePageIds?: Set<number>
}>()

const route = useRoute()

type TreeNode = CollectionPageTreeNode

const tree = computed(() => buildCollectionTree(props.pages))

const keyOf = (node: TreeNode) => String(node.id)
const childrenOf = (node: TreeNode) => (node.children.length ? node.children : undefined)

function flatten(nodes: TreeNode[]): TreeNode[] {
  return nodes.flatMap((node) => [node, ...flatten(node.children)])
}

// Parents open when first seen (pages arrive after mount); a collapse
// the user makes is kept.
const expanded = ref<string[]>([])
const seen = new Set<string>()
watch(
  tree,
  (nodes) => {
    const fresh = flatten(nodes)
      .filter((node) => node.children.length && !seen.has(keyOf(node)))
      .map(keyOf)
    fresh.forEach((key) => seen.add(key))
    if (fresh.length) expanded.value = [...expanded.value, ...fresh]
  },
  { immediate: true },
)

const active = computed(() => flatten(tree.value).find((node) => docUrl(node) === route.path))

// A row click navigates; only the chevron and the arrow keys toggle.
function onToggle(event: TreeItemToggleEvent<TreeNode>) {
  if (event.detail.originalEvent.type === 'click') event.preventDefault()
}

// Selection is the route: Reka never writes it. Enter/Space follow the
// link; a click already did through the router.
function onSelect(event: TreeItemSelectEvent<TreeNode>, navigate: () => void) {
  event.preventDefault()
  if (event.detail.originalEvent instanceof KeyboardEvent) navigate()
}

const hasOverride = (node: TreeNode) => props.overridePageIds?.has(node.id) ?? false
</script>

<template>
  <div class="collection-tree">
    <!-- Empty state -->
    <div v-if="tree.length === 0" class="text-center py-8 text-tertiary text-sm">
      {{ $t('docs-collection-tree-list-empty') }}
    </div>

    <TreeRoot
      v-else
      v-slot="{ flattenItems }"
      v-model:expanded="expanded"
      :items="tree"
      :get-key="keyOf"
      :get-children="childrenOf"
      :model-value="active"
      :aria-label="$t('docs-collection-tree-aria')"
      class="flex flex-col"
    >
      <RouterLink v-for="item in flattenItems" :key="item._id" v-slot="{ href, navigate }" :to="docUrl(item.value)" custom>
        <TreeItem
          v-slot="{ isExpanded, handleToggle }"
          v-bind="item.bind"
          as-child
          @toggle="onToggle"
          @select="onSelect($event, navigate)"
        >
          <a
            :href="href"
            class="tree-row group relative flex items-center py-1.5 sm:py-2 pr-3 rounded-lg text-sm transition-all duration-150 outline-none focus-visible:ring-2 focus-visible:ring-accent"
            :class="[
              item.level > 1 && 'tree-row--nested',
              item.value === active
                ? 'bg-accent/8 text-primary font-medium ring-1 ring-accent/20'
                : 'text-secondary hover:text-primary hover:bg-surface-hover',
            ]"
            :style="{ '--indent': `${16 + (item.level - 1) * 20}px` }"
            @click="navigate"
          >
            <!-- The title comes first in the DOM so Reka's typeahead (which
                 reads the row text) matches on it; the indent and icon are
                 ordered ahead of it visually. -->
            <span class="flex-1 min-w-0 truncate ml-1.5" :class="item.value === active ? 'text-accent font-semibold' : 'group-hover:text-accent'">
              {{ item.value.title || $t('docs-collection-tree-item-untitled') }}
            </span>

            <!-- Indent; nested rows draw a guide line under the parent's icon. -->
            <span class="flex-shrink-0 order-first" :style="{ width: 'var(--indent)' }" aria-hidden="true"></span>

            <!-- Chevron: pointer affordance for collapsing; the keys do
                 the same for keyboard users. -->
            <span
              v-if="item.hasChildren"
              class="absolute w-4 h-4 flex items-center justify-center text-tertiary hover:text-primary rounded transition-transform"
              :class="{ '-rotate-90': !isExpanded }"
              :style="{ left: 'calc(var(--indent) - 16px)' }"
              aria-hidden="true"
              @click.stop.prevent="handleToggle()"
            >
              <Icon name="chevronDown" class="w-3 h-3" />
            </span>

            <!-- Page icon -->
            <span class="flex-shrink-0 w-6 h-6 flex items-center justify-center order-first" aria-hidden="true">
              <span class="text-base leading-none">{{ item.value.icon || '📄' }}</span>
            </span>

            <!-- Child count badge (on hover) -->
            <span
              v-if="item.hasChildren"
              class="flex-shrink-0 text-3xs text-tertiary ml-1.5 tabular-nums opacity-0 group-hover:opacity-100 transition-opacity"
              aria-hidden="true"
            >
              {{ item.value.children.length }}
            </span>

            <!-- Draft badge -->
            <span
              v-if="item.value.status === 'draft'"
              class="flex-shrink-0 text-4xs px-1.5 py-0.5 rounded-full bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-300 font-medium ml-1.5"
            >
              {{ $t('docs-collection-tree-item-draft') }}
            </span>

            <!-- Override lock icon -->
            <svg
              v-if="hasOverride(item.value)"
              class="flex-shrink-0 w-3.5 h-3.5 text-status-warning ml-1"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
              role="img"
              :aria-label="$t('docs-collection-tree-item-override-title')"
            >
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
            </svg>
          </a>
        </TreeItem>
      </RouterLink>
    </TreeRoot>
  </div>
</template>

<style scoped>
/* Guide line centred under the parent's icon (the parent's indent is
   20px less than this row's; its icon is 24px wide), continuous across
   siblings because every row draws its own segment. */
.tree-row--nested::before {
  content: '';
  position: absolute;
  top: 0;
  bottom: 0;
  left: calc(var(--indent) - 20px + 12px);
  width: 1px;
  background-color: currentColor;
  opacity: 0.1;
}
</style>
