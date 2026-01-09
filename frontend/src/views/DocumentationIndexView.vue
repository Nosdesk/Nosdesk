<script setup lang="ts">
import { ref, computed, onMounted, onActivated, onDeactivated, onUnmounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useTitleManager } from '@/composables/useTitleManager'
import { useMobileSearch } from '@/composables/useMobileSearch'
import { useDocumentation } from '@/composables/useDocumentation'
import { useStaggeredList } from '@/composables/useStaggeredList'
import BackButton from '@/components/common/BackButton.vue'
import DebouncedSearchInput from '@/components/common/DebouncedSearchInput.vue'
import DocumentationCardGrid from '@/components/documentationComponents/DocumentationCardGrid.vue'
import DocumentationCardSkeleton from '@/components/documentationComponents/DocumentationCardSkeleton.vue'
import type { Page } from '@/services/documentationService'

const route = useRoute()
const router = useRouter()
const titleManager = useTitleManager()

// Use shared documentation composable
const {
  pages,
  loading,
  showSkeleton,
  loadAllPages,
  createNewPage,
} = useDocumentation()

// Search state
const searchQuery = ref('')
const searchDropdownVisible = ref(false)
const searchRef = ref<HTMLElement | null>(null)

// Staggered animation for cards
const { getStyle: getCardStyle } = useStaggeredList({
  staggerDelay: 30,
  maxStaggerItems: 12
})

// Filtered pages for search - recursive search through hierarchy
const filteredPages = computed(() => {
  if (!searchQuery.value) return []

  const query = searchQuery.value.toLowerCase()
  const results: Array<{
    id: string
    title: string
    description?: string
    path?: string
    icon?: string
    isPage: boolean
  }> = []

  const searchPagesRecursively = (pageList: Page[], parentPath = '') => {
    pageList.forEach(page => {
      if (page.title.toLowerCase().includes(query)) {
        results.push({
          id: String(page.id),
          title: page.title,
          description: page.content?.substring(0, 100) || '',
          icon: page.icon || undefined,
          isPage: true
        })
      }

      if (page.children && page.children.length > 0) {
        const currentPath = parentPath ? `${parentPath}/${page.slug || page.id}` : `${page.slug || page.id}`

        page.children.forEach(child => {
          if (child.title.toLowerCase().includes(query)) {
            results.push({
              id: String(child.id),
              title: child.title,
              path: `/documentation/${child.id}`,
              icon: child.icon || undefined,
              isPage: false
            })
          }

          if (child.children && child.children.length > 0) {
            searchPagesRecursively([child as Page], currentPath)
          }
        })
      }
    })
  }

  searchPagesRecursively(pages.value)
  return results
})

// Search handlers
const handleSearch = (query: string) => {
  searchDropdownVisible.value = query.length > 0
}

const handleClickOutside = (event: MouseEvent) => {
  if (searchRef.value && !searchRef.value.contains(event.target as Node)) {
    searchDropdownVisible.value = false
  }
}

// Handle page creation
const handleCreatePage = async () => {
  try {
    await createNewPage()
  } catch (error) {
    console.error('Failed to create page:', error)
  }
}

// Mobile search bar integration
const { registerMobileSearch, deregisterMobileSearch, updateSearchQuery: updateMobileSearchQuery } = useMobileSearch()

const setupMobileSearch = () => {
  registerMobileSearch({
    searchQuery: searchQuery.value,
    placeholder: 'Search documentation...',
    showCreateButton: true,
    createIcon: 'document',
    onSearchUpdate: (value: string) => {
      searchQuery.value = value
      handleSearch(value)
    },
    onCreate: handleCreatePage
  })
}

// Lifecycle
onMounted(async () => {
  titleManager.setCustomTitle('Documentation')
  await loadAllPages()
  setupMobileSearch()
  document.addEventListener('click', handleClickOutside)
})

onActivated(() => {
  // Refresh data when returning to this view (KeepAlive)
  loadAllPages()
  setupMobileSearch()
})

onDeactivated(() => {
  deregisterMobileSearch()
})

onUnmounted(() => {
  deregisterMobileSearch()
  document.removeEventListener('click', handleClickOutside)
})

// Sync search query to mobile search bar
watch(searchQuery, (value) => {
  updateMobileSearchQuery(value)
})

// Watch for search query in URL
watch(() => route.query.search, (newSearch) => {
  if (newSearch && typeof newSearch === 'string' && searchQuery.value === '') {
    searchQuery.value = newSearch
  }
}, { immediate: true })

// Expose for parent components (SiteHeader create button)
defineExpose({
  createNewPage: handleCreatePage
})
</script>

<template>
  <div class="bg-app flex flex-col h-full">
    <!-- Header bar -->
    <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
      <div class="p-2 flex items-center gap-2 flex-wrap" ref="searchRef">
        <!-- Back button -->
        <BackButton fallbackRoute="/" label="Back to Dashboard" />

        <!-- Search bar - hidden on mobile (shown in MobileSearchBar) -->
        <DebouncedSearchInput
          v-model="searchQuery"
          placeholder="Search documentation..."
          @update:modelValue="handleSearch"
          @focus="searchDropdownVisible = searchQuery.length > 0"
          class="hidden sm:block"
        />

        <!-- Spacer -->
        <div class="flex-1"></div>

        <!-- Results count -->
        <div v-if="searchQuery" class="text-xs text-tertiary">
          {{ filteredPages.length }} result{{ filteredPages.length !== 1 ? 's' : '' }}
        </div>
      </div>

      <!-- Search Results dropdown -->
      <div
        v-if="searchQuery && searchDropdownVisible"
        class="absolute left-0 right-0 mt-1 mx-2 bg-surface border border-default rounded-lg shadow-xl z-50 max-h-96 overflow-y-auto"
      >
        <div class="p-2 border-b border-default flex justify-between items-center">
          <h2 class="text-sm font-medium text-primary flex items-center gap-2">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 text-accent" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
            </svg>
            Search Results
          </h2>
          <button
            @click="searchDropdownVisible = false"
            class="text-secondary hover:text-primary rounded-full p-1 hover:bg-surface-hover"
            aria-label="Close search results"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        <!-- No results message -->
        <div v-if="filteredPages.length === 0" class="p-4 text-center text-secondary text-sm">
          <div class="flex flex-col items-center gap-2">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-8 w-8 text-tertiary" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9.172 16.172a4 4 0 015.656 0M9 10h.01M15 10h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <p>No pages found for "<span class="text-primary">{{ searchQuery }}</span>"</p>
          </div>
        </div>

        <!-- Results list -->
        <div v-else class="divide-y divide-subtle">
          <div
            v-for="(item, index) in filteredPages"
            :key="item.id"
            class="hover:bg-surface-hover transition-colors"
            :style="getCardStyle(index)"
          >
            <RouterLink
              :to="item.path ? `/documentation/${item.path}` : `/documentation/${item.id}`"
              class="flex items-start gap-3 p-3"
              @click="searchDropdownVisible = false"
            >
              <div class="text-lg flex-shrink-0 bg-surface-alt p-1 rounded text-center" style="min-width: 1.75rem">
                {{ item.icon || '📄' }}
              </div>
              <div class="flex-1 min-w-0">
                <h3 class="text-primary font-medium text-sm truncate">{{ item.title }}</h3>
                <p v-if="item.description" class="text-secondary text-xs mt-0.5 line-clamp-1">
                  {{ item.description }}
                </p>
              </div>
            </RouterLink>
          </div>
        </div>
      </div>
    </div>

    <!-- Main content -->
    <div class="flex flex-col flex-1 overflow-auto bg-gradient-to-b from-bg-app to-bg-surface items-center">
      <div class="flex flex-col max-w-7xl mx-auto w-full px-4 py-6 gap-6">
        <!-- Header -->
        <div class="flex items-center justify-between gap-4 pb-4 border-b border-default">
          <div class="flex items-center gap-2">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-6 w-6 text-accent" viewBox="0 0 20 20" fill="currentColor">
              <path d="M2 6a2 2 0 012-2h5l2 2h5a2 2 0 012 2v6a2 2 0 01-2 2H4a2 2 0 01-2-2V6z" />
            </svg>
            <h2 class="text-xl font-semibold text-primary">Documentation</h2>
          </div>

          <!-- Page count badge -->
          <span
            class="text-xs bg-surface-alt px-2 py-1 rounded-full"
            :class="showSkeleton ? 'text-transparent animate-pulse' : 'text-tertiary'"
          >
            {{ showSkeleton ? '0 pages' : `${pages.length} page${pages.length !== 1 ? 's' : ''}` }}
          </span>
        </div>

        <!-- Card Grid or Skeleton -->
        <DocumentationCardSkeleton v-if="showSkeleton" :count="6" />
        <DocumentationCardGrid v-else :pages="pages" @create="handleCreatePage" />
      </div>
    </div>
  </div>
</template>
