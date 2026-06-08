<script setup lang="ts">
/**
 * Compact documentation hub row — editorial index style.
 * Icon, title, and inline meta (author · time) stay grouped together.
 */
import { computed } from 'vue'
import { RouterLink, type RouteLocationRaw } from 'vue-router'
import { docUrl } from '@/utils/docUrl'
import type { Page } from '@/services/documentationService'

const props = defineProps<{
  page?: Page
  /** Override when the row is not backed by a full Page (e.g. starred). */
  href?: string | RouteLocationRaw
  title?: string
  icon?: string | null
  /** Inline meta after title — author, relative time, child count, etc. */
  meta?: string
}>()

const destination = computed(() => {
  if (props.href != null) return props.href
  if (props.page) return docUrl(props.page)
  return '/documentation'
})

const label = computed(() => props.title ?? props.page?.title ?? '')
const glyph = computed(() => props.icon ?? props.page?.icon ?? '📄')
</script>

<template>
  <RouterLink
    :to="destination"
    class="group flex items-center gap-2 py-1.5 min-h-7 px-2 -mx-2 rounded hover:bg-surface-hover transition-colors"
  >
    <span
      class="shrink-0 w-4 text-center text-sm leading-none opacity-80"
      aria-hidden="true"
    >
      {{ glyph || '📄' }}
    </span>
    <div class="flex items-center gap-1.5 min-w-0 flex-1">
      <span class="truncate text-[13px] leading-snug text-primary group-hover:text-accent transition-colors">
        {{ label }}
      </span>
      <template v-if="meta">
        <span class="shrink-0 text-[11px] text-tertiary/60" aria-hidden="true">·</span>
        <span class="shrink-0 text-[11px] leading-none text-tertiary whitespace-nowrap tabular-nums">
          {{ meta }}
        </span>
      </template>
    </div>
  </RouterLink>
</template>
