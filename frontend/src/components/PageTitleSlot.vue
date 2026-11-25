<!--
Where a detail view's title goes on a phone. Empty by itself: SiteHeader
teleports the editable title in (see usePageTitleSlot). From `sm` up the
title stays in the header and this takes no room.

The view gives it its margins. The title's own box (the tap highlight,
the edit border) is 6px wider than its text on each side, so a view sets
it 10px in from the screen edge (`mx-2.5`), or 6px out from a padded
column (`-mx-1.5`): the words then sit on the page's 16px line and the
box reaches past them.
-->
<script setup lang="ts">
import { ref } from 'vue'
import { usePageTitleSlot } from '@/composables/usePageTitleSlot'

const el = ref<HTMLElement | null>(null)
usePageTitleSlot(el)
</script>

<template>
  <div ref="el" class="sm:hidden" data-page-title></div>
</template>
