<!--
Render a comment's body, choosing the right pipeline for its content
format and tucking any quoted history behind a disclosure.

Data flow priority (most-trusted first):
  1. `newContent` + `quotedContent` from the backend ingest splitter
     (Pass 1 of the email rendering plan). When set, these are the
     authoritative split — the splitter ran once at ingest with full
     access to MIME parts and headers and the result was persisted.
  2. `sanitisedHtml` from the backend sanitiser (Pass 2). When the
     parent doesn't have the split (older rows), this is still
     server-side safe HTML and is passed to `<EmailHtmlBody>` with
     `pre-sanitised` so DOMPurify doesn't double-process.
  3. `content` + client-side splitter (`splitQuotedHtml` /
     `splitQuotedReply`). Legacy path for rows ingested before the
     backend pipeline shipped.

  - `html`: an inbound email or any other channel that delivers HTML.
    Renders inside `<EmailHtmlBody>` (sandboxed iframe + DOMPurify
    or backend-sanitised). Both visible and quoted halves render in
    their own iframe so each one's CSS and layout reset stay
    isolated.

  - Markdown / plaintext / unknown: `<MarkdownRenderer>` +
    `splitQuotedReply`. UI-authored comments that pre-date the
    `content_format` column take this branch via the missing-format
    default.

Deliberately not used in the print path: when someone prints a ticket
they want the full archival record, not the summary view. The print
loop in `CommentsAndAttachments.vue` keeps calling `<MarkdownRenderer>`
directly.
-->
<template>
  <div class="flex flex-col gap-1">
    <template v-if="renderAsHtml">
      <EmailHtmlBody :html="visibleHtml" :pre-sanitised="htmlIsPreSanitised" />
      <details v-if="trimmedHtml" class="group">
        <summary :class="summaryClass">
          <svg :class="summaryIconClass" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
          </svg>
          <span>Show quoted thread</span>
        </summary>
        <div class="mt-1">
          <EmailHtmlBody :html="trimmedHtml" :pre-sanitised="htmlIsPreSanitised" />
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
    <!--
      Plain anchor rather than a button-plus-window.open: middle-click,
      right-click "open in new tab", copy-link, and keyboard activation
      all work for free. The href is null-guarded by v-if so the link
      can't navigate when we don't have a comment id.
    -->
    <a
      v-if="hasRawSource && rawSourceUrl"
      :href="rawSourceUrl"
      target="_blank"
      rel="noopener noreferrer"
      class="self-start text-[11px] text-tertiary hover:text-secondary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-status-info rounded"
      title="Open the raw RFC-822 source in a new tab"
    >
      Show original message
    </a>
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
  /**
   * Backend-extracted just-the-reply (post-sanitise, post-split).
   * When set, the renderer uses this as the visible body and treats
   * `quotedContent` as the disclosure body. Same shape as the client
   * splitter would produce, but generated once at ingest with full
   * MIME context and persisted.
   */
  newContent?: string | null;
  /**
   * Backend-extracted prior thread, paired with `newContent`.
   */
  quotedContent?: string | null;
  /**
   * Whether the backend can serve the original `.eml` via
   * `/api/comments/{id}/raw.eml`. Drives the visibility of the
   * "Show original message" affordance below the body.
   */
  hasRawSource?: boolean;
  /**
   * Used to construct the "Show original message" URL. Optional
   * because not every comment-context has the id in scope; the
   * link is hidden when missing.
   */
  commentId?: number;
}>();

// Tailwind class strings deduplicated at the template level.
const summaryClass =
  'cursor-pointer text-xs text-tertiary hover:text-secondary select-none inline-flex items-center gap-1 py-0.5 rounded focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-status-info';
const summaryIconClass = 'w-3 h-3 transition-transform group-open:rotate-90';

const renderAsHtml = computed(() => props.contentFormat === 'html');

// True when the backend supplied a pre-split, pre-sanitised view.
// Both halves of the split came from ammonia, so the iframe can
// skip the client-side DOMPurify pass. Legacy rows (no split
// available) take the client-side path and keep DOMPurify.
const htmlIsPreSanitised = computed(() => props.newContent != null);

// Markdown / plaintext branch. Prefer the backend's split when
// present; fall through to client-side `splitQuotedReply` for legacy
// rows.
const textSplit = computed(() => {
  if (renderAsHtml.value) return null;
  if (props.newContent != null) {
    return {
      visible: props.newContent,
      trimmed: props.quotedContent ?? '',
    };
  }
  return splitQuotedReply(props.content);
});
const visibleText = computed(() => textSplit.value?.visible ?? '');
const trimmedText = computed(() => textSplit.value?.trimmed ?? '');
const trimmedTextLines = computed(() =>
  trimmedText.value ? trimmedText.value.split('\n').length : 0,
);

// HTML branch. Same priority order: backend split first, then a
// client-side split on the raw `content` for legacy rows.
const htmlSplit = computed(() => {
  if (!renderAsHtml.value) return null;
  if (props.newContent != null) {
    return {
      visibleHtml: props.newContent,
      trimmedHtml: props.quotedContent ?? '',
    };
  }
  return splitQuotedHtml(props.content);
});
const visibleHtml = computed(() => htmlSplit.value?.visibleHtml ?? '');
const trimmedHtml = computed(() => htmlSplit.value?.trimmedHtml ?? '');

const rawSourceUrl = computed(() =>
  props.commentId != null ? `/api/comments/${props.commentId}/raw.eml` : null,
);
</script>
