<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useDataStore } from '@/stores/dataStore'
import { groupService } from '@/services/groupService'

export interface SelectedPrincipal {
  type: 'group' | 'user'
  id: string // group ID (as string) or user UUID
  name: string
  avatar?: string | null
}

const props = withDefaults(defineProps<{
  selectedItems: SelectedPrincipal[]
  placeholder?: string
}>(), {
  placeholder: 'Search users and groups...',
})

const emit = defineEmits<{
  (e: 'update:selectedItems', items: SelectedPrincipal[]): void
}>()

const dataStore = useDataStore()

const searchQuery = ref('')
const showDropdown = ref(false)
const loading = ref(false)

// Groups loaded once on mount
const allGroups = ref<Array<{ id: number; name: string }>>([])

// Users fetched per search
const searchedUsers = ref<Array<{ uuid: string; name: string; email: string; avatar_url?: string | null }>>([])

let searchTimer: ReturnType<typeof setTimeout> | null = null

const selectedSet = computed(() => {
  const set = new Set<string>()
  for (const item of props.selectedItems) {
    set.add(`${item.type}:${item.id}`)
  }
  return set
})

const filteredGroups = computed(() => {
  const q = searchQuery.value.toLowerCase()
  return allGroups.value.filter(g => {
    if (selectedSet.value.has(`group:${g.id}`)) return false
    if (!q) return true
    return g.name.toLowerCase().includes(q)
  })
})

const filteredUsers = computed(() => {
  return searchedUsers.value.filter(u => {
    return !selectedSet.value.has(`user:${u.uuid}`)
  })
})

const hasResults = computed(() => filteredGroups.value.length > 0 || filteredUsers.value.length > 0)

const searchUsers = async (query: string) => {
  loading.value = true
  try {
    const result = await dataStore.getPaginatedUsers({
      page: 1,
      pageSize: 20,
      search: query || undefined,
      sortField: 'name',
      sortDirection: 'asc',
    })
    searchedUsers.value = result.data.map(u => ({
      uuid: u.uuid,
      name: u.name,
      email: u.email,
      avatar_url: u.avatar_url,
    }))
  } catch {
    searchedUsers.value = []
  } finally {
    loading.value = false
  }
}

const onSearchInput = () => {
  if (searchTimer) clearTimeout(searchTimer)
  searchTimer = setTimeout(() => {
    searchUsers(searchQuery.value)
  }, 300)
  showDropdown.value = true
}

const addItem = (item: SelectedPrincipal) => {
  emit('update:selectedItems', [...props.selectedItems, item])
}

const removeItem = (item: SelectedPrincipal) => {
  emit('update:selectedItems', props.selectedItems.filter(i => !(i.type === item.type && i.id === item.id)))
}

const selectGroup = (group: { id: number; name: string }) => {
  addItem({
    type: 'group',
    id: String(group.id),
    name: group.name,
  })
}

const selectUser = (user: { uuid: string; name: string; avatar_url?: string | null }) => {
  addItem({
    type: 'user',
    id: user.uuid,
    name: user.name,
    avatar: user.avatar_url,
  })
}

const onFocus = () => {
  showDropdown.value = true
  if (searchedUsers.value.length === 0) {
    searchUsers('')
  }
}

const onBlur = () => {
  // Small delay so click events on dropdown items fire first
  setTimeout(() => {
    showDropdown.value = false
  }, 200)
}

onMounted(async () => {
  const groups = await groupService.getGroups()
  allGroups.value = groups.map(g => ({ id: g.id, name: g.name }))
  // Pre-load users
  searchUsers('')
})
</script>

<template>
  <div class="flex flex-col gap-2">
    <!-- Search input + dropdown (positioned first so dropdown opens above chips) -->
    <div class="relative">
      <input
        v-model="searchQuery"
        :placeholder="placeholder"
        @input="onSearchInput"
        @focus="onFocus"
        @blur="onBlur"
        class="w-full px-3 py-2 text-sm rounded-lg border border-default bg-surface text-primary placeholder:text-tertiary focus:outline-none focus:ring-1 focus:ring-accent/30 focus:border-accent/30"
      />

      <!-- Dropdown (opens upward) -->
      <div
        v-if="showDropdown"
        class="absolute z-50 left-0 right-0 bottom-full mb-1 max-h-60 overflow-y-auto rounded-lg border border-default bg-surface shadow-lg"
      >
        <div v-if="loading && !hasResults" class="p-3 text-xs text-tertiary text-center">
          Searching...
        </div>

        <div v-else-if="!hasResults && searchQuery" class="p-3 text-xs text-tertiary text-center">
          No results found
        </div>

        <template v-else>
          <!-- Groups section -->
          <div v-if="filteredGroups.length > 0">
            <div class="px-3 py-1.5 text-[10px] font-semibold text-tertiary uppercase tracking-wider bg-surface-alt">
              Groups
            </div>
            <button
              v-for="group in filteredGroups"
              :key="`g-${group.id}`"
              @mousedown.prevent="selectGroup(group)"
              class="w-full flex items-center gap-3 px-3 py-2.5 text-left hover:bg-surface-hover transition-colors"
            >
              <svg class="w-4 h-4 text-tertiary flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z" />
              </svg>
              <span class="text-sm text-primary truncate">{{ group.name }}</span>
            </button>
          </div>

          <!-- Users section -->
          <div v-if="filteredUsers.length > 0">
            <div class="px-3 py-1.5 text-[10px] font-semibold text-tertiary uppercase tracking-wider bg-surface-alt">
              Users
            </div>
            <button
              v-for="user in filteredUsers"
              :key="`u-${user.uuid}`"
              @mousedown.prevent="selectUser(user)"
              class="w-full flex items-center gap-3 px-3 py-2.5 text-left hover:bg-surface-hover transition-colors"
            >
              <div class="w-5 h-5 rounded-full bg-accent/20 flex items-center justify-center flex-shrink-0 overflow-hidden">
                <img
                  v-if="user.avatar_url"
                  :src="user.avatar_url"
                  :alt="user.name"
                  class="w-full h-full object-cover"
                />
                <span v-else class="text-[10px] font-medium text-accent">{{ user.name.charAt(0).toUpperCase() }}</span>
              </div>
              <div class="flex-1 min-w-0">
                <div class="text-sm text-primary truncate">{{ user.name }}</div>
                <div class="text-[11px] text-tertiary truncate">{{ user.email }}</div>
              </div>
            </button>
          </div>
        </template>
      </div>
    </div>

    <!-- Selected items as chips -->
    <div v-if="selectedItems.length > 0" class="flex flex-wrap gap-2">
      <span
        v-for="item in selectedItems"
        :key="`${item.type}:${item.id}`"
        class="inline-flex items-center gap-1.5 px-2 py-1 text-xs rounded-full border border-default bg-surface-alt text-primary"
      >
        <!-- Group icon -->
        <svg v-if="item.type === 'group'" class="w-3 h-3 text-tertiary flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z" />
        </svg>
        <!-- User icon -->
        <svg v-else class="w-3 h-3 text-tertiary flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" />
        </svg>
        <span class="truncate max-w-[120px]">{{ item.name }}</span>
        <button
          @click="removeItem(item)"
          class="text-tertiary hover:text-primary transition-colors"
        >
          <svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </span>
    </div>
  </div>
</template>
