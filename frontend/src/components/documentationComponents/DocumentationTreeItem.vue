<script setup lang="ts">
import { RouterLink } from 'vue-router'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import type { Page } from '@/services/documentationService'
import { computed } from 'vue'
import { formatDate } from '@/utils/dateUtils'

const props = defineProps<{
  page: Page
  level?: number
  staggerIndex?: number
}>()

const docNavStore = useDocumentationNavStore()

// Current level (0-based)
const currentLevel = computed(() => props.level ?? 0)

// Check if this page has children
const hasChildren = computed(() => {
  return props.page.children && props.page.children.length > 0
})

// Check if this page is expanded
const isExpanded = computed(() => {
  return docNavStore.expandedPages[props.page.id]
})

// Toggle expansion
const toggleExpand = (event: Event) => {
  event.preventDefault()
  event.stopPropagation()
  docNavStore.togglePage(String(props.page.id))
}

// Calculate stagger style for animations
const staggerStyle = computed(() => ({
  '--stagger-delay': `${(props.staggerIndex ?? 0) * 30}ms`,
  '--level': currentLevel.value
}))

// Author display name
const authorName = computed(() => {
  return props.page.last_edited_by?.name || props.page.created_by?.name || props.page.author
})
</script>

<template>
  <div class="doc-tree-item" :style="staggerStyle">
    <!-- Main Item Row -->
    <RouterLink
      :to="`/documentation/${page.id}`"
      class="doc-row group"
      :class="{ 'has-children': hasChildren }"
    >
      <!-- Expand/Collapse Button -->
      <button
        v-if="hasChildren"
        @click="toggleExpand"
        class="expand-btn"
        :aria-label="isExpanded ? 'Collapse' : 'Expand'"
      >
        <svg
          class="w-3.5 h-3.5 transition-transform duration-200"
          :class="{ 'rotate-90': isExpanded }"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
          stroke-width="2.5"
        >
          <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
        </svg>
      </button>
      <span v-else class="expand-placeholder"></span>

      <!-- Icon -->
      <span class="doc-icon">{{ page.icon || '📄' }}</span>

      <!-- Title & Meta -->
      <div class="flex-1 min-w-0 flex items-center gap-3">
        <h3 class="text-sm font-medium text-primary group-hover:text-accent transition-colors truncate">
          {{ page.title }}
        </h3>

        <!-- Inline metadata for density -->
        <div class="hidden sm:flex items-center gap-2 text-xs text-tertiary flex-shrink-0">
          <span>{{ formatDate(page.updated_at || page.lastUpdated || new Date().toISOString()) }}</span>
          <template v-if="authorName">
            <span class="text-subtle">·</span>
            <span class="truncate max-w-[100px]">{{ authorName }}</span>
          </template>
        </div>
      </div>

      <!-- Right side indicators -->
      <div class="flex items-center gap-2 flex-shrink-0">
        <!-- Children count badge -->
        <span
          v-if="hasChildren"
          class="text-[10px] px-1.5 py-0.5 rounded-full bg-accent/10 text-accent font-medium"
        >
          {{ page.children?.length }}
        </span>

        <!-- Arrow indicator -->
        <svg
          class="w-4 h-4 text-tertiary opacity-0 group-hover:opacity-100 transition-opacity"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
          stroke-width="2"
        >
          <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
        </svg>
      </div>
    </RouterLink>

    <!-- Description (only for top-level items) -->
    <p
      v-if="page.description && currentLevel === 0"
      class="doc-description"
    >
      {{ page.description }}
    </p>

    <!-- Children Container -->
    <div
      v-if="hasChildren && isExpanded"
      class="doc-children"
    >
      <TransitionGroup name="tree-expand" tag="div" class="space-y-0.5">
        <DocumentationTreeItem
          v-for="(child, index) in page.children"
          :key="child.id"
          :page="child"
          :level="currentLevel + 1"
          :stagger-index="index"
        />
      </TransitionGroup>
    </div>
  </div>
</template>

<style scoped>
/* Main container with stagger animation */
.doc-tree-item {
  animation: fadeSlideIn 200ms ease-out forwards;
  animation-delay: var(--stagger-delay, 0ms);
  opacity: 0;
  /* Indent based on level */
  margin-left: calc(var(--level, 0) * 20px);
}

@keyframes fadeSlideIn {
  from {
    opacity: 0;
    transform: translateY(6px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

/* Main row - compact, clean layout */
.doc-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.625rem 0.75rem;
  border-radius: 0.5rem;
  background: var(--color-bg-surface, #1f2937);
  border: 1px solid var(--color-border-default, rgba(255,255,255,0.1));
  transition: all 150ms ease;
  text-decoration: none;
}

.doc-row:hover {
  background: var(--color-bg-surface-hover, #374151);
  border-color: var(--color-accent, #3b82f6);
  border-color: color-mix(in srgb, var(--color-accent, #3b82f6) 40%, transparent);
}

.doc-row:focus-visible {
  outline: 2px solid var(--color-accent, #3b82f6);
  outline-offset: 2px;
}

/* Expand button */
.expand-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 1.25rem;
  height: 1.25rem;
  border-radius: 0.25rem;
  color: var(--color-text-tertiary, #9ca3af);
  background: transparent;
  border: none;
  cursor: pointer;
  flex-shrink: 0;
  transition: all 150ms ease;
}

.expand-btn:hover {
  color: var(--color-text-primary, #f9fafb);
  background: var(--color-bg-surface-alt, #374151);
}

.expand-placeholder {
  width: 1.25rem;
  flex-shrink: 0;
}

/* Document icon */
.doc-icon {
  font-size: 1.125rem;
  line-height: 1;
  flex-shrink: 0;
  width: 1.5rem;
  text-align: center;
}

/* Description - subtle, below the row */
.doc-description {
  margin-top: 0.25rem;
  margin-left: calc(1.25rem + 0.5rem + 1.5rem + 0.5rem + 0.75rem);
  padding-right: 0.75rem;
  font-size: 0.8125rem;
  color: var(--color-text-secondary, #9ca3af);
  line-height: 1.5;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

/* Children container with indent line */
.doc-children {
  position: relative;
  margin-top: 0.25rem;
  padding-left: 0.625rem;
}

/* Vertical line for children */
.doc-children::before {
  content: '';
  position: absolute;
  left: calc(0.625rem + 2px);
  top: 0;
  bottom: 0.5rem;
  width: 1px;
  background: currentColor;
  opacity: 0.15;
  border-radius: 1px;
}

/* Tree expand/collapse animation */
.tree-expand-enter-active {
  transition: all 200ms ease-out;
}

.tree-expand-leave-active {
  transition: all 100ms ease-in;
}

.tree-expand-enter-from {
  opacity: 0;
  transform: translateX(-4px);
}

.tree-expand-leave-to {
  opacity: 0;
  transform: translateX(-4px);
}
</style>
