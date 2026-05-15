<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import type { Page } from '@/services/documentationService'
import { formatDate } from '@/utils/dateUtils'
import { docUrl } from '@/utils/docUrl'
import UserAvatar from '@/components/UserAvatar.vue'
import DocumentationChildCard from './DocumentationChildCard.vue'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const props = defineProps<{
  page: Page
}>()

const router = useRouter()

const navigateToPage = () => router.push(docUrl(props.page))

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
  if (freshnessClass.value === 'fresh') return t('docs-card-freshness-fresh')
  if (freshnessClass.value === 'recent') return t('docs-card-freshness-recent')
  return t('docs-card-freshness-stale')
})

// Format relative date
const formatRelativeDate = (dateStr: string | undefined) => {
  if (!dateStr) return t('docs-card-relative-unknown')
  const date = new Date(dateStr)
  const now = new Date()
  const diffMs = now.getTime() - date.getTime()
  const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24))

  if (diffDays === 0) return t('docs-card-relative-today')
  if (diffDays === 1) return t('docs-card-relative-yesterday')
  if (diffDays < 7) return t('docs-card-relative-days', { count: diffDays })
  if (diffDays < 30) return t('docs-card-relative-weeks', { count: Math.floor(diffDays / 7) })
  return formatDate(dateStr, 'MMM d')
}
</script>

<template>
  <article
    class="doc-card"
    role="link"
    tabindex="0"
    @click="navigateToPage"
    @keydown.enter="navigateToPage"
  >
    <!-- Card Content -->
    <div class="doc-card-content">
      <!-- Title with inline icon -->
      <div class="doc-card-title">
        <span class="icon-emoji">{{ page.icon || '📄' }}</span>
        <h3>{{ page.title }}</h3>
      </div>

      <!-- Content Preview -->
      <p v-if="contentPreview" class="doc-card-description">
        {{ contentPreview }}
      </p>
      <p v-else class="doc-card-description doc-card-description--empty">
        {{ $t('docs-card-empty-content') }}
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
        <span class="meta-separator">&middot;</span>

        <!-- Last Updated -->
        <span class="meta-date">{{ formatRelativeDate(page.updated_at || page.lastUpdated) }}</span>

        <!-- Children Count -->
        <template v-if="hasChildren">
          <span class="meta-separator">&middot;</span>
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

    <!-- Inline Children Section -->
    <div v-if="hasChildren" class="doc-card-children" @click.stop>
      <DocumentationChildCard
        v-for="child in page.children!.slice(0, 3)"
        :key="child.id"
        :page="child"
      />
      <span v-if="page.children!.length > 3" class="children-more">
        {{ $t('docs-card-children-more', { count: page.children!.length - 3 }) }}
      </span>
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
  cursor: pointer;

  /* Subtle glassmorphism */
  backdrop-filter: blur(8px);

  /* Transitions for hover effects */
  transition:
    transform 200ms ease,
    box-shadow 300ms ease,
    border-color 200ms ease;
}

/* Hover state */
.doc-card:hover {
  transform: translateY(-2px) scale(1.01);
  box-shadow:
    0 4px 20px rgba(0, 0, 0, 0.08),
    0 0 0 1px color-mix(in srgb, var(--color-accent) 30%, transparent);
  border-color: color-mix(in srgb, var(--color-accent) 40%, transparent);
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
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.icon-emoji {
  font-size: 1.25rem;
  line-height: 1;
  flex-shrink: 0;
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
  padding: 0.5rem 0.75rem 0.75rem;
  background: var(--color-surface-alt);
  display: flex;
  flex-direction: column;
  gap: 0.375rem;
}

.children-more {
  font-size: 0.75rem;
  color: var(--color-tertiary);
  padding: 0.25rem 0.75rem;
  font-weight: 500;
}

/* Animations */
@keyframes glow {
  0%, 100% {
    box-shadow: 0 0 4px currentColor;
  }
  50% {
    box-shadow: 0 0 8px currentColor;
  }
}

/* Focus state */
.doc-card:focus-visible {
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
    transition: none;
  }

  .doc-card:hover {
    transform: none;
  }

  .freshness-indicator {
    animation: none;
  }
}
</style>
