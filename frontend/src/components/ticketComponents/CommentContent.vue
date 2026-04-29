<!--
Render a comment's body, choosing the right pipeline for its content
format and tucking any quoted history behind a disclosure.

  - `html`: an inbound email or any other channel that delivers HTML.
    Renders inside `<EmailHtmlBody>` (sandboxed iframe + DOMPurify).
    Quoted thread history is split off via `splitQuotedHtml` so the
    new content shows by default and the prior chain expands on
    demand. Both halves render in their own iframe instance, which
    keeps each one's CSS and layout reset isolated from the other.

  - Markdown / plaintext / unknown: legacy path through
    `<MarkdownRenderer>` + `splitQuotedReply`. UI-authored comments
    that pre-date the `content_format` column take this branch via
    the missing-format default.

Deliberately not used in the print path: when someone prints a ticket
they want the full archival record, not the summary view. The print
loop in `CommentsAndAttachments.vue` keeps calling `<MarkdownRenderer>`
directly.
-->
<template>
  <div class="flex flex-col gap-1">
    <template v-if="renderAsHtml">
      <EmailHtmlBody :html="visibleHtml" />
      <details v-if="trimmedHtml" class="group">
        <summary :class="summaryClass">
          <svg :class="summaryIconClass" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
          </svg>
          <span>Show quoted thread</span>
        </summary>
        <div class="mt-1">
          <EmailHtmlBody :html="trimmedHtml" />
        </div>
      </details>
    </template>
    <template v-else>
      <MarkdownRenderer :content="visibleText" class="text-primary" />
      <details v-if="trimmedText" class="group">
        <summary :class="summaryClass">
          <svg :class="summaryIconClass" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
          </svg>
          <span>Show quoted reply ({{ trimmedTextLines }} {{ trimmedTextLines === 1 ? 'line' : 'lines' }})</span>
        </summary>
        <div class="mt-1 border-l-2 border-subtle pl-3 text-secondary">
          <MarkdownRenderer :content="trimmedText" />
        </div>
      </details>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import MarkdownRenderer from '@/components/common/MarkdownRenderer.vue';
import EmailHtmlBody from '@/components/ticketComponents/EmailHtmlBody.vue';
import { splitQuotedReply } from '@/utils/quotedReply';
import { splitQuotedHtml } from '@/utils/quotedReplyHtml';
import type { CommentContentFormat } from '@/types/comment';

const props = defineProps<{
  content: string;
  /**
   * Format of the bytes in `content`, as declared by the writer. When
   * absent we fall through to the Markdown renderer — that matches
   * the pre-`content_format` behaviour for legacy comments.
   */
  contentFormat?: CommentContentFormat;
}>();

// Tailwind class strings deduplicated at the template level.
const summaryClass =
  'cursor-pointer text-xs text-tertiary hover:text-secondary select-none inline-flex items-center gap-1 py-0.5 rounded focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-status-info';
const summaryIconClass = 'w-3 h-3 transition-transform group-open:rotate-90';

const renderAsHtml = computed(() => props.contentFormat === 'html');

// Markdown / plaintext branch (legacy + inbound plaintext emails).
const textSplit = computed(() =>
  renderAsHtml.value ? null : splitQuotedReply(props.content),
);
const visibleText = computed(() => textSplit.value?.visible ?? '');
const trimmedText = computed(() => textSplit.value?.trimmed ?? '');
const trimmedTextLines = computed(() =>
  trimmedText.value ? trimmedText.value.split('\n').length : 0,
);

// HTML branch (inbound rich email).
const htmlSplit = computed(() =>
  renderAsHtml.value ? splitQuotedHtml(props.content) : null,
);
const visibleHtml = computed(() => htmlSplit.value?.visibleHtml ?? '');
const trimmedHtml = computed(() => htmlSplit.value?.trimmedHtml ?? '');
</script>
