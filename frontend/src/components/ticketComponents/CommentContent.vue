<!--
Renders a comment body with email-style quoted history tucked behind a
disclosure. For normal UI-authored comments (no quoted section) this
collapses to a single `<MarkdownRenderer>` — no extra chrome.

Deliberately not used in the print path: when someone prints a ticket
they want the full archival record, not the summary view. The print
loop in `CommentsAndAttachments.vue` keeps calling `<MarkdownRenderer>`
directly.
-->
<template>
  <div class="flex flex-col gap-1">
    <MarkdownRenderer :content="visible" class="text-primary" />

    <details v-if="trimmed" class="group">
      <summary
        class="cursor-pointer text-xs text-tertiary hover:text-secondary select-none inline-flex items-center gap-1 py-0.5 rounded focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-status-info"
      >
        <svg
          class="w-3 h-3 transition-transform group-open:rotate-90"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
        </svg>
        <span>Show quoted reply ({{ trimmedLines }} {{ trimmedLines === 1 ? 'line' : 'lines' }})</span>
      </summary>
      <div class="mt-1 border-l-2 border-subtle pl-3 text-secondary">
        <MarkdownRenderer :content="trimmed" />
      </div>
    </details>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import MarkdownRenderer from '@/components/common/MarkdownRenderer.vue';
import { splitQuotedReply } from '@/utils/quotedReply';

const props = defineProps<{
  content: string;
}>();

// Computed so Vue caches it across re-renders of the same comment.
const split = computed(() => splitQuotedReply(props.content));
const visible = computed(() => split.value.visible);
const trimmed = computed(() => split.value.trimmed);
// Line count up front in the disclosure label ("Show quoted reply (12
// lines)") — matches Gmail / Help Scout / Outlook and helps the user
// decide whether the expand is worth the scroll.
const trimmedLines = computed(() =>
  trimmed.value ? trimmed.value.split('\n').length : 0,
);
</script>
