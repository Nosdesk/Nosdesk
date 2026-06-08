import { computed, type Ref } from 'vue'
import { useQuery } from '@pinia/colada'
import { useDebouncedRef } from '@/composables/useDebouncedRef'
import userService from '@/services/userService'
import { groupService } from '@/services/groupService'

/** Shared with GroupsManagementView / SlaAdminView so picker modals reuse cached groups. */
export const GROUPS_QUERY_KEY = ['groups'] as const

export interface AssignmentPickerUser {
  uuid: string
  name: string
  email: string
  avatar_url?: string | null
}

export function useAssignmentPickerQueries(searchQuery: Ref<string>) {
  const debouncedSearch = useDebouncedRef(searchQuery, 300)

  const groupsQuery = useQuery({
    key: GROUPS_QUERY_KEY,
    query: () => groupService.getGroups(),
  })

  const usersQuery = useQuery({
    key: () => ['users', 'picker', debouncedSearch.value.trim()],
    query: async (): Promise<AssignmentPickerUser[]> => {
      const result = await userService.getPaginatedUsers({
        page: 1,
        pageSize: 20,
        search: debouncedSearch.value.trim() || undefined,
        sortField: 'name',
        sortDirection: 'asc',
      })
      return result.data.map((u) => ({
        uuid: u.uuid,
        name: u.name,
        email: u.email,
        avatar_url: u.avatar_url,
      }))
    },
    staleTime: 30_000,
  })

  const allGroups = computed(() =>
    (Array.isArray(groupsQuery.data.value) ? groupsQuery.data.value : []).map((g) => ({
      id: g.id,
      name: g.name,
    })),
  )

  const searchedUsers = computed(() => usersQuery.data.value ?? [])

  const loading = computed(
    () =>
      (groupsQuery.status.value === 'pending' && groupsQuery.data.value === undefined)
      || (usersQuery.status.value === 'pending' && usersQuery.data.value === undefined),
  )

  return {
    allGroups,
    searchedUsers,
    loading,
  }
}
