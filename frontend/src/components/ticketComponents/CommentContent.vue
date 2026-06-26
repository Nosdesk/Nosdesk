<!--
Render a comment's body using the native-first model: most email
correspondence is text or simple HTML, so render those inline (like an
agent's own reply) and reserve the sandboxed iframe for genuinely rich
mail (newsletters, layout tables, Word-soup).

Tier comes from the backend `render_kind` (set by the inbound pipeline):
  - `text`   — plaintext / format=flowed. Linkified, pre-wrap, app
    typography. No iframe.
  - `simple` — human HTML reduced server-side to a semantic-inline
    subset. Rendered inline via `v-safe-html` (DOMPurify defence in
    depth over the already-reduced HTML). No iframe.
  - `rich`   — kept as full sanitised HTML; rendered in `<EmailHtmlBody>`
    (sandboxed iframe). The quoted half renders as an inline sanitised
    `<div>`, not a second iframe.

When `render_kind` is absent (UI-authored comments, rows ingested before
the pipeline), fall back to the legacy per-`content_format` rendering:
HTML → two `<EmailHtmlBody>` iframes via the client-side splitter;
otherwise `<MarkdownRenderer>` with the client-side text splitter.

The visible / quoted split is the backend's (`new_content` /
`quoted_content`) for pipeline rows, or the client splitter for legacy
rows.

Deliberately not used in the print path: the print loop in
`CommentsAndAttachments.vue` keeps calling `<MarkdownRenderer>` directly
so a printed ticket carries the full archival record, not the summary.
-->
<template>
  <div class="flex flex-col gap-1">
    <!-- Visible body -->
    <EmailHtmlBody
      v-if="visibleMode === 'iframe'"
      :html="visible"
      :pre-sanitised="htmlIsPreSanitised"
    />
    <div
      v-else-if="visibleMode === 'inline-html'"
      v-safe-html="visible"
      class="email-inline-body text-primary"
    />
    <div
      v-else-if="visibleMode === 'inline-text'"
      v-safe-html="visibleTextHtml"
      class="whitespace-pre-wrap break-words text-primary"
    />
    <MarkdownRenderer v-else :content="visible" class="text-primary" />

    <!-- Quoted history, tucked behind a disclosure -->
    <details v-if="quoted" class="group">
      <summary :class="summaryClass">
        <svg :class="summaryIconClass" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
        </svg>
        <span>{{ quotedLabel }}</span>
      </summary>
      <div class="mt-1">
        <!-- Quoted never gets its own iframe except on the legacy HTML
             path; rich/simple quoted render as inline sanitised HTML. -->
        <EmailHtmlBody
          v-if="quotedMode === 'iframe'"
          :html="quoted"
          :pre-sanitised="htmlIsPreSanitised"
        />
        <div
          v-else-if="quotedMode === 'inline-html'"
          v-safe-html="quoted"
          class="email-inline-body border-l-2 border-subtle pl-3 text-secondary"
        />
        <div
          v-else-if="quotedMode === 'inline-text'"
          v-safe-html="quotedTextHtml"
          class="whitespace-pre-wrap break-words border-l-2 border-subtle pl-3 text-secondary"
        />
        <div v-else class="border-l-2 border-subtle pl-3 text-secondary">
          <MarkdownRenderer :content="quoted" />
        </div>
      </div>
    </details>

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
      :title="$t('ticket-comments-show-original-title')"
    >
      {{ $t('ticket-comments-show-original') }}
    </a>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import MarkdownRenderer from '@/components/common/MarkdownRenderer.vue';
import EmailHtmlBody from '@/components/ticketComponents/EmailHtmlBody.vue';
import { splitQuotedReply } from '@nosdesk/core/utils/quotedReply';
import { splitQuotedHtml } from '@nosdesk/core/utils/quotedReplyHtml';
import { linkifyText } from '@nosdesk/core/utils/linkifyText';
import type { CommentContentFormat, CommentRenderKind } from '@nosdesk/core/types/comment';

const props = defineProps<{
  content: string;
  /**
   * Format of the bytes in `content`, as declared by the writer. Used
   * for the legacy fallback when `renderKind` is absent.
   */
  contentFormat?: CommentContentFormat;
  /**
   * Native-first render tier from the backend. When set, it drives the
   * renderer directly; when absent, the legacy per-`contentFormat` path
   * is used.
   */
  renderKind?: CommentRenderKind | null;
  /**
   * Backend-extracted just-the-reply (post-sanitise, post-split). When
   * set, it's the visible body and `quotedContent` is the disclosure.
   */
  newContent?: string | null;
  /** Backend-extracted prior thread, paired with `newContent`. */
  quotedContent?: string | null;
  /** Whether `/api/comments/{id}/raw.eml` is available. */
  hasRawSource?: boolean;
  /** Used to build the "Show original message" URL. */
  commentId?: number;
}>();

const fluent = useFluent();

const summaryClass =
  'cursor-pointer text-xs text-tertiary hover:text-secondary select-none inline-flex items-center gap-1 py-0.5 rounded focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-status-info';
const summaryIconClass = 'w-3 h-3 transition-transform group-open:rotate-90';

type Kind = 'text' | 'simple' | 'rich' | 'legacy-html' | 'legacy-markdown';

const kind = computed<Kind>(() => {
  switch (props.renderKind) {
    case 'text':
      return 'text';
    case 'simple':
      return 'simple';
    case 'rich':
      return 'rich';
    default:
      return props.contentFormat === 'html' ? 'legacy-html' : 'legacy-markdown';
  }
});

// Legacy rows have no backend split; compute it once from `content`.
const legacySplit = computed(() => {
  if (kind.value === 'legacy-html') {
    const { visibleHtml, trimmedHtml } = splitQuotedHtml(props.content);
    return { visible: visibleHtml, quoted: trimmedHtml };
  }
  if (kind.value === 'legacy-markdown') {
    const { visible, trimmed } = splitQuotedReply(props.content);
    return { visible, quoted: trimmed };
  }
  return null;
});

// For pipeline tiers, prefer the backend split; `content` is the
// safety net if `new_content` is somehow absent.
const visible = computed(() =>
  legacySplit.value ? legacySplit.value.visible : (props.newContent ?? props.content),
);
const quoted = computed(() =>
  legacySplit.value ? legacySplit.value.quoted : (props.quotedContent ?? ''),
);

const visibleTextHtml = computed(() => linkifyText(visible.value));
const quotedTextHtml = computed(() => linkifyText(quoted.value));

// Render mode per half. Rich + legacy-html use the iframe for the
// visible body; rich's quoted half drops to inline HTML (halves the
// iframe count), while legacy-html keeps its second iframe.
const visibleMode = computed(() => {
  switch (kind.value) {
    case 'rich':
    case 'legacy-html':
      return 'iframe' as const;
    case 'simple':
      return 'inline-html' as const;
    case 'text':
      return 'inline-text' as const;
    default:
      return 'markdown' as const;
  }
});
const quotedMode = computed(() => {
  switch (kind.value) {
    case 'legacy-html':
      return 'iframe' as const;
    case 'rich':
    case 'simple':
      return 'inline-html' as const;
    case 'text':
      return 'inline-text' as const;
    default:
      return 'markdown' as const;
  }
});

// Both halves of a backend split came from ammonia, so the iframe can
// skip the client DOMPurify pass. Legacy rows keep DOMPurify.
const htmlIsPreSanitised = computed(() => props.newContent != null);

const quotedLabel = computed(() => {
  if (kind.value === 'text' || kind.value === 'legacy-markdown') {
    const lines = quoted.value ? quoted.value.split('\n').length : 0;
    return fluent.$t('ticket-comments-show-quoted-reply', { lines });
  }
  return fluent.$t('ticket-comments-show-quoted-thread');
});

const rawSourceUrl = computed(() =>
  props.commentId != null ? `/api/comments/${props.commentId}/raw.eml` : null,
);
</script>
