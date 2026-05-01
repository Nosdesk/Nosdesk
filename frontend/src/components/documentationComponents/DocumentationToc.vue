<!--
  DocumentationToc — auto-generated outline derived from a sibling
  editor's rendered DOM.

  Pure presentation: the heading walk, mutation observation, and
  active-heading tracking all live in the `useTableOfContents`
  composable. This file just wires the prop in and lays out the
  rail. The component renders nothing when fewer than two headings
  exist — a one-item TOC is noise.

  Receives the editor's rendered DOM element as a prop, set by the
  parent in response to the editor's `@ready` event. This is the
  canonical Vue 3 sibling-communication shape: child emits, parent
  stores in a ref, sibling watches via prop. See
  https://vuejs.org/guide/components/events.html.
-->
<script setup lang="ts">
import { computed } from 'vue';
import { useTableOfContents, type TocEntry } from '@/composables/useTableOfContents';

const props = defineProps<{
  /**
   * Rendered DOM root the TOC should walk for headings. `null` until
   * the editor finishes mounting; the composable handles the
   * transition cleanly.
   */
  element: HTMLElement | null;
}>();

const { headings, activeId } = useTableOfContents(() => props.element);

const visible = computed(() => headings.value.length >= 2);
/** Smallest heading level present, used to flatten the indent so a
 *  page that opens at H2 doesn't render every entry indented an
 *  extra column. */
const baseLevel = computed(() =>
  headings.value.reduce((min, h) => Math.min(min, h.level), 6),
);

function jumpTo(entry: TocEntry, ev: MouseEvent) {
  ev.preventDefault();
  // `scroll-margin-top` on the scroll container handles the sticky-
  // header offset; we let the browser's native smooth scroll do the
  // rest.
  entry.el.scrollIntoView({ behavior: 'smooth', block: 'start' });
  history.replaceState(null, '', `#${entry.id}`);
  activeId.value = entry.id;
}
</script>

<template>
  <nav v-if="visible" class="docs-toc" aria-label="On this page">
    <h2 class="text-[11px] font-semibold uppercase tracking-wide text-tertiary mb-2">
      On this page
    </h2>
    <ol class="flex flex-col gap-0.5">
      <li
        v-for="entry in headings"
        :key="entry.id"
        :style="{ paddingLeft: `${(entry.level - baseLevel) * 12}px` }"
      >
        <a
          :href="`#${entry.id}`"
          class="docs-toc-link block py-0.5 text-xs text-secondary hover:text-primary transition-colors truncate"
          :class="{ 'docs-toc-link--active': entry.id === activeId }"
          @click="jumpTo(entry, $event)"
        >
          {{ entry.text }}
        </a>
      </li>
    </ol>
  </nav>
</template>

<style scoped>
.docs-toc {
  position: sticky;
  top: 1rem;
  font-size: 0.75rem;
  line-height: 1.4;
  border-left: 1px solid var(--color-default);
  padding-left: 0.75rem;
  max-height: calc(100vh - 6rem);
  overflow-y: auto;
}

.docs-toc-link {
  position: relative;
  border-radius: 4px;
  padding-right: 6px;
}

.docs-toc-link--active {
  color: var(--color-accent);
  font-weight: 500;
}

.docs-toc-link--active::before {
  content: '';
  position: absolute;
  left: -0.78rem;
  top: 0.25rem;
  bottom: 0.25rem;
  width: 2px;
  background: currentColor;
  border-radius: 1px;
}
</style>
