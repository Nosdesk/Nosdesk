<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted, computed } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import documentationService from '@/services/documentationService'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import type { Page } from '@/services/documentationService'
import DocumentationNavItem from './DocumentationNavItem.vue'

// Define emitted events
defineEmits<{
  'search': [query: string];
}>();

// Define our own PageChild interface to match what we're using
interface PageChild {
  id: string | number;
  slug: string;
  title: string;
  icon: string | null;
  description: string | null;
  content: string;
  parent_id: string | number | null;
  author: string;
  status: string;
  children: PageChild[];
  lastUpdated?: string;
  ticket_id?: string | null;
}

const route = useRoute()
const router = useRouter()
const pages = ref<Page[]>([])
const isLoading = ref(true)
const docNavStore = useDocumentationNavStore()
const pageParentMap = ref<Record<string, string | null>>({})
const hierarchicalPages = ref<Page[]>([])

// Drag and Drop state
const draggedPageId = ref<string | number | null>(null);
const dropTargetId = ref<string | number | null>(null);
const dropPosition = ref<'above' | 'inside' | 'below' | null>(null);
const isDragging = ref(false);

// Load pages and organize them hierarchically
const loadPages = async () => {
  isLoading.value = true;
  try {
    // Try to get ordered top-level pages first
    let topLevelPages = await documentationService.getOrderedTopLevelPages();
    
    // If the ordered endpoint returns nothing (likely because display_order isn't set up yet),
    // fall back to getting all pages and organizing them manually
    if (!topLevelPages || topLevelPages.length === 0) {
      console.warn('Ordered endpoints returned no data, falling back to getPages');
      
      // Get all pages from the API
      const allPages = await documentationService.getPages();
      
      // Organize into a hierarchical structure
      const parentMap: Record<string, string | null> = {};
      const pageMap: Record<string, Page> = {};
      
      // First pass: create a map of all pages
      for (const page of allPages) {
        pageMap[String(page.id)] = {...page, children: []};
        parentMap[String(page.id)] = page.parent_id ? String(page.parent_id) : null;
      }
      
      // Second pass: organize into hierarchical structure
      topLevelPages = [];
      for (const page of allPages) {
        const pageId = String(page.id);
        const parentId = page.parent_id ? String(page.parent_id) : null;
        
        if (parentId === null) {
          // This is a top-level page
          topLevelPages.push(pageMap[pageId]);
        } else if (pageMap[parentId]) {
          // This is a child page and the parent exists
          if (!pageMap[parentId].children) {
            pageMap[parentId].children = [];
          }
          pageMap[parentId].children.push(pageMap[pageId]);
        }
      }
      
      // Sort by display_order in the fallback too
      const sortByOrder = (a: Page, b: Page) => {
        // Ensure display_order is treated as a number, default to 999 if not defined
        const orderA = a.display_order !== undefined && a.display_order !== null ? Number(a.display_order) : 999;
        const orderB = b.display_order !== undefined && b.display_order !== null ? Number(b.display_order) : 999;
        return orderA - orderB;
      };
      
      // Sort top-level pages
      topLevelPages.sort(sortByOrder);
      
      // Recursively sort children
      const sortChildrenRecursively = (page: Page) => {
        if (page.children && page.children.length > 0) {
          page.children.sort(sortByOrder);
          page.children.forEach(sortChildrenRecursively);
        }
      };
      
      // Apply sorting
      topLevelPages.forEach(sortChildrenRecursively);
      
      // Store the parent map
      pageParentMap.value = parentMap;
      
      // Update the pages ref
      pages.value = topLevelPages;
      
    } else {
      // For each top-level page, fetch its ordered children recursively
      await Promise.all(topLevelPages.map(async (page) => {
        await loadOrderedChildrenRecursively(page);
      }));
    }
    
    // Build a parent-child relationship map
    const parentMap: Record<string, string | null> = {};
    
    // Recursive function to build parent mapping
    const buildParentMap = (pages: Page[], parentId: string | number | null = null) => {
      for (const page of pages) {
        // Store parent relationship
        parentMap[String(page.id)] = parentId !== null ? String(parentId) : null;
        
        // Process children recursively
        if (page.children && page.children.length > 0) {
          buildParentMap(page.children, page.id);
        }
      }
    };
    
    // Build the parent map starting with top-level pages
    buildParentMap(topLevelPages);
    
    // Store the parent map
    pageParentMap.value = parentMap;
    
    // Update the pages ref
    pages.value = topLevelPages;
    
  } catch (error) {
    console.error('Error loading pages:', error);
    
    // If any error occurs, attempt to load unordered pages as a fallback
    try {
      const allPages = await documentationService.getPages();
      
      // Organize into a hierarchical structure
      const parentMap: Record<string, string | null> = {};
      const pageMap: Record<string, Page> = {};
      
      // First pass: create a map of all pages
      for (const page of allPages) {
        pageMap[String(page.id)] = {...page, children: []};
        parentMap[String(page.id)] = page.parent_id ? String(page.parent_id) : null;
      }
      
      // Second pass: organize into hierarchical structure
      const topLevelPages: Page[] = [];
      for (const page of allPages) {
        const pageId = String(page.id);
        const parentId = page.parent_id ? String(page.parent_id) : null;
        
        if (parentId === null) {
          // This is a top-level page
          topLevelPages.push(pageMap[pageId]);
        } else if (pageMap[parentId]) {
          // This is a child page and the parent exists
          if (!pageMap[parentId].children) {
            pageMap[parentId].children = [];
          }
          pageMap[parentId].children.push(pageMap[pageId]);
        }
      }
      
      // Sort by display_order in the fallback too
      const sortByOrder = (a: Page, b: Page) => {
        // Ensure display_order is treated as a number, default to 999 if not defined
        const orderA = a.display_order !== undefined && a.display_order !== null ? Number(a.display_order) : 999;
        const orderB = b.display_order !== undefined && b.display_order !== null ? Number(b.display_order) : 999;
        return orderA - orderB;
      };
      
      // Sort top-level pages
      topLevelPages.sort(sortByOrder);
      
      // Recursively sort children
      const sortChildrenRecursively = (page: Page) => {
        if (page.children && page.children.length > 0) {
          page.children.sort(sortByOrder);
          page.children.forEach(sortChildrenRecursively);
        }
      };
      
      // Apply sorting
      topLevelPages.forEach(sortChildrenRecursively);
      
      // Store the parent map
      pageParentMap.value = parentMap;
      
      // Update the pages ref
      pages.value = topLevelPages;
      
    } catch (fallbackError) {
      console.error('Error loading fallback pages:', fallbackError);
    }
  } finally {
    isLoading.value = false;
  }
};

// Helper function to recursively load ordered children for a page
const loadOrderedChildrenRecursively = async (page: Page) => {
  try {
    // Get ordered children for this page
    const orderedChildren = await documentationService.getOrderedPagesByParentId(page.id);
    
    if (orderedChildren && orderedChildren.length > 0) {
      // Set the children on the page
      page.children = orderedChildren;
      
      // Recursively load children for each child page
      await Promise.all(page.children.map(async (childPage) => {
        await loadOrderedChildrenRecursively(childPage);
      }));
    }
  } catch (error) {
    console.error(`Error loading ordered children for page ${page.id}:`, error);
    // If there's an error, leave the children as is
  }
};

// Check if an icon is an SVG
const isIconSvg = (icon: string | undefined): boolean => {
  return Boolean(icon && icon.startsWith('<svg'))
}

// Find parent pages for auto-expansion
const findParentPages = (targetPath: string): string[] => {
  const parents: string[] = []
  
  // Check if the path is in our parent map
  const pageId = targetPath.split('/').pop() || ''
  
  // Traverse up the parent chain
  let currentId = pageId
  while (pageParentMap.value[currentId]) {
    const parentId = pageParentMap.value[currentId]
    if (parentId) {
      parents.push(parentId)
      currentId = parentId
    } else {
      break
    }
  }
  
  return parents
}

// Handle page click - navigate to page or toggle expansion
const handlePageClick = (id: string | number) => {
  const stringId = String(id)

  // Always use ID for routing - slugs can contain invalid characters
  const pageRoute = `/documentation/${stringId}`

  // Find the page to check if it has children
  const foundPage = findPageById(pages.value, id)

  // If the page has children, handle expansion/collapse
  if (foundPage && foundPage.children && foundPage.children.length > 0) {
    // Check if we're already on the same route
    if (route.path === pageRoute) {
      // Only collapse if already on that page
      docNavStore.togglePage(stringId)
    } else {
      // Always expand if coming from another route
      docNavStore.expandPage(stringId)
    }
  }

  // Navigate to the page using ID
  router.push(pageRoute)
}

// Handle toggle expansion only (no navigation)
const handleToggleExpand = (id: string | number) => {
  const stringId = String(id)
  docNavStore.togglePage(stringId)
}

// Watch route changes to auto-expand pages
watch(() => route.path, (newPath) => {
  const parentPages = findParentPages(newPath)
  parentPages.forEach(pageId => {
    docNavStore.expandPage(pageId)
  })
})

// Handle window resize
const handleResize = () => {
  docNavStore.updateSidebarForScreenSize()
}

// Function to find a page by ID in the hierarchical pages structure
const findPageById = (pages: Page[], id: string | number): Page | null => {
  for (const page of pages) {
    if (String(page.id) === String(id)) {
      return page;
    }
    
    if (page.children && page.children.length > 0) {
      const foundInChildren = findPageById(page.children, id);
      if (foundInChildren) {
        return foundInChildren;
      }
    }
  }
  
  return null;
};

// Function to find the parent of a page by ID
const findParentPage = (pages: Page[], childId: string | number): Page | null => {
  for (const page of pages) {
    if (page.children && page.children.some(child => String(child.id) === String(childId))) {
      return page;
    }
    
    if (page.children && page.children.length > 0) {
      const foundInChildren = findParentPage(page.children, childId);
      if (foundInChildren) {
        return foundInChildren;
      }
    }
  }
  
  return null;
};

// Handle page drag start
const handlePageDragStart = (id: string | number, event: DragEvent) => {
  draggedPageId.value = id;
  isDragging.value = true;
};

// Handle page drag end
const handlePageDragEnd = () => {
  isDragging.value = false;
  draggedPageId.value = null;
  dropTargetId.value = null;
  dropPosition.value = null;
};

// Handle page drag over
const handlePageDragOver = (id: string | number, event: DragEvent, position: 'above' | 'inside' | 'below') => {
  if (String(draggedPageId.value) === String(id)) {
    // Can't drop onto itself
    return;
  }
  
  // Prevent dropping a page into its own child (would create infinite loop)
  const draggedPage = findPageById(pages.value, draggedPageId.value as string | number);
  if (!draggedPage) return;
  
  if (position === 'inside') {
    const childPages = getAllChildrenIds(draggedPage);
    if (childPages.includes(String(id))) {
      return;
    }
  }
  
  dropTargetId.value = id;
  dropPosition.value = position;
};

// Function to get all children IDs recursively
const getAllChildrenIds = (page: Page): string[] => {
  const ids: string[] = [];
  
  if (page.children && page.children.length > 0) {
    for (const child of page.children) {
      ids.push(String(child.id));
      ids.push(...getAllChildrenIds(child));
    }
  }
  
  return ids;
};

// Handle page drop
const handlePageDrop = async (id: string | number, event: DragEvent, position: 'above' | 'inside' | 'below') => {
  if (String(draggedPageId.value) === String(id)) {
    // Can't drop onto itself
    return;
  }
  
  if (!draggedPageId.value || !position) {
    return;
  }

  try {
    // Get the target page
    const targetPage = findPageById(pages.value, id);
    if (!targetPage) return;
    
    // Get the parent of the target page
    const targetParent = findParentPage(pages.value, id);
    const targetParentId = targetParent ? targetParent.id : null;
    
    if (position === 'inside') {
      // Move the dragged page to be a child of the target page
      await documentationService.movePage(
        draggedPageId.value,
        id,
        0 // First position inside the target
      );
      
      // Expand the target to show the newly nested page
      docNavStore.expandPage(String(id));
    } else {
      // Get the siblings of the target
      let siblings: Page[] = [];
      
      if (targetParentId) {
        // Get the parent's children
        const parent = findPageById(pages.value, targetParentId);
        if (parent && parent.children) {
          siblings = [...parent.children];
        }
      } else {
        // This is a top-level page, siblings are other top-level pages
        siblings = pages.value;
      }
      
      // Find the index of the target in its siblings
      const targetIndex = siblings.findIndex(p => String(p.id) === String(id));
      if (targetIndex === -1) return;
      
      // Move the dragged page to be before or after the target
      const newIndex = position === 'above' ? targetIndex : targetIndex + 1;
      
      // Create page orders with the new position
      const pageOrders = siblings
        .filter(p => String(p.id) !== String(draggedPageId.value)) // Remove the dragged page from its current position
        .map((p, i) => {
          if (i >= newIndex) {
            // Shift pages down
            return { page_id: Number(p.id), display_order: i + 1 };
          }
          return { page_id: Number(p.id), display_order: i };
        });
      
      // Insert the dragged page at the new position
      pageOrders.splice(newIndex, 0, { page_id: Number(draggedPageId.value), display_order: newIndex });
      
      // Reorder the pages
      await documentationService.reorderPages(
        targetParentId || null, // Use null for top-level pages
        pageOrders
      );
    }
    
    // Reload the pages with the new structure
    await loadPages();
  } catch (error) {
    console.error('Error dropping page:', error);
  } finally {
    // Reset drag state
    handlePageDragEnd();
  }
};

onMounted(async () => {
  await loadPages()
  
  // Set initial sidebar state based on screen size
  docNavStore.updateSidebarForScreenSize()

  // Add resize event listener
  window.addEventListener('resize', handleResize)
  
  // Auto-expand parents of the current page
  const currentPageId = route.path.split('/').pop() || ''
  if (currentPageId) {
    // Expand the current page if it has children
    const currentPage = findPageById(pages.value, currentPageId);
    if (currentPage && currentPage.children && currentPage.children.length > 0) {
      docNavStore.expandPage(currentPageId)
    }
    
    // Expand all parent pages
    docNavStore.expandParents(currentPageId, pageParentMap.value)
  }
})

// Clean up event listeners when component is unmounted
onUnmounted(() => {
  window.removeEventListener('resize', handleResize)
})

// Create a method to reload the sidebar
const reloadSidebar = async () => {
  console.log('Reloading documentation sidebar...');
  await loadPages();
};

// Export the reloadSidebar method to make it accessible to other components
defineExpose({
  reloadSidebar
});

// Watch for changes to the needsRefresh flag
watch(() => docNavStore.needsRefresh, (needsRefresh) => {
  if (needsRefresh) {
    console.log('Documentation navigation refresh requested');
    loadPages();
  }
});
</script>

<template>
  <div class="documentation-nav relative">
    <div v-if="isLoading" class="p-2 text-center">
      <div class="animate-pulse">
        <div class="h-3 bg-surface rounded w-3/4 mx-auto mb-1"></div>
        <div class="h-3 bg-surface rounded w-1/2 mx-auto mb-1"></div>
        <div class="h-3 bg-surface rounded w-2/3 mx-auto"></div>
      </div>
    </div>
    <div v-else class="relative">
      <ul class="flex flex-col gap-1 p-1 scrollbar-thin scrollbar-track-transparent scrollbar-thumb-border-default scrollbar-thumb-rounded">
        <DocumentationNavItem
          v-for="page in pages"
          :key="page.id"
          :page="page"
          :level="0"
          :is-dragging="String(draggedPageId) === String(page.id)"
          :is-drop-target="String(dropTargetId) === String(page.id) && dropPosition === 'inside'"
          :is-drop-above="String(dropTargetId) === String(page.id) && dropPosition === 'above'"
          :is-drop-below="String(dropTargetId) === String(page.id) && dropPosition === 'below'"
          @toggle-expand="handleToggleExpand"
          @page-click="handlePageClick"
          @drag-start="handlePageDragStart"
          @drag-end="handlePageDragEnd"
          @drag-over="handlePageDragOver"
          @drop="handlePageDrop"
        />
      </ul>
    </div>
  </div>
</template>

<style scoped>
/* Only keep the transition for drag overlay since it's not easily done with Tailwind */
.documentation-nav {
  transition: opacity 0.2s ease;
}

.documentation-nav.dragging {
  opacity: 0.8;
}
</style>