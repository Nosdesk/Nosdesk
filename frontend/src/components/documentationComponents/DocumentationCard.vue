<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink } from 'vue-router'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import type { Page } from '@/services/documentationService'
import { formatDate } from '@/utils/dateUtils'
import UserAvatar from '@/components/UserAvatar.vue'
import DocumentationChildCard from './DocumentationChildCard.vue'

const props = defineProps<{
  page: Page
  staggerIndex?: number
}>()

const docNavStore = useDocumentationNavStore()

// Expansion state
const isExpanded = computed(() => {
  return docNavStore.expandedPages[props.page.id]
})

// Has children
const hasChildren = computed(() => {
  return props.page.children && props.page.children.length > 0
})

// Author info (prefer last_edited_by, fallback to created_by)
const authorInfo = computed(() => {
  return props.page.last_edited_by || props.page.created_by
})

// Content preview - truncate the backend-provided plain text content
const contentPreview = computed(() => {
  const content = props.page.content
  if (!content) return null

  // Normalize whitespace and truncate to ~150 chars
  const normalized = content.replace(/\s+/g, ' ').trim()
  if (normalized.length > 150) {
    return normalized.slice(0, 150).trim() + '...'
  }
  return normalized || null
})

// Stagger animation style
const staggerStyle = computed(() => ({
  '--stagger-delay': `${(props.staggerIndex ?? 0) * 50}ms`
}))

// Freshness calculation (how recently updated)
const freshnessClass = computed(() => {
  const updated = new Date(props.page.updated_at || props.page.lastUpdated || Date.now())
  const now = new Date()
  const hoursDiff = (now.getTime() - updated.getTime()) / (1000 * 60 * 60)

  if (hoursDiff < 24) return 'fresh'      // Updated within 24 hours
  if (hoursDiff < 168) return 'recent'    // Updated within a week
  return 'stale'
})

const freshnessTitle = computed(() => {
  if (freshnessClass.value === 'fresh') return 'Updated recently'
  if (freshnessClass.value === 'recent') return 'Updated this week'
  return 'Not updated recently'
})

// Format relative date
const formatRelativeDate = (dateStr: string | undefined) => {
  if (!dateStr) return 'Unknown'
  const date = new Date(dateStr)
  const now = new Date()
  const diffMs = now.getTime() - date.getTime()
  const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24))

  if (diffDays === 0) return 'Today'
  if (diffDays === 1) return 'Yesterday'
  if (diffDays < 7) return `${diffDays}d ago`
  if (diffDays < 30) return `${Math.floor(diffDays / 7)}w ago`
  return formatDate(dateStr, 'MMM d')
}

// Toggle children expansion
const toggleExpand = (event: Event) => {
  event.preventDefault()
  event.stopPropagation()
  docNavStore.togglePage(String(props.page.id))
}
</script>

<template>
  <article
    class="doc-card"
    :class="{ 'is-expanded': isExpanded }"
    :style="staggerStyle"
  >
    <!-- Card Header with Icon -->
    <RouterLink :to="`/documentation/${page.id}`" class="doc-card-header">
      <!-- Large Icon Area -->
      <div class="doc-card-icon">
        <span class="icon-emoji">{{ page.icon || '📄' }}</span>
      </div>
    </RouterLink>

    <!-- Card Content -->
    <div class="doc-card-content">
      <!-- Title -->
      <RouterLink :to="`/documentation/${page.id}`" class="doc-card-title">
        <h3>{{ page.title }}</h3>
      </RouterLink>

      <!-- Content Preview -->
      <p v-if="contentPreview" class="doc-card-description">
        {{ contentPreview }}
      </p>
      <p v-else class="doc-card-description doc-card-description--empty">
        No content yet
      </p>

      <!-- Metadata Row -->
      <div class="doc-card-meta">
        <!-- Author Avatar -->
        <UserAvatar
          v-if="authorInfo"
          :name="authorInfo.uuid"
          :user-name="authorInfo.name"
          :avatar="authorInfo.avatar_thumb || authorInfo.avatar_url"
          size="xxs"
          :show-name="false"
          :clickable="false"
        />
        <span v-if="authorInfo" class="meta-author">{{ authorInfo.name }}</span>

        <!-- Separator -->
        <span class="meta-separator">·</span>

        <!-- Last Updated -->
        <span class="meta-date">{{ formatRelativeDate(page.updated_at || page.lastUpdated) }}</span>

        <!-- Children Count -->
        <template v-if="hasChildren">
          <span class="meta-separator">·</span>
          <span class="meta-children">
            <svg class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
            </svg>
            {{ page.children?.length }}
          </span>
        </template>

        <!-- Freshness Indicator -->
        <div
          class="freshness-indicator"
          :class="freshnessClass"
          :title="freshnessTitle"
        ></div>
      </div>
    </div>

    <!-- Expandable Children Section -->
    <div v-if="hasChildren" class="doc-card-children">
      <button @click="toggleExpand" class="children-toggle">
        <svg
          class="chevron"
          :class="{ 'rotate-180': isExpanded }"
          width="16"
          height="16"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <path d="M6 9l6 6 6-6" />
        </svg>
        <span>{{ page.children?.length }} sub-page{{ page.children?.length === 1 ? '' : 's' }}</span>
      </button>

      <Transition name="expand">
        <div v-if="isExpanded" class="children-list">
          <DocumentationChildCard
            v-for="(child, index) in page.children"
            :key="child.id"
            :page="child"
            :stagger-index="index"
          />
        </div>
      </Transition>
    </div>
  </article>
</template>

<style scoped>
/* Card Container */
.doc-card {
  background: var(--color-surface);
  border: 1px solid var(--color-default);
  border-radius: 1rem;
  overflow: hidden;
  display: flex;
  flex-direction: column;

  /* Subtle glassmorphism */
  backdrop-filter: blur(8px);

  /* Transitions */
  transition:
    transform 200ms ease,
    box-shadow 300ms ease,
    border-color 200ms ease;

  /* Stagger animation */
  animation: cardFadeIn 300ms ease-out forwards;
  animation-delay: var(--stagger-delay, 0ms);
  opacity: 0;
}

/* Hover state */
.doc-card:hover {
  transform: translateY(-2px) scale(1.01);
  box-shadow:
    0 4px 20px rgba(0, 0, 0, 0.08),
    0 0 0 1px color-mix(in srgb, var(--color-accent) 30%, transparent);
  border-color: color-mix(in srgb, var(--color-accent) 40%, transparent);
}

/* Card Header */
.doc-card-header {
  position: relative;
  display: block;
  text-decoration: none;
}

.doc-card-icon {
  width: 100%;
  height: 5rem;
  display: flex;
  align-items: center;
  justify-content: center;
  background: linear-gradient(
    135deg,
    color-mix(in srgb, var(--color-accent-muted) 50%, transparent) 0%,
    var(--color-surface-alt) 100%
  );
  transition: background 300ms ease;
}

.doc-card:hover .doc-card-icon {
  background: linear-gradient(
    135deg,
    color-mix(in srgb, var(--color-accent) 20%, transparent) 0%,
    color-mix(in srgb, var(--color-accent-muted) 60%, transparent) 50%,
    var(--color-surface-alt) 100%
  );
}

.icon-emoji {
  font-size: 2.5rem;
  line-height: 1;
  filter: drop-shadow(0 2px 4px rgba(0, 0, 0, 0.1));
  transition: transform 200ms ease;
}

.doc-card:hover .icon-emoji {
  transform: scale(1.1);
}

/* Card Content */
.doc-card-content {
  padding: 1rem 1.25rem;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  flex: 1;
}

.doc-card-title {
  text-decoration: none;
}

.doc-card-title h3 {
  font-size: 1rem;
  font-weight: 600;
  color: var(--color-primary);
  line-height: 1.4;
  margin: 0;
  transition: color 150ms ease;

  /* 2-line clamp */
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.doc-card:hover .doc-card-title h3 {
  color: var(--color-accent);
}

.doc-card-description {
  font-size: 0.8125rem;
  color: var(--color-secondary);
  line-height: 1.5;
  margin: 0;

  /* 3-line clamp */
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.doc-card-description--empty {
  color: var(--color-tertiary);
  font-style: italic;
}

/* Metadata Row */
.doc-card-meta {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin-top: auto;
  padding-top: 0.75rem;
  border-top: 1px solid var(--color-subtle);
  font-size: 0.6875rem;
  color: var(--color-tertiary);
}

.meta-author {
  max-width: 80px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.meta-separator {
  opacity: 0.5;
}

.meta-children {
  display: flex;
  align-items: center;
  gap: 0.25rem;
}

/* Freshness Indicator */
.freshness-indicator {
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 50%;
  margin-left: auto;
  flex-shrink: 0;
}

.freshness-indicator.fresh {
  background: var(--color-status-success);
  box-shadow: 0 0 6px var(--color-status-success);
  animation: glow 2s ease-in-out infinite;
}

.freshness-indicator.recent {
  background: var(--color-status-info);
}

.freshness-indicator.stale {
  background: var(--color-tertiary);
  opacity: 0.5;
}

/* Children Section */
.doc-card-children {
  border-top: 1px solid var(--color-subtle);
}

.children-toggle {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  width: 100%;
  padding: 0.75rem 1.25rem;
  background: var(--color-surface-alt);
  border: none;
  color: var(--color-secondary);
  font-size: 0.75rem;
  font-weight: 500;
  cursor: pointer;
  transition: background 150ms ease, color 150ms ease;
}

.children-toggle:hover {
  background: var(--color-surface-hover);
  color: var(--color-primary);
}

.children-toggle .chevron {
  transition: transform 200ms ease;
}

.children-list {
  padding: 0.5rem 0.75rem 0.75rem;
  background: var(--color-surface-alt);
  display: flex;
  flex-direction: column;
  gap: 0.375rem;
}

/* Animations */
@keyframes cardFadeIn {
  from {
    opacity: 0;
    transform: translateY(12px) scale(0.98);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}

@keyframes glow {
  0%, 100% {
    box-shadow: 0 0 4px currentColor;
  }
  50% {
    box-shadow: 0 0 8px currentColor;
  }
}

/* Expand/collapse transition */
.expand-enter-active {
  transition: all 300ms ease-out;
  overflow: hidden;
}

.expand-leave-active {
  transition: all 200ms ease-in;
  overflow: hidden;
}

.expand-enter-from,
.expand-leave-to {
  opacity: 0;
  max-height: 0;
  padding-top: 0;
  padding-bottom: 0;
}

.expand-enter-to,
.expand-leave-from {
  max-height: 500px;
}

/* Focus state */
.doc-card:focus-within {
  outline: 2px solid var(--color-accent);
  outline-offset: 2px;
}

/* ========== Theme-Specific Enhancements ========== */

/* Dark theme - Enhanced glassmorphism */
.dark .doc-card {
  background: color-mix(in srgb, var(--color-surface) 90%, transparent);
  box-shadow:
    0 1px 2px rgba(0, 0, 0, 0.1),
    inset 0 1px 0 rgba(255, 255, 255, 0.05);
}

.dark .doc-card:hover {
  box-shadow:
    0 8px 32px rgba(0, 0, 0, 0.2),
    0 0 0 1px color-mix(in srgb, var(--color-accent) 40%, transparent),
    inset 0 1px 0 rgba(255, 255, 255, 0.08);
}

/* Red Horizon theme - Futuristic glow */
[data-theme="red-horizon"] .doc-card {
  border-color: rgba(255, 100, 50, 0.15);
}

[data-theme="red-horizon"] .doc-card:hover {
  box-shadow:
    0 4px 20px rgba(200, 80, 0, 0.2),
    0 0 0 1px rgba(255, 100, 50, 0.3);
}

[data-theme="red-horizon"] .freshness-indicator.fresh {
  box-shadow: 0 0 10px rgba(255, 136, 68, 0.8);
}

/* E-Paper theme - Flat, print-like style */
[data-theme="epaper"] .doc-card {
  border-radius: 0.25rem;
  box-shadow: none;
  backdrop-filter: none;
}

[data-theme="epaper"] .doc-card:hover {
  transform: none;
  box-shadow: none;
  border-color: var(--color-primary);
}

[data-theme="epaper"] .doc-card-icon {
  background: var(--color-surface-alt);
}

[data-theme="epaper"] .freshness-indicator {
  animation: none;
  box-shadow: none;
}

/* High contrast mode support */
@media (prefers-contrast: high) {
  .doc-card {
    border-width: 2px;
  }

  .doc-card:hover {
    border-color: var(--color-accent);
  }
}

/* Reduced motion support */
@media (prefers-reduced-motion: reduce) {
  .doc-card {
    animation: none;
    opacity: 1;
  }

  .doc-card:hover {
    transform: none;
  }

  .icon-emoji {
    transition: none;
  }

  .freshness-indicator {
    animation: none;
  }

  .children-toggle .chevron {
    transition: none;
  }
}
</style>
