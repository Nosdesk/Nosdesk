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
import { isAllowedImageSrc } from '@/composables/useSanitise';
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

// Configure marked. gfm is the default in v18; setting `breaks` flips
// single newlines into <br> so chat-style content reads correctly.
// marked.setOptions() was removed in v18, use marked.use() instead.
marked.use({ breaks: true });

// Render content
const renderedHtml = computed(() => {
  if (!props.content) return '';

  // First, preprocess mentions
  const withMentions = preprocessMentions(props.content);

  // Parse markdown
  const html = marked.parse(withMentions) as string;

  // Block off-origin <img src> (tracking-pixel exfiltration) while
  // allowing same-origin / relative / data: images. See
  // security-audit-2026-06.
  DOMPurify.addHook('uponSanitizeAttribute', (node, data) => {
    if (node.nodeName === 'IMG' && data.attrName === 'src' && !isAllowedImageSrc(data.attrValue)) {
      data.keepAttr = false;
    }
  });

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

  DOMPurify.removeHook('uponSanitizeAttribute');

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
</style>
