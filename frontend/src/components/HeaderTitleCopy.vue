<!--
The bar's one-line copy of a title that lives on the page (phones; see
usePageTitleSlot). It keeps a person's place once the page title has
scrolled out of view, and a tap on it brings the title back.

It is always laid out and only its opacity changes, so the bar never
changes height as it comes and goes; `text-base` keeps one line inside the
bar's least height up to the largest text size. Presentation only: the
page's `h1` is the title a screen reader hears, and this is neither read
nor focusable.
-->
<script setup lang="ts">
import { pageTitleAway, pageTitleSettled, scrollToPageTitle } from '@/composables/usePageTitleSlot'

defineProps<{
  text: string
  /** Shown, quieter, when the title is empty. */
  fallback: string
}>()
</script>

<template>
  <span
    aria-hidden="true"
    class="header-title-copy min-w-0 flex-1 truncate text-base font-semibold"
    :class="[text ? 'text-primary' : 'text-tertiary', { 'is-shown': pageTitleAway, 'is-settled': pageTitleSettled }]"
    data-header-title-copy
    @click="scrollToPageTitle"
  >{{ text || fallback }}</span>
</template>

<style scoped>
/* In as the title leaves: a short fade with a 4px rise, the direction the
   title went. Out a little faster, so it is gone before the title is back
   in full. */
.header-title-copy {
  opacity: 0;
  transform: translateY(0.25rem);
  pointer-events: none;
}
.header-title-copy.is-settled {
  transition: opacity 120ms ease-in, transform 120ms ease-in;
}
.header-title-copy.is-shown {
  opacity: 1;
  transform: none;
  pointer-events: auto;
}
.header-title-copy.is-shown.is-settled {
  transition: opacity 200ms ease-out, transform 200ms ease-out;
}
/* Reduced motion: it is there or it is not. */
@media (prefers-reduced-motion: reduce) {
  .header-title-copy,
  .header-title-copy.is-settled,
  .header-title-copy.is-shown.is-settled {
    transform: none;
    transition: none;
  }
}
</style>
