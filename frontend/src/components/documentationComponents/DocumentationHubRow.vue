<script setup lang="ts">
/**
 * Compact documentation hub row — editorial index style.
 * Icon, title, and trailing meta (avatar · name · time).
 * Name is hidden on small screens to preserve title space.
 */
import { computed } from 'vue'
import { RouterLink, type RouteLocationRaw } from 'vue-router'
import { docUrl } from '@nosdesk/core/utils/docUrl'
import { formatCompactRelativeTime, formatRelativeTime } from '@nosdesk/core/utils/dateUtils'
import type { Page } from '@nosdesk/core/services/documentationService'
import type { UserInfo } from '@nosdesk/core/types/user'
import UserAvatar from '@/components/UserAvatar.vue'

const props = defineProps<{
  page?: Page
  /** Override when the row is not backed by a full Page (e.g. starred). */
  href?: string | RouteLocationRaw
  title?: string
  icon?: string | null
  /** Page author — shown as avatar in trailing meta. */
  author?: UserInfo | null
  /** Last-updated timestamp for trailing meta. */
  updatedAt?: string | null
  /** Plain inline meta — verification labels, child counts, etc. */
  meta?: string
}>()

const destination = computed(() => {
  if (props.href != null) return props.href
  if (props.page) return docUrl(props.page)
  return '/documentation'
})

const label = computed(() => props.title ?? props.page?.title ?? '')
const glyph = computed(() => props.icon ?? props.page?.icon ?? '📄')

const hasStructuredMeta = computed(
  () => !!props.author?.name || !!props.updatedAt,
)

const compactTime = computed(() =>
  props.updatedAt ? formatCompactRelativeTime(props.updatedAt) : '',
)

const fullTime = computed(() =>
  props.updatedAt ? formatRelativeTime(props.updatedAt) : '',
)
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
    <span class="truncate min-w-0 flex-1 text-[13px] leading-snug text-primary group-hover:text-accent transition-colors">
      {{ label }}
    </span>

    <!-- Author avatar + updated time -->
    <span
      v-if="hasStructuredMeta"
      class="shrink-0 flex items-center gap-1.5 min-w-0 max-w-[min(14rem,45%)]"
    >
      <UserAvatar
        v-if="author"
        :uuid="author.uuid"
        :fallback-name="author.name"
        :fallback-avatar="author.avatar_thumb ?? author.avatar_url"
        :show-name="false"
        size="xxs"
        :clickable="false"
      />
      <template v-if="author?.name">
        <span class="sr-only sm:hidden">{{ author.name }}</span>
        <span class="hidden sm:inline truncate max-w-[8rem] text-[11px] leading-none text-tertiary">
          {{ author.name }}
        </span>
      </template>
      <span
        v-if="author?.name && fullTime"
        class="hidden sm:inline text-[11px] text-tertiary/60 shrink-0"
        aria-hidden="true"
      >·</span>
      <span
        v-if="compactTime"
        class="sm:hidden text-[11px] leading-none text-tertiary whitespace-nowrap tabular-nums shrink-0"
        :title="fullTime"
      >
        {{ compactTime }}
      </span>
      <span
        v-if="fullTime"
        class="hidden sm:inline text-[11px] leading-none text-tertiary whitespace-nowrap tabular-nums shrink-0"
      >
        {{ fullTime }}
      </span>
    </span>

    <!-- Plain meta string -->
    <span
      v-else-if="meta"
      class="shrink-0 text-[11px] leading-none text-tertiary whitespace-nowrap tabular-nums"
    >
      {{ meta }}
    </span>
  </RouterLink>
</template>
