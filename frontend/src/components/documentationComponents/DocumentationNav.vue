<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted, computed } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import documentationService from '@/services/documentationService'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import type { Page } from '@/services/documentationService'
import DocumentationNavItem from './DocumentationNavItem.vue'
import { storeToRefs } from 'pinia'
import { useSSE } from '@/services/sseService'

// Define emitted events
defineEmits<{
  'search': [query: string];
}>();

// PageChild interface for internal component use
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
const docNavStore = useDocumentationNavStore()

// SSE for real-time updates
const { addEventListener, removeEventListener } = useSSE()

// Handle SSE documentation updates - update sidebar reactively
const handleDocumentationUpdate = (event: any) => {
  // SSE events come as {type: "DocumentationUpdated", data: {...}} from backend
  const data = event.data || event;

  console.log('[DocumentationNav] SSE event received:', {
    rawEvent: event,
    extractedData: data,
    field: data?.field,
    value: data?.value,
    document_id: data?.document_id,
    pagesCount: pages.value?.length,
  });

  // Update the sidebar reactively for title/icon changes
  if (data.field === 'title' || data.field === 'icon') {
    console.log('[DocumentationNav] Updating sidebar field:', data.field, '=', data.value, 'for page', data.document_id);
    docNavStore.updatePageField(data.document_id, data.field, data.value);
  }
};

// Use store's reactive pages and loading state
const { pages, isLoading } = storeToRefs(docNavStore)

const pageParentMap = ref<Record<string, string | null>>({})
const hierarchicalPages = ref<Page[]>([])

// Drag and Drop state
const draggedPageId = ref<string | number | null>(null);
const dropTargetId = ref<string | number | null>(null);
const dropPosition = ref<'above' | 'inside' | 'below' | null>(null);
const isDragging = ref(false);

// Single global drop indicator state
const navRef = ref<HTMLElement | null>(null);
const dropIndicatorY = ref<number | null>(null);
const dropIndicatorIndent = ref<number>(8);

// Load pages and organize them hierarchically
const loadPages = async () => {
  // Only show loading state if we don't have cached pages
  const hasCachedPages = pages.value && pages.value.length > 0;
  if (!hasCachedPages) {
    docNavStore.setLoading(true);
  }
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
      
      // Update the pages in the store
      docNavStore.setPages(topLevelPages);

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

    // Update the pages in the store
    docNavStore.setPages(topLevelPages);

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

      // Update the pages in the store
      docNavStore.setPages(topLevelPages);

    } catch (fallbackError) {
      console.error('Error loading fallback pages:', fallbackError);
    }
  } finally {
    docNavStore.setLoading(false);
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
    // Check if already on the same route
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
  dropIndicatorY.value = null;
};

// Check if dropping would create a circular reference
const wouldCreateCircularReference = (draggedId: string | number, targetId: string | number, position: 'above' | 'inside' | 'below'): boolean => {
  // Can't drop onto itself
  if (String(draggedId) === String(targetId)) {
    return true;
  }

  const draggedPage = findPageById(pages.value, draggedId);
  if (!draggedPage) return true;

  // Get all descendants of the dragged page
  const descendantIds = getAllChildrenIds(draggedPage);

  // For 'inside' position: can't drop into any descendant
  if (position === 'inside') {
    return descendantIds.includes(String(targetId));
  }

  // For 'above' or 'below' positions: can't drop as sibling of any descendant
  // because that would change the parent to the descendant's parent
  if (descendantIds.includes(String(targetId))) {
    return true;
  }

  return false;
};

// Handle page drag over
const handlePageDragOver = (id: string | number, event: DragEvent, position: 'above' | 'inside' | 'below', level: number = 0) => {
  // Check for circular reference
  if (wouldCreateCircularReference(draggedPageId.value as string | number, id, position)) {
    // Reset drop indicators to show this is invalid
    dropTargetId.value = null;
    dropPosition.value = null;
    dropIndicatorY.value = null;
    return;
  }

  dropTargetId.value = id;
  dropPosition.value = position;

  // Calculate the global indicator position for above/below
  if (position === 'above' || position === 'below') {
    const targetElement = event.currentTarget as HTMLElement;
    if (targetElement && navRef.value) {
      const targetRect = targetElement.getBoundingClientRect();
      const navRect = navRef.value.getBoundingClientRect();

      // Position at top or bottom of the target element
      const yPos = position === 'above'
        ? targetRect.top - navRect.top
        : targetRect.bottom - navRect.top;

      dropIndicatorY.value = yPos;
      dropIndicatorIndent.value = 8 + (level * 8);
    }
  } else {
    // For 'inside' position, hide the line indicator
    dropIndicatorY.value = null;
  }
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
  if (!draggedPageId.value || !position) {
    return;
  }

  // Final safety check for circular reference
  if (wouldCreateCircularReference(draggedPageId.value, id, position)) {
    console.warn('Prevented circular reference in page hierarchy');
    handlePageDragEnd();
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
      // For 'above' or 'below' positions, the dragged page becomes a sibling of the target
      // This means it should have the same parent as the target

      // Get the current parent of the dragged page
      const draggedPageCurrentParent = findParentPage(pages.value, draggedPageId.value as string | number);
      const draggedPageCurrentParentId = draggedPageCurrentParent ? draggedPageCurrentParent.id : null;

      // Check if the dragged page needs to change its parent
      const needsParentChange = String(draggedPageCurrentParentId) !== String(targetParentId);

      // Get the siblings of the target (which will become siblings of the dragged page)
      let siblings: Page[] = [];

      if (targetParentId) {
        // Get the parent's children
        const parent = findPageById(pages.value, targetParentId);
        if (parent && parent.children) {
          siblings = [...parent.children];
        }
      } else {
        // This is a top-level page, siblings are other top-level pages
        siblings = [...pages.value];
      }

      // Find the index of the target in its siblings
      const targetIndex = siblings.findIndex(p => String(p.id) === String(id));
      if (targetIndex === -1) return;

      // Calculate the new position
      const newIndex = position === 'above' ? targetIndex : targetIndex + 1;

      if (needsParentChange) {
        // First, move the page to the new parent with the correct display order
        await documentationService.movePage(
          draggedPageId.value,
          targetParentId, // null for top-level
          newIndex
        );

        // Then reorder all siblings to ensure correct order
        // Filter out the dragged page from current siblings (it's already been moved)
        const siblingsWithoutDragged = siblings.filter(p => String(p.id) !== String(draggedPageId.value));

        // Build the page orders including the newly moved page
        const pageOrders: { page_id: number, display_order: number }[] = [];
        let orderIndex = 0;

        for (let i = 0; i < siblingsWithoutDragged.length; i++) {
          if (orderIndex === newIndex) {
            // Insert the dragged page at this position
            pageOrders.push({ page_id: Number(draggedPageId.value), display_order: orderIndex });
            orderIndex++;
          }
          pageOrders.push({ page_id: Number(siblingsWithoutDragged[i].id), display_order: orderIndex });
          orderIndex++;
        }

        // If newIndex is at the end, add the dragged page
        if (newIndex >= siblingsWithoutDragged.length) {
          pageOrders.push({ page_id: Number(draggedPageId.value), display_order: orderIndex });
        }

        await documentationService.reorderPages(targetParentId || null, pageOrders);
      } else {
        // Same parent - just reorder
        const pageOrders = siblings
          .filter(p => String(p.id) !== String(draggedPageId.value))
          .map((p, i) => {
            if (i >= newIndex) {
              return { page_id: Number(p.id), display_order: i + 1 };
            }
            return { page_id: Number(p.id), display_order: i };
          });

        // Insert the dragged page at the new position
        pageOrders.splice(newIndex, 0, { page_id: Number(draggedPageId.value), display_order: newIndex });

        await documentationService.reorderPages(targetParentId || null, pageOrders);
      }
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

  // Register SSE listener for documentation updates
  addEventListener('documentation-updated' as any, handleDocumentationUpdate);

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
  removeEventListener('documentation-updated' as any, handleDocumentationUpdate);
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
  <nav ref="navRef" class="documentation-nav" :class="{ 'is-dragging': isDragging }">
    <!-- Single Global Drop Indicator -->
    <div
      v-if="dropIndicatorY !== null && isDragging"
      class="drop-indicator"
      :style="{
        top: `${dropIndicatorY}px`,
        left: `${dropIndicatorIndent}px`,
      }"
    >
      <div class="drop-indicator-dot"></div>
    </div>

    <!-- Loading State (only shown on initial load when no cached pages) -->
    <div v-if="isLoading && pages.length === 0" class="py-1 px-2 space-y-1">
      <div v-for="i in 5" :key="i" class="flex items-center gap-1.5 py-1">
        <div class="w-3 h-3 rounded bg-surface-hover/50 animate-pulse"></div>
        <div class="flex-1 h-3 rounded bg-surface-hover/60 animate-pulse" :style="{ maxWidth: `${70 + (i % 3) * 10}%` }"></div>
      </div>
    </div>

    <!-- Navigation Tree -->
    <ul v-else class="flex flex-col py-1">
      <DocumentationNavItem
        v-for="page in pages"
        :key="page.id"
        :page="page"
        :level="0"
        :dragged-page-id="draggedPageId"
        :is-dragging="String(draggedPageId) === String(page.id)"
        :is-drop-target="String(dropTargetId) === String(page.id) && dropPosition === 'inside'"
        @toggle-expand="handleToggleExpand"
        @page-click="handlePageClick"
        @drag-start="handlePageDragStart"
        @drag-end="handlePageDragEnd"
        @drag-over="(id, event, position, level) => handlePageDragOver(id, event, position, level)"
        @drop="handlePageDrop"
      />
    </ul>

    <!-- Empty State -->
    <div v-if="!isLoading && pages.length === 0" class="px-4 py-8 text-center">
      <div class="text-tertiary text-sm">No documents yet</div>
    </div>
  </nav>
</template>

<style scoped>
.documentation-nav {
  @apply relative;
}

.documentation-nav.is-dragging {
  @apply select-none;
}

/* Single global drop indicator */
.drop-indicator {
  position: absolute;
  right: 8px;
  height: 2px;
  background-color: #3b82f6;
  border-radius: 1px;
  pointer-events: none;
  z-index: 50;
  transform: translateY(-1px);
  transition: top 0.1s ease-out, left 0.1s ease-out;
}

.drop-indicator-dot {
  position: absolute;
  left: -3px;
  top: -3px;
  width: 8px;
  height: 8px;
  background-color: #3b82f6;
  border-radius: 50%;
}
</style>