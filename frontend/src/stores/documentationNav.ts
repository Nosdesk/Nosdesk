import { defineStore } from 'pinia'
import { ref, watch } from 'vue'
import type { StarredPageInfo } from '@/services/documentationService'
import { findInTree } from '@/utils/treeUtils'
import { BREAKPOINTS } from '@/composables/useMobileDetection'

interface ExpandedState {
  [pageId: string]: boolean;
}

// Page interface matching the documentation service
interface NavPage {
  id: string | number;
  slug: string;
  title: string;
  icon: string | null;
  parent_id: string | number | null;
  display_order?: number;
  children: NavPage[];
  [key: string]: any; // Allow other properties
}

export const useDocumentationNavStore = defineStore('documentationNav', () => {
  // State for expanded pages
  const expandedPages = ref<ExpandedState>({})

  // State for sidebar visibility
  const isSidebarOpen = ref(false)

  // State for current page path
  const currentPagePath = ref<string[]>([])

  // State for page hierarchy
  const pageHierarchy = ref<Record<string, string[]>>({})

  // Counter for refreshing the navigation (incremented on each refresh request)
  const needsRefresh = ref(0)

  // Centralized pages state - the single source of truth for nav pages
  const pages = ref<NavPage[]>([])

  // Loading state
  const isLoading = ref(false)

  // Starred pages state
  const starredPages = ref<StarredPageInfo[]>([])
  const isStarredExpanded = ref(localStorage.getItem('docNavStarredExpanded') !== 'false')

  // Initialize from localStorage if available
  try {
    const savedExpanded = localStorage.getItem('docNavExpandedPages')
    if (savedExpanded) expandedPages.value = JSON.parse(savedExpanded)
  } catch { /* ignore corrupted data */ }

  try {
    const savedSidebar = localStorage.getItem('docNavSidebarOpen')
    if (savedSidebar) {
      isSidebarOpen.value = JSON.parse(savedSidebar)
    } else {
      // Default to open on desktop, closed on mobile
      isSidebarOpen.value = window.innerWidth >= BREAKPOINTS.md
    }
  } catch {
    isSidebarOpen.value = window.innerWidth >= BREAKPOINTS.md
  }
  
  // Save to localStorage when updated
  watch(expandedPages, (newState) => {
    localStorage.setItem('docNavExpandedPages', JSON.stringify(newState))
  }, { deep: true })
  
  watch(isSidebarOpen, (newState) => {
    localStorage.setItem('docNavSidebarOpen', JSON.stringify(newState))
  })
  
  // Increment-based refresh: each call bumps the counter so watchers always detect a change
  const refreshPages = () => {
    needsRefresh.value++
  }
  
  // Check if refresh is needed (non-zero means a refresh was requested)
  const isRefreshNeeded = () => {
    return needsRefresh.value > 0
  }
  
  // Toggle page expansion
  const togglePage = (pageId: string) => {
    expandedPages.value = {
      ...expandedPages.value,
      [pageId]: !expandedPages.value[pageId]
    }
  }
  
  // Expand a specific page
  const expandPage = (pageId: string) => {
    expandedPages.value[pageId] = true
  }
  
  // Collapse a specific page
  const collapsePage = (pageId: string) => {
    expandedPages.value[pageId] = false
  }
  
  // Expand all parents of a page
  const expandParents = (pageId: string, parentMap: Record<string, string | null>) => {
    let currentId = pageId;
    
    while (parentMap[currentId]) {
      const parentId = parentMap[currentId];
      if (parentId) {
        expandPage(parentId);
        currentId = parentId;
      } else {
        break;
      }
    }
  }
  
  // Set the current page path
  const setCurrentPagePath = (path: string[]) => {
    currentPagePath.value = path;
  }
  
  // Update page hierarchy
  const updatePageHierarchy = (hierarchy: Record<string, string[]>) => {
    pageHierarchy.value = hierarchy;
  }
  
  // Get children of a page
  const getChildrenOfPage = (pageId: string): string[] => {
    return pageHierarchy.value[pageId] || [];
  }
  
  // Toggle sidebar visibility
  const toggleSidebar = () => {
    isSidebarOpen.value = !isSidebarOpen.value
  }
  
  // Open sidebar
  const openSidebar = () => {
    isSidebarOpen.value = true
  }
  
  // Close sidebar
  const closeSidebar = () => {
    isSidebarOpen.value = false
  }
  
  // Set sidebar state based on screen size
  const updateSidebarForScreenSize = () => {
    const isMobile = window.innerWidth < BREAKPOINTS.md
    isSidebarOpen.value = !isMobile
  }

  // Set pages (used by DocumentationNav to initialize/reload)
  const setPages = (newPages: NavPage[]) => {
    pages.value = newPages
  }

  // Set loading state
  const setLoading = (loading: boolean) => {
    isLoading.value = loading
  }

  // Update a specific field on a page reactively (no API call, just state update)
  const updatePageField = (pageId: string | number, field: string, value: any) => {
    const page = findInTree(pages.value, pageId)
    if (page) {
      page[field] = value
    }
  }

  // Starred pages actions
  const setStarredPages = (pages: StarredPageInfo[]) => {
    starredPages.value = pages
  }

  const addStarredPage = (page: StarredPageInfo) => {
    starredPages.value = [page, ...starredPages.value]
  }

  const removeStarredPage = (pageId: number) => {
    starredPages.value = starredPages.value.filter(p => p.page_id !== pageId)
  }

  const toggleStarredExpanded = () => {
    isStarredExpanded.value = !isStarredExpanded.value
    localStorage.setItem('docNavStarredExpanded', String(isStarredExpanded.value))
  }

  return {
    // State
    expandedPages,
    isSidebarOpen,
    currentPagePath,
    pageHierarchy,
    needsRefresh,
    pages,
    isLoading,
    starredPages,
    isStarredExpanded,

    // Legacy refresh (still needed for structural changes like drag-drop)
    refreshPages,
    isRefreshNeeded,

    // Page state management
    setPages,
    setLoading,
    updatePageField,

    // Expansion state
    togglePage,
    expandPage,
    collapsePage,
    expandParents,
    setCurrentPagePath,
    updatePageHierarchy,
    getChildrenOfPage,

    // Sidebar state
    toggleSidebar,
    openSidebar,
    closeSidebar,
    updateSidebarForScreenSize,

    // Starred pages
    setStarredPages,
    addStarredPage,
    removeStarredPage,
    toggleStarredExpanded,
  }
})