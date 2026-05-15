<script setup lang="ts">
/**
 * Read-only secondary surface showing source timestamps, content
 * stats, and contributors for a documentation page. Mirrors what
 * Outline surfaces under its "Insights" menu item — MVP scope,
 * no analytics pipeline required.
 *
 * Layout is owned by `<ResponsivePanel>`: side panel at md+,
 * bottom sheet on phone. This component just renders the body
 * content and the contributor / stats query logic; chrome
 * (header, close, drag handle, backdrop) lives in the wrapper.
 *
 * Stats are computed from the editor's current text content
 * (word / character / emoji counts, reading time at 200 WPM).
 * The contributor list pulls the unique set of `created_by`
 * UUIDs from the revision history and resolves each via the
 * dataStore.
 */
import { computed, ref, onMounted, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import UserAvatar from '@/components/UserAvatar.vue'
import ResponsivePanel from '@/components/common/ResponsivePanel.vue'
import * as syncPool from '@/sync/pool'
import type { User } from '@/types/user'
import apiClient from '@/services/apiConfig'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

interface ContributorInfo {
  uuid: string
  name: string
  avatar: string | null
}

interface Props {
  /** Open state. Owned by the parent. */
  open: boolean
  pageId: number
  createdAt: string | null
  updatedAt: string | null
  /** Plain-text content of the page used to compute stats.
   * The page view passes this in so the panel doesn't need to
   * reach into the editor itself. */
  text: string
}

const props = defineProps<Props>()

const emit = defineEmits<{
  (e: 'close'): void
}>()

const contributors = ref<ContributorInfo[]>([])
const loadingContributors = ref(false)

// Words per minute for reading-time estimation. 200 is the
// commonly-used average for technical/internal documentation.
const READING_WPM = 200

const wordCount = computed(() => {
  const trimmed = props.text.trim()
  if (!trimmed) return 0
  return trimmed.split(/\s+/).length
})

const charCount = computed(() => props.text.length)

// Emoji regex covers the common Unicode emoji blocks. Skin-tone
// modifiers and ZWJ sequences are counted as one emoji each (the
// regex matches the base codepoint, not the variation selector).
const emojiCount = computed(() => {
  const matches = props.text.match(
    /\p{Extended_Pictographic}/gu,
  )
  return matches?.length ?? 0
})

const readingTimeMinutes = computed(() => {
  return Math.max(1, Math.round(wordCount.value / READING_WPM))
})

function relative(iso: string | null): string {
  if (!iso) return t('docs-insights-relative-unknown')
  const diff = Date.now() - new Date(iso).getTime()
  const seconds = Math.round(diff / 1000)
  if (seconds < 60) return t('docs-insights-relative-just-now')
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return t('docs-insights-relative-minutes', { count: minutes })
  const hours = Math.round(minutes / 60)
  if (hours < 24) return t('docs-insights-relative-hours', { count: hours })
  const days = Math.round(hours / 24)
  if (days < 30) return t('docs-insights-relative-days', { count: days })
  const months = Math.round(days / 30)
  if (months < 12) return t('docs-insights-relative-months', { count: months })
  const years = Math.round(months / 12)
  return t('docs-insights-relative-years', { count: years })
}

interface RevisionResponse {
  created_by?: string | null
  created_at: string
}

async function loadContributors() {
  if (!props.pageId) return
  loadingContributors.value = true
  try {
    const response = await apiClient.get<RevisionResponse[]>(
      `/collaboration/docs/${props.pageId}/revisions`,
    )
    const ids = new Set<string>()
    for (const rev of response.data) {
      if (rev.created_by) ids.add(rev.created_by)
    }
    const idList = Array.from(ids)
    // Workspace:1 user bootstrap means every contributor uuid is
    // already in the sync pool. Map directly rather than rounding
    // through a fetch the pool would dedupe to a no-op anyway.
    const users = idList.map((uuid) => syncPool.get<User>('user', uuid) ?? null)
    contributors.value = idList.map((uuid, i) => {
      const u = users[i]
      return {
        uuid,
        name: u?.name ?? t('docs-insights-unknown-user'),
        avatar: u?.avatar_thumb ?? u?.avatar_url ?? null,
      }
    })
  } catch {
    contributors.value = []
  } finally {
    loadingContributors.value = false
  }
}

onMounted(loadContributors)
watch(() => props.pageId, loadContributors)
</script>

<template>
  <ResponsivePanel
    :open="open"
    :title="$t('docs-insights-title')"
    side-panel-class="w-80"
    @close="emit('close')"
  >
    <div class="flex flex-1 flex-col gap-6 px-4 py-4 text-sm">
      <section>
        <h3 class="mb-2 text-xs font-semibold tracking-wide text-tertiary uppercase">{{ $t('docs-insights-source-heading') }}</h3>
        <ul class="flex flex-col gap-1 text-secondary" role="list">
          <li>{{ $t('docs-insights-created', { relative: relative(createdAt) }) }}</li>
          <li>{{ $t('docs-insights-updated', { relative: relative(updatedAt) }) }}</li>
        </ul>
      </section>

      <section>
        <h3 class="mb-2 text-xs font-semibold tracking-wide text-tertiary uppercase">{{ $t('docs-insights-stats-heading') }}</h3>
        <ul class="flex flex-col gap-1 text-secondary" role="list">
          <li>{{ $t('docs-insights-reading-time', { minutes: readingTimeMinutes }) }}</li>
          <li>{{ $t('docs-insights-word-count', { count: wordCount.toLocaleString() }) }}</li>
          <li>{{ $t('docs-insights-char-count', { count: charCount.toLocaleString() }) }}</li>
          <li>{{ $t('docs-insights-emoji-count', { count: emojiCount }) }}</li>
        </ul>
      </section>

      <section>
        <h3 class="mb-2 text-xs font-semibold tracking-wide text-tertiary uppercase">
          {{ $t('docs-insights-contributors-heading') }}
        </h3>
        <p
          v-if="loadingContributors && contributors.length === 0"
          class="text-xs text-tertiary"
        >
          {{ $t('docs-insights-contributors-loading') }}
        </p>
        <p
          v-else-if="contributors.length === 0"
          class="text-xs text-tertiary"
        >
          {{ $t('docs-insights-contributors-empty') }}
        </p>
        <ul v-else class="flex flex-col gap-2" role="list">
          <li
            v-for="c in contributors"
            :key="c.uuid"
            class="flex items-center gap-2"
          >
            <UserAvatar
              :name="c.uuid"
              :user-name="c.name"
              :avatar="c.avatar"
              :show-name="false"
              size="sm"
              :clickable="false"
            />
            <div class="min-w-0 flex-1">
              <div class="truncate text-primary">{{ c.name }}</div>
              <div class="truncate text-xs text-tertiary">{{ $t('docs-insights-contributor-role') }}</div>
            </div>
          </li>
        </ul>
      </section>
    </div>
  </ResponsivePanel>
</template>
