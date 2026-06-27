/**
 * Workspace tag store. Pinia Colada query backed by
 * `tagService.list`. Single workspace-wide cache — every consumer
 * (picker, chip renderer, future filter UI) reads the same array.
 *
 * Tags are workspace config and change infrequently, so this
 * store doesn't subscribe to SSE for tag CRUD. Mutations from
 * the admin tag-management UI invalidate the cache directly via
 * `useQueryCache().invalidateQueries`.
 */
import { defineStore } from 'pinia'
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'
import { tagService } from '@nosdesk/core/services/tagService'
import type { Tag } from '@nosdesk/core/types/tag'

const TAGS_KEY = ['tags', 'list', 'active'] as const

export const useTagsStore = defineStore('tags', () => {
  // Active tags only — archived tags are excluded from the picker.
  // The admin management UI fetches archived tags separately when
  // it needs to render the restore affordance.
  const query = useQuery<Tag[]>({
    key: TAGS_KEY,
    query: () => tagService.list(false),
    // Tags rarely change — a 5-minute stale window keeps the
    // picker snappy without showing badly out-of-date names. Edits
    // through the management UI invalidate explicitly.
    staleTime: 5 * 60 * 1000,
  })

  const tags = computed<Tag[]>(() => query.data.value ?? [])
  const isLoading = computed(() => query.asyncStatus.value === 'loading')

  /** Resolve an id to a tag row, or null if it isn't in the
   *  active set (deleted / archived). Synchronous — drives the
   *  per-ticket chip renderer that just has ids to start with. */
  function findById(id: number): Tag | null {
    return tags.value.find((t) => t.id === id) ?? null
  }

  return {
    tags,
    isLoading,
    findById,
    refetch: query.refetch,
  }
})

export const TAGS_QUERY_KEY = TAGS_KEY
