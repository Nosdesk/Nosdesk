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
import UserAvatar from '@/components/UserAvatar.vue'
import ResponsivePanel from '@/components/common/ResponsivePanel.vue'
import { useDataStore } from '@/stores/dataStore'
import apiClient from '@/services/apiConfig'

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

const dataStore = useDataStore()
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
  if (!iso) return 'unknown'
  const diff = Date.now() - new Date(iso).getTime()
  const seconds = Math.round(diff / 1000)
  if (seconds < 60) return 'just now'
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `${minutes} min ago`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return `${hours} hr ago`
  const days = Math.round(hours / 24)
  if (days < 30) return `${days} day${days === 1 ? '' : 's'} ago`
  const months = Math.round(days / 30)
  if (months < 12) return `${months} month${months === 1 ? '' : 's'} ago`
  const years = Math.round(months / 12)
  return `${years} year${years === 1 ? '' : 's'} ago`
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
    const users = await dataStore.getUsersByUuids(idList)
    contributors.value = idList.map((uuid, i) => {
      const u = users[i]
      return {
        uuid,
        name: u?.name ?? 'Unknown user',
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
    title="Insights"
    side-panel-class="w-80"
    @close="emit('close')"
  >
    <div class="flex flex-1 flex-col gap-6 px-4 py-4 text-sm">
      <section>
        <h3 class="mb-2 text-xs font-semibold tracking-wide text-tertiary uppercase">Source</h3>
        <ul class="flex flex-col gap-1 text-secondary" role="list">
          <li>Created {{ relative(createdAt) }}</li>
          <li>Last updated {{ relative(updatedAt) }}</li>
        </ul>
      </section>

      <section>
        <h3 class="mb-2 text-xs font-semibold tracking-wide text-tertiary uppercase">Stats</h3>
        <ul class="flex flex-col gap-1 text-secondary" role="list">
          <li>{{ readingTimeMinutes }} minute read</li>
          <li>{{ wordCount.toLocaleString() }} words</li>
          <li>{{ charCount.toLocaleString() }} characters</li>
          <li>{{ emojiCount }} emoji</li>
        </ul>
      </section>

      <section>
        <h3 class="mb-2 text-xs font-semibold tracking-wide text-tertiary uppercase">
          Contributors
        </h3>
        <p
          v-if="loadingContributors && contributors.length === 0"
          class="text-xs text-tertiary"
        >
          Loading contributors...
        </p>
        <p
          v-else-if="contributors.length === 0"
          class="text-xs text-tertiary"
        >
          No contributors yet.
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
              <div class="truncate text-xs text-tertiary">Contributor</div>
            </div>
          </li>
        </ul>
      </section>
    </div>
  </ResponsivePanel>
</template>
