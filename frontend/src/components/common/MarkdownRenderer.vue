<script setup lang="ts">
/**
 * MarkdownRenderer - Renders markdown content with mention and ticket support
 *
 * Supports:
 * - Standard markdown (bold, italic, links, lists, code, etc.)
 * - @mentions rendered as styled chips with avatars
 * - Ticket references rendered as interactive cards
 * - Safe HTML via DOMPurify
 */
import { computed, ref, watch, nextTick } from 'vue';
import { marked } from 'marked';
import DOMPurify from 'dompurify';
import { enhanceMentions } from '@/plugins/prosemirror-mention-view';
import { enhanceTicketLinks } from '@/components/editor/ticketLinkPlugin';

const props = defineProps<{
  content: string;
  class?: string;
}>();

const containerRef = ref<HTMLElement | null>(null);

// Custom renderer for mentions
const mentionRegex = /@\[([^\]]+)\]\(([a-f0-9-]+)\)/g;

// Process mentions before markdown parsing
const preprocessMentions = (text: string): string => {
  return text.replace(mentionRegex, (_, name, uuid) => {
    // Convert to a special HTML span that will survive markdown parsing
    return `<span class="mention" data-uuid="${uuid}">@${name}</span>`;
  });
};

// Configure marked for safe rendering
marked.setOptions({
  gfm: true,
  breaks: true,
});

// Render content
const renderedHtml = computed(() => {
  if (!props.content) return '';

  // First, preprocess mentions
  const withMentions = preprocessMentions(props.content);

  // Parse markdown
  const html = marked.parse(withMentions) as string;

  // Sanitize HTML but allow our mention spans
  const clean = DOMPurify.sanitize(html, {
    ALLOWED_TAGS: [
      'p', 'br', 'strong', 'b', 'em', 'i', 'u', 's', 'del',
      'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
      'ul', 'ol', 'li',
      'blockquote', 'pre', 'code',
      'a', 'img',
      'table', 'thead', 'tbody', 'tr', 'th', 'td',
      'hr', 'span',
    ],
    ALLOWED_ATTR: [
      'href', 'src', 'alt', 'title', 'class', 'target', 'rel', 'contenteditable',
      // Mention attributes
      'data-uuid', 'data-mention', 'data-name', 'data-avatar-url',
      // Ticket link attributes
      'data-ticket-link', 'data-ticket-id', 'data-href',
    ],
  });

  return clean;
});

// Enhance mentions and ticket links after content renders
watch(renderedHtml, () => {
  nextTick(() => {
    if (containerRef.value) {
      enhanceMentions(containerRef.value);
      enhanceTicketLinks(containerRef.value);
    }
  });
}, { immediate: true });
</script>

<template>
  <div
    ref="containerRef"
    class="markdown-content"
    :class="props.class"
    v-html="renderedHtml"
  />
</template>

<style scoped>
.markdown-content {
  font-size: 0.875rem;
  line-height: 1.625;
  color: var(--color-primary);
}

.markdown-content :deep(p) {
  margin-bottom: 0.5rem;
}

.markdown-content :deep(p:last-child) {
  margin-bottom: 0;
}

.markdown-content :deep(strong),
.markdown-content :deep(b) {
  font-weight: 600;
}

.markdown-content :deep(em),
.markdown-content :deep(i) {
  font-style: italic;
}

.markdown-content :deep(a) {
  color: var(--color-accent);
  text-decoration: underline;
}

.markdown-content :deep(a:hover) {
  color: var(--color-accent-hover);
}

.markdown-content :deep(code) {
  background-color: var(--color-surface-alt);
  padding: 0.125rem 0.375rem;
  border-radius: 0.25rem;
  font-size: 0.75rem;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.markdown-content :deep(pre) {
  background-color: var(--color-surface-alt);
  padding: 0.75rem;
  border-radius: 0.5rem;
  overflow-x: auto;
  margin-bottom: 0.5rem;
}

.markdown-content :deep(pre code) {
  background-color: transparent;
  padding: 0;
}

.markdown-content :deep(blockquote) {
  border-left: 4px solid var(--color-default);
  padding-left: 1rem;
  font-style: italic;
  color: var(--color-secondary);
  margin: 0.5rem 0;
}

.markdown-content :deep(ul),
.markdown-content :deep(ol) {
  padding-left: 1.25rem;
  margin-bottom: 0.5rem;
}

.markdown-content :deep(ul) {
  list-style-type: disc;
}

.markdown-content :deep(ol) {
  list-style-type: decimal;
}

.markdown-content :deep(li) {
  font-size: 0.875rem;
  margin-bottom: 0.25rem;
}

.markdown-content :deep(h1),
.markdown-content :deep(h2),
.markdown-content :deep(h3),
.markdown-content :deep(h4) {
  font-weight: 600;
  color: var(--color-primary);
  margin-bottom: 0.5rem;
  margin-top: 0.75rem;
}

.markdown-content :deep(h1:first-child),
.markdown-content :deep(h2:first-child),
.markdown-content :deep(h3:first-child),
.markdown-content :deep(h4:first-child) {
  margin-top: 0;
}

.markdown-content :deep(h1) {
  font-size: 1.125rem;
}

.markdown-content :deep(h2) {
  font-size: 1rem;
}

.markdown-content :deep(h3),
.markdown-content :deep(h4) {
  font-size: 0.875rem;
}

.markdown-content :deep(hr) {
  border-color: var(--color-default);
  margin: 0.75rem 0;
}

.markdown-content :deep(img) {
  max-width: 100%;
  height: auto;
  border-radius: 0.5rem;
}

.markdown-content :deep(table) {
  width: 100%;
  border-collapse: collapse;
  margin-bottom: 0.5rem;
}

.markdown-content :deep(th),
.markdown-content :deep(td) {
  border: 1px solid var(--color-default);
  padding: 0.5rem 0.75rem;
  text-align: left;
  font-size: 0.875rem;
}

.markdown-content :deep(th) {
  background-color: var(--color-surface-alt);
  font-weight: 500;
}

/* Mention chip styling */
.markdown-content :deep(.mention-chip) {
  display: inline-flex;
  align-items: center;
  gap: 0.1875rem;
  padding: 0.0625em 0.375em 0.0625em 0.1875em;
  margin: 0 0.0625em;
  border-radius: 9999px;
  background-color: color-mix(in srgb, var(--color-accent) 12%, transparent);
  color: var(--color-accent);
  font-weight: 500;
  font-size: inherit;
  line-height: 1;
  vertical-align: text-bottom;
  cursor: pointer;
  user-select: none;
  transition: background-color 0.15s ease;
}

.markdown-content :deep(.mention-chip:hover) {
  background-color: color-mix(in srgb, var(--color-accent) 20%, transparent);
}

.markdown-content :deep(.mention-avatar) {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 1em;
  height: 1em;
  border-radius: 9999px;
  overflow: hidden;
  flex-shrink: 0;
}

.markdown-content :deep(.mention-avatar-img) {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.markdown-content :deep(.mention-avatar-fallback) {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: white;
  font-size: 0.5em;
  font-weight: 600;
}

.markdown-content :deep(.mention-name) {
  white-space: nowrap;
}

/* Ticket Link Card Styles */
.markdown-content :deep(.ticket-link-card) {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px 10px;
  margin: 8px 0;
  background: var(--color-surface-alt);
  border: 1px solid var(--color-border-subtle);
  border-radius: 6px;
  font-size: 12px;
  line-height: 1.4;
  cursor: pointer;
  transition: all 0.15s ease;
  width: 100%;
  max-width: 100%;
  /* Ensure it displays as a block and breaks out of inline flow */
  clear: both;
}

@media (min-width: 640px) {
  .markdown-content :deep(.ticket-link-card) {
    padding: 10px 12px;
    font-size: 13px;
  }
}

.markdown-content :deep(.ticket-link-card:hover) {
  border-color: var(--color-accent);
  background: var(--color-surface-hover);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
}

.markdown-content :deep(.ticket-link-loading) {
  opacity: 0.7;
}

.markdown-content :deep(.ticket-link-error) {
  border-color: var(--color-status-error);
}

.markdown-content :deep(.ticket-link-header) {
  display: flex;
  align-items: center;
  gap: 8px;
}

.markdown-content :deep(.ticket-link-id) {
  font-weight: 600;
  color: var(--color-text-secondary, #888);
  font-family: var(--font-mono, monospace);
  font-size: 12px;
  flex-shrink: 0;
}

.markdown-content :deep(.ticket-link-title) {
  color: var(--color-text-primary, #fff);
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}

.markdown-content :deep(.ticket-link-meta) {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
  font-size: 11px;
}

.markdown-content :deep(.ticket-link-person) {
  color: var(--color-text-secondary, #aaa);
}

.markdown-content :deep(.ticket-link-label) {
  color: var(--color-text-tertiary, #666);
}

.markdown-content :deep(.ticket-link-status) {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 4px;
  font-weight: 500;
  text-transform: capitalize;
}

.markdown-content :deep(.ticket-link-status-open) {
  background: var(--color-status-open-muted, rgba(59, 130, 246, 0.15));
  color: var(--color-status-open, #3b82f6);
}

.markdown-content :deep(.ticket-link-status-in-progress) {
  background: var(--color-status-in-progress-muted, rgba(245, 158, 11, 0.15));
  color: var(--color-status-in-progress, #f59e0b);
}

.markdown-content :deep(.ticket-link-status-closed) {
  background: var(--color-status-closed-muted, rgba(34, 197, 94, 0.15));
  color: var(--color-status-closed, #22c55e);
}

.markdown-content :deep(.ticket-link-priority) {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 4px;
  font-weight: 500;
  text-transform: capitalize;
}

.markdown-content :deep(.ticket-link-priority-high) {
  background: var(--color-priority-high-muted, rgba(239, 68, 68, 0.15));
  color: var(--color-priority-high, #ef4444);
}

.markdown-content :deep(.ticket-link-priority-medium) {
  background: var(--color-priority-medium-muted, rgba(245, 158, 11, 0.15));
  color: var(--color-priority-medium, #f59e0b);
}

.markdown-content :deep(.ticket-link-priority-low) {
  background: var(--color-priority-low-muted, rgba(34, 197, 94, 0.15));
  color: var(--color-priority-low, #22c55e);
}

.markdown-content :deep(.ticket-link-status .indicator-icon),
.markdown-content :deep(.ticket-link-priority .indicator-icon) {
  width: 10px;
  height: 10px;
  flex-shrink: 0;
}

.markdown-content :deep(.ticket-link-loader) {
  width: 12px;
  height: 12px;
  border: 2px solid var(--color-border-default, #333);
  border-top-color: var(--color-accent);
  border-radius: 50%;
  animation: ticket-link-spin 0.8s linear infinite;
}

@keyframes ticket-link-spin {
  to { transform: rotate(360deg); }
}
</style>
