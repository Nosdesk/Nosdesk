<script setup lang="ts">
import { formatDate, formatDateTime } from '@/utils/dateUtils';
import { ref, onMounted, watch, computed, onUnmounted, nextTick } from "vue";
import { useRoute, useRouter } from "vue-router";
import CollaborativeEditor from "@/components/CollaborativeEditor.vue";
import { useTitleManager } from "@/composables/useTitleManager";
import documentationService from "@/services/documentationService";
import ticketService from "@/services/ticketService";
import type { Article, Page, PageChild } from "@/services/documentationService";
import BackButton from '@/components/common/BackButton.vue';
import DeleteButton from '@/components/common/DeleteButton.vue';
import { useDocumentationNavStore } from "@/stores/documentationNav";
import DocumentationTocItem from '@/components/documentationComponents/DocumentationTocItem.vue';
import { docsEmitter } from "@/services/docsEmitter";
import RevisionHistory from '@/components/editor/RevisionHistory.vue';
import apiClient from '@/services/apiConfig';

const route = useRoute();
const router = useRouter();
const documentationNavStore = useDocumentationNavStore();
const article = ref<Article | null>(null);
const page = ref<Page | null>(null);
const pages = ref<Page[]>([]);
const pageParentMap = ref<Record<string, string | null>>({});
const isLoading = ref(true);
const isSaving = ref(false); // Still needed for create/delete operations (not editor saves)
const showSuccessMessage = ref(false); // Still needed for create/delete feedback (not editor saves)
const saveMessage = ref(""); // Still needed for create/delete feedback (not editor saves)
const titleManager = useTitleManager();
const isTicketNote = ref(false);
const isIndexPage = ref(false);
const ticketId = ref<string | null>(null);
const searchQuery = ref('');
const isEditing = ref(false);
const isCreateFromTicket = ref(false);
const searchResults = ref<Article[]>([]);
const selectedTicketId = ref<number | null>(null);
const searchDropdownVisible = ref(false);
const searchRef = ref<HTMLElement | null>(null);

// Content editing
const editContent = ref("");
const editTitle = ref("");

// Document icon
const documentIcon = ref('📄');

// Revision history state
const showRevisionHistory = ref(false);
const editorRef = ref<any>(null);

// Create a document object for the header
const documentObj = computed(() => {
  if (page.value) {
    return {
      id: String(page.value.id),
      title: page.value.title,
      icon: page.value.icon || '📄',
      slug: page.value.slug
    };
  } else if (!article.value) {
    return null;
  }
  
  return {
    id: String(article.value.id),
    title: editTitle.value || article.value.title,
    icon: documentIcon.value,
    slug: article.value.slug
  };
});

// Define emits for article data
const emit = defineEmits<{
  (e: 'update:title', title: string): void;
  (e: 'update:document', document: { id: string; title: string; icon: string; slug?: string } | null): void;
}>();

// Add a computed property to determine if we should show the page URL
const isMainDocumentationPage = computed(() => {
  return isIndexPage.value;
});

// Add a function to load the main documentation page
const loadAllPages = async () => {
  isLoading.value = true;
  try {
    // Get all pages from the API - they're already organized in a hierarchy
    const topLevelPages = await documentationService.getPages();
    console.log("Received organized pages:", topLevelPages);
    
    // Add additional debugging to check the page structure
    console.log("Debugging page structures:");
    topLevelPages.forEach((page, index) => {
      console.log(`Top level page ${index + 1}: ${page.title} (ID: ${page.id})`);
      if (page.children && page.children.length > 0) {
        console.log(`  Has ${page.children.length} children`);
        page.children.forEach((child, childIndex) => {
          console.log(`    Child ${childIndex + 1}: ${child.title} (ID: ${child.id})`);
        });
      } else {
        console.log(`  No children`);
      }
    });
    
    // Build a parent-child relationship map for navigation
    const parentMap: Record<string, string | null> = {};
    const hierarchyMap: Record<string, string[]> = {};
    
    // Function to build maps recursively
    const buildMaps = (page: Page, parentId: string | null = null) => {
      const pageId = String(page.id);
      parentMap[pageId] = parentId;
      
      // Initialize hierarchy entry
      if (!hierarchyMap[pageId]) {
        hierarchyMap[pageId] = [];
      }
      
      // Process children recursively
      if (page.children && page.children.length > 0) {
        page.children.forEach(child => {
          const childId = String(child.id);
          hierarchyMap[pageId].push(childId);
          buildMaps(child, pageId);
        });
      }
    };
    
    topLevelPages.forEach(page => buildMaps(page));
    
    // Store the parent map in a ref for later use
    pageParentMap.value = parentMap;
    
    // Update the store with the hierarchy
    documentationNavStore.updatePageHierarchy(hierarchyMap);
    
    // Set the pages for display
    pages.value = topLevelPages;
    
    console.log("Parent map:", parentMap);
    console.log("Hierarchy map:", hierarchyMap);
    
    isIndexPage.value = true;
    titleManager.setCustomTitle('Documentation');
    emit('update:title', 'Documentation');
  } catch (error) {
    console.error('Error loading pages:', error);
  } finally {
    isLoading.value = false;
  }
};

// Add a docId computed property for the CollaborativeEditor
const docId = computed(() => {
  // If viewing a ticket note directly
  if (isTicketNote.value && ticketId.value) {
    return `ticket-${ticketId.value}`;
  }

  // If documentation page is linked to a ticket, use ticket doc-id for shared content
  if (page.value?.ticket_id) {
    return `ticket-${page.value.ticket_id}`;
  }
  if (article.value?.ticket_id) {
    return `ticket-${article.value.ticket_id}`;
  }

  // Otherwise use documentation-specific doc-id (must use numeric ID, not UUID)
  if (page.value) {
    return `doc-${page.value.id}`;
  }
  if (article.value) {
    return `doc-${article.value.id}`;
  }

  return 'documentation-new';
});

// SIMPLIFIED: CollaborativeEditor handles all saves automatically via WebSocket
// No need for manual save logic - Yjs CRDT syncs in real-time

// Simple content update handler - just update local state
const updateContent = (newContent: string) => {
  editContent.value = newContent;

  // Update the local state for display purposes only
  // Actual persistence is handled by CollaborativeEditor's WebSocket connection
  if (article.value) {
    article.value.content = newContent;
  } else if (page.value) {
    page.value.content = newContent;
  }
};

// Handle title update
const updateTitle = (newTitle: string) => {
  editTitle.value = newTitle;

  // Update the title in the header
  if (article.value || page.value) {
    emit('update:title', newTitle);
    titleManager.setCustomTitle(newTitle);

    // Generate a slug from the title
    const slug = newTitle.toLowerCase().replace(/\s+/g, '-').replace(/[^a-z0-9-]/g, '');

    // Update the document object with new title and slug
    if (article.value) {
      article.value.title = newTitle;
      article.value.slug = slug;
    } else if (page.value) {
      page.value.title = newTitle;
      page.value.slug = slug;
    }

    // Save title/slug changes to database (metadata only, not content)
    saveTitleChanges();
  }
};

// Save title and metadata changes only (content is auto-synced via WebSocket)
const saveTitleChanges = async () => {
  if (!page.value && !article.value) return;

  // TODO: Implement metadata-only update endpoint in backend
  // For now, metadata updates happen when the page is saved through other means
  // The title/slug changes are tracked in local state and will be persisted
  // when the user navigates away or the page is updated
  console.log('Title changed to:', editTitle.value);
};

// Revision history handlers
const toggleRevisionHistory = () => {
  showRevisionHistory.value = !showRevisionHistory.value;
};

const handleSelectRevision = async (revisionNumber: number | null) => {
  if (!editorRef.value) return;

  if (revisionNumber === null) {
    // Exit revision view and return to live document
    editorRef.value.exitRevisionView();
    return;
  }

  try {
    // Get the page ID
    const pageId = page.value?.id || article.value?.id;
    if (!pageId) return;

    // Fetch the specific revision snapshot from the API
    const response = await apiClient.get(
      `/collaboration/docs/${pageId}/revisions/${revisionNumber}`
    );
    const revisionData = response.data;

    // Display the revision in the editor (read-only mode)
    editorRef.value.viewSnapshot(revisionData);
    console.log('Revision data received:', revisionData);
  } catch (error) {
    console.error('Failed to fetch revision:', error);
  }
};

const handleCloseRevisionHistory = () => {
  showRevisionHistory.value = false;
  // Also exit revision view if we're currently viewing one
  if (editorRef.value && editorRef.value.isViewingRevision) {
    editorRef.value.exitRevisionView();
  }
};

const handleRevisionRestored = () => {
  // Refresh the page after restoration
  fetchContent();
};

// Add a computed property for the fallback route
const fallbackRoute = computed(() => {
  if (isTicketNote.value && ticketId.value) {
    return `/tickets/${ticketId.value}`;
  } else if (!isIndexPage.value) {
    return '/documentation';
  } else {
    return '/';
  }
});

// Add a computed property for the back button label
const backButtonLabel = computed(() => {
  if (isTicketNote.value) {
    return 'Back to Ticket';
  } else if (!isIndexPage.value) {
    return 'Back to Documentation';
  } else {
    return 'Back to Dashboard';
  }
});

// Filtered pages for search
const filteredPages = computed(() => {
  if (!searchQuery.value) return [];
  
  const query = searchQuery.value.toLowerCase();
  const results: Array<{ id: string; title: string; description?: string; path?: string; icon?: string | undefined; isPage: boolean }> = [];
  
  // Helper function to recursively search through all pages and their children
  const searchPagesRecursively = (pageList: Page[], parentPath = '') => {
    pageList.forEach(page => {
      // Check if this page matches
      if (page.title.toLowerCase().includes(query)) {
        results.push({
          id: String(page.id),
          title: page.title,
          description: page.content?.substring(0, 100) || '',
          icon: page.icon || undefined,
          isPage: true
        });
      }
      
      // Recursively search children if they exist
      if (page.children && page.children.length > 0) {
        // Create the path for this level
        const currentPath = parentPath ? `${parentPath}/${page.slug || page.id}` : `${page.slug || page.id}`;
        
        // Search each child
        page.children.forEach(child => {
          if (child.title.toLowerCase().includes(query)) {
            results.push({
              id: String(child.id),
              title: child.title,
              path: `/documentation/${child.id}`,
              icon: child.icon || undefined,
              isPage: false
            });
          }
          
          // If this child has its own children, search those too
          if (child.children && child.children.length > 0) {
            searchPagesRecursively([child as Page], currentPath);
          }
        });
      }
    });
  };
  
  // Start recursive search with top-level pages
  searchPagesRecursively(pages.value);
  
  return results;
});

// Handle search input
const handleSearch = (query: string) => {
  searchQuery.value = query;
  searchDropdownVisible.value = query.length > 0;
  // No longer updating URL parameters to avoid any page refreshes
};

// Close search dropdown when clicking outside
const handleClickOutside = (event: MouseEvent) => {
  if (searchRef.value && !searchRef.value.contains(event.target as Node)) {
    searchDropdownVisible.value = false;
  }
};

// Create a new documentation page
const createNewPage = async () => {
  try {
    console.log("Creating new documentation page...");
    
    // Set loading state
    isSaving.value = true;
    saveMessage.value = "Creating new page...";
    showSuccessMessage.value = true;
    
    // Create a new page with all required fields matching the backend NewDocumentationPage struct
    const newPageData = {
      title: "New Documentation Page",
      content: "# New Documentation Page\n\nStart writing your documentation here...",
      description: "Add a description here",
      status: "draft",
      icon: "📄",
      slug: "new-documentation-page-" + Date.now(),
      // Let the service handle the timestamps
    };
    
    console.log("Sending page data:", newPageData);
    
    // Call the API
    const newPage = await documentationService.createArticle(newPageData);
    
    // Navigate to the new page
    if (newPage && newPage.id) {
      console.log("New page created successfully, navigating to:", newPage);
      
      // Emit an event that a new document was created
      docsEmitter.emit('doc:created', { id: newPage.id });
      
      // Update the sidebar via the documentation nav store
      documentationNavStore.refreshPages();
      
      // Navigate to the new page
      router.push(`/documentation/${newPage.id}`);
    } else {
      console.error("Failed to create new page: newPage is null or missing ID");
      // Show an error message to the user
      saveMessage.value = "Error creating new page";
      showSuccessMessage.value = true;
      setTimeout(() => {
        showSuccessMessage.value = false;
      }, 3000);
    }
  } catch (error) {
    console.error("Error creating new page:", error);
    // Show a more detailed error message
    if (error instanceof Error) {
      saveMessage.value = `Error: ${error.message}`;
    } else {
      saveMessage.value = "Error creating new page";
    }
    showSuccessMessage.value = true;
    setTimeout(() => {
      showSuccessMessage.value = false;
    }, 3000);
  } finally {
    // Reset loading state
    isSaving.value = false;
  }
};

// Create a documentation page from a ticket
const createFromTicket = async () => {
  if (!selectedTicketId.value) {
    return;
  }
  
  try {
    // Fetch the ticket to get its content
    const ticket = await ticketService.getTicketById(selectedTicketId.value);
    
    if (ticket) {
      // Create a new page with the ticket content
      const newPage = await documentationService.createArticle({
        title: `Documentation: ${ticket.title}`,
        description: `Documentation created from ticket #${ticket.id}`,
        content: ticket.article_content || '', // Use empty string if article_content is null
        lastUpdated: new Date().toISOString(),
        status: 'published',
        icon: 'mdi-text-box-outline'
      });
      
      // Navigate to the new page
      if (newPage) {
        router.push(`/documentation/${newPage.id}`);
      }
    } else {
      console.error('Ticket not found');
    }
  } catch (error) {
    console.error('Error creating documentation from ticket:', error);
  }
};

// Handle delete page
const handleDeletePage = async () => {
  if (!article.value && !page.value) {
    return;
  }
  
  try {
    isSaving.value = true;
    saveMessage.value = "Deleting document...";
    showSuccessMessage.value = true;
    
    const pageId = article.value?.id || page.value?.id;
    if (pageId) {
      const success = await documentationService.deleteArticle(pageId);
      
      if (success) {
        // Emit a delete event for the documentation nav store
        docsEmitter.emit('doc:deleted', { id: pageId });
        
        // Update the sidebar
        documentationNavStore.refreshPages();
        
        saveMessage.value = "Document deleted successfully";
        
        // Navigate back to documentation index after a short delay
        setTimeout(() => {
          router.push('/documentation');
        }, 1000);
      } else {
        saveMessage.value = "Error deleting document";
        setTimeout(() => {
          showSuccessMessage.value = false;
        }, 3000);
      }
    }
  } catch (error) {
    console.error('Error deleting page:', error);
    saveMessage.value = "Error deleting document";
    setTimeout(() => {
      showSuccessMessage.value = false;
    }, 3000);
  } finally {
    isSaving.value = false;
  }
};

// Modify the fetchContent method to load binary updates when appropriate
const fetchContent = async () => {
  isLoading.value = true;
  // Reset state for the new page
  isIndexPage.value = false;
  isTicketNote.value = false;
  page.value = null;
  article.value = null;
  
  // Check if we're creating from a ticket
  if (route.query.createFromTicket === 'true') {
    isCreateFromTicket.value = true;
    isLoading.value = false;
    return;
  }
  
  // Check if this is a ticket note (moved this check before the path check)
  if (route.query.ticketId) {
    const ticketIdParam = route.query.ticketId as string;
    console.log(`Loading ticket article for ticket ID: ${ticketIdParam}`);
    
    try {
      const ticket = await ticketService.getTicketById(Number(ticketIdParam));
      
      if (ticket) {
        article.value = {
          id: `ticket-note-${ticketIdParam}`,
          title: `Notes for Ticket #${ticket.id}`,
          description: `Documentation for ticket ${ticket.title}`,
          content: ticket.article_content || '', // Use empty string if article_content is null
          author: ticket.assignee || 'System',
          lastUpdated: ticket.modified,
          status: 'published',
          slug: '',  // Add missing required properties
          parent_id: null,
          icon: null
        };
        
        isTicketNote.value = true;
        ticketId.value = ticketIdParam;
        editContent.value = article.value.content || '';
        editTitle.value = article.value.title;
        documentIcon.value = article.value?.icon || 'mdi-text-box-outline';
        
        emit('update:title', article.value.title);
        // documentObj watcher will handle document updates
        
        isLoading.value = false;
        return;
      } else {
        console.error(`Ticket ${ticketIdParam} not found`);
      }
    } catch (error) {
      console.error(`Error loading ticket ${ticketIdParam}:`, error);
    }
  }
  
  // If we're on the main documentation route with no path, show the index page
  if (!route.params.path || route.params.path === '') {
    console.log('Loading main documentation index page');
    await loadAllPages();
    isLoading.value = false;
    return;
  }
  
  const path = route.params.path as string;
  console.log(`Loading documentation page with path: ${path}`);
  
  try {
    // Try to get the page by path
    const result = await documentationService.getPageByPath(path);
    
    if (result) {
      // If it's a Page with children (category)
      if ('children' in result && Array.isArray(result.children)) {
        page.value = result;
        editContent.value = page.value.content || '';
        editTitle.value = page.value.title;
        documentIcon.value = page.value.icon || 'mdi-folder-outline';
        
        emit('update:title', page.value.title);
        // documentObj watcher will handle document updates
      } 
      // If it's an Article or PageChild
      else if ('id' in result && (typeof result.id === 'string' || typeof result.id === 'number')) {
        // Fetch the full article content
        const articleData = await documentationService.getArticleById(String(result.id));
        
        if (articleData) {
          article.value = articleData;
          editContent.value = article.value.content || '';
          editTitle.value = article.value.title;
          documentIcon.value = article.value?.icon || 'mdi-text-box-outline';
          
          emit('update:title', article.value.title);
          // documentObj watcher will handle document updates
        } else {
          console.error(`Article with ID ${String(result.id)} not found`);
          router.push('/documentation');
          return;
        }
      } else {
        console.error('Invalid result object:', result);
        router.push('/documentation');
        return;
      }
    } else {
      console.error(`Page with path ${path} not found`);
      // Instead of showing an error, redirect to the documentation index
      router.push('/documentation');
      return;
    }
  } catch (error) {
    console.error('Error fetching content:', error);
    // On error, redirect to the documentation index
    router.push('/documentation');
    return;
  } finally {
    // CollaborativeEditor will automatically load and sync content via WebSocket
    // No need to manually fetch binary updates
    isLoading.value = false;
  }
};

// Watch for route changes to update content
watch(() => route.params.path, () => {
  fetchContent();
}, { immediate: true });

// Watch for search query in URL, but only on initial page load
watch(() => route.query.search, (newSearch) => {
  // Only update searchQuery from URL on initial load or navigation
  // This prevents loops when the user types in the search box
  if (newSearch && typeof newSearch === 'string' && searchQuery.value === '') {
    searchQuery.value = newSearch;
  } else if (!newSearch && route.path.startsWith('/documentation') && searchQuery.value !== '') {
    // Only clear search if navigating to a documentation page without search
    searchQuery.value = '';
  }
}, { immediate: true });

// Watch for createFromTicket query parameter
watch(() => route.query.createFromTicket, (newValue) => {
  isCreateFromTicket.value = newValue === 'true';
}, { immediate: true });

// Emit the document object when it changes
watch(documentObj, (newDocument) => {
  emit('update:document', newDocument);
}, { immediate: true });

// Computed properties
const flattenedPages = computed(() => {
  const flattened: Array<Page & { level: number }> = [];
  
  function flattenPage(page: Page, level = 0) {
    // Add the current page with its level
    flattened.push({ ...page, level });
    
    // Recursively add children if they exist
    if (page.children && Array.isArray(page.children)) {
      page.children.forEach(child => {
        if (child && typeof child === 'object' && 'id' in child) {
          flattenPage(child as Page, level + 1);
        }
      });
    }
  }
  
  // Process each top-level page
  pages.value.forEach(page => flattenPage(page));
  
  return flattened;
});

// Computed property for sorted pages
const sortedPages = computed(() => {
  // Create a flat array of all pages and their children
  const allPages = pages.value.reduce((acc: Page[], page) => {
    // Add the current page
    acc.push(page);
    // Add children if they exist
    if (page.children && Array.isArray(page.children)) {
      acc.push(...page.children.filter((child): child is Page => 
        child && typeof child === 'object' && 'id' in child
      ));
    }
    return acc;
  }, []);

  // Sort pages by lastUpdated date in descending order (most recent first)
  return allPages.sort((a, b) => {
    const dateA = new Date(a.lastUpdated || 0).getTime();
    const dateB = new Date(b.lastUpdated || 0).getTime();
    return dateB - dateA;
  });
});

onMounted(() => {
  // Check if we have a search query on mount
  const flattened: any[] = [];
  if (route.query.search === 'string') {
    searchQuery.value = route.query.search;
  }
  
  // Fetch content based on the route
  fetchContent();
});

// Remove click outside listener on component unmount
onUnmounted(() => {
  window.document.removeEventListener('click', handleClickOutside);
});
</script>

<template>
  <div class="bg-app flex flex-col h-full overflow-hidden">
    <!-- Back button and metadata bar with subtle gradient background -->
    <div class="bg-gradient-to-r from-bg-app to-bg-surface border-b border-default w-full">
      <!-- Using grid for 3-column layout with fr units for responsive design -->
      <div class="grid grid-cols-[1fr_2fr_1fr] w-full items-center px-6 py-3">
        <!-- Left column: Back button -->
        <div class="flex justify-start">
          <BackButton :fallbackRoute="fallbackRoute" :label="backButtonLabel" class="hover:scale-105 transition-transform duration-200" />
        </div>
        
        <!-- Center column: Search bar, using fr units for width -->
        <div v-if="isMainDocumentationPage" class="flex justify-center items-center" ref="searchRef">
          <div class="relative w-full max-w-3xl mx-auto">
            <div class="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 text-blue-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
              </svg>
            </div>
            <input
              v-model="searchQuery"
              type="text"
              placeholder="Search documentation..."
              class="w-full pl-10 pr-4 py-2 bg-surface/80 text-primary rounded-full placeholder-secondary focus:outline-none focus:ring-2 focus:ring-brand-blue border border-default shadow-lg transition-all duration-200 hover:border-brand-blue/50 focus:border-brand-blue"
              @input="handleSearch(searchQuery)"
              @focus="searchDropdownVisible = searchQuery.length > 0"
            />
            <div v-if="searchQuery" class="absolute inset-y-0 right-0 pr-3 flex items-center">
              <button
                @click="searchQuery = ''; searchDropdownVisible = false"
                class="text-secondary hover:text-primary p-1 rounded-full hover:bg-surface-hover transition-colors"
                aria-label="Clear search"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            </div>
          </div>
          
          <!-- Search Results as a dropdown -->
          <div v-if="searchQuery && searchDropdownVisible"
               class="absolute top-full left-1/2 transform -translate-x-1/2 mt-2 bg-surface border border-default rounded-lg shadow-xl z-50 max-h-96 overflow-y-auto w-full max-w-3xl">
            <div class="p-3 border-b border-default flex justify-between items-center">
              <h2 class="text-sm font-medium text-primary flex items-center gap-2">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 text-blue-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
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
            <div v-if="filteredPages.length === 0" class="p-6 text-center text-secondary text-sm">
              <div class="flex flex-col items-center gap-2">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-10 w-10 text-tertiary mb-2" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9.172 16.172a4 4 0 015.656 0M9 10h.01M15 10h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
                </svg>
                <p>No pages found for "<span class="text-primary">{{ searchQuery }}</span>"</p>
                <span class="text-xs text-tertiary mt-1">Try a different search term</span>
              </div>
            </div>

            <!-- Results list -->
            <div v-else class="divide-y divide-subtle">
              <div v-for="item in filteredPages" :key="item.id"
                   class="hover:bg-surface-hover transition-colors">
                <RouterLink 
                  :to="item.path ? `/documentation/${item.path}` : `/documentation/${item.id}`" 
                  class="flex items-start gap-3 p-4"
                  @click="searchDropdownVisible = false"
                >
                  <div class="text-xl flex-shrink-0 bg-surface-alt p-1.5 rounded text-center" style="min-width: 2rem">
                    {{ item.icon || '📄' }}
                  </div>
                  <div class="flex-1">
                    <h3 class="text-primary font-medium">{{ item.title }}</h3>
                    <p v-if="item.description" class="text-secondary text-xs mt-1 line-clamp-2">
                      {{ item.description }}
                    </p>
                    <div class="flex items-center gap-2 mt-2">
                      <span class="text-xs text-brand-blue bg-brand-blue/20 px-2 py-0.5 rounded">
                        {{ item.isPage ? 'Page' : 'Topic' }}
                      </span>
                    </div>
                  </div>
                </RouterLink>
              </div>
            </div>
          </div>
        </div>
        <div v-else></div> <!-- Empty placeholder when search isn't shown -->
        
        <!-- Right column: Actions -->
        <div class="flex justify-end items-center gap-3">
          <span v-if="isSaving" class="text-brand-blue flex items-center gap-1 text-xs">
            <svg class="animate-spin h-3 w-3" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
            Saving...
          </span>
          <DeleteButton
            v-if="(article || page) && !isTicketNote && !isIndexPage"
            :itemName="'documentation page'"
            @delete="handleDeletePage"
          />
        </div>
      </div>
    </div>

    <!-- Main content area -->
    <div class="relative flex flex-col flex-1 min-h-0 overflow-hidden bg-gradient-to-b from-bg-app to-bg-surface">
      <!-- Search Results - Removed from main content area -->

      <!-- Index Page View -->
      <div v-if="isIndexPage" class="flex flex-col max-w-5xl mx-auto w-full px-4 py-8 gap-8 animate-fadeIn overflow-auto">  
        <!-- Documentation header and controls -->
        <div class="flex flex-col md:flex-row md:justify-between md:items-center gap-4">
          <div class="flex flex-col gap-2">
            <h1 class="text-3xl font-bold text-primary flex items-center gap-3">
              <span class="text-brand-blue text-4xl">📚</span>
              Documentation
            </h1>
            <p class="text-primary text-base max-w-2xl">
              Browse and manage your documentation pages. Click on a page to view or edit it.
            </p>
          </div>
          <button
            @click="createNewPage"
            class="bg-brand-blue hover:bg-brand-blue/80 text-white px-6 py-2.5 rounded-lg flex items-center gap-2 text-sm font-medium shadow-lg transition-all duration-200 ease-in-out transform hover:scale-105"
            :disabled="isSaving"
          >
            <span v-if="isSaving" class="animate-spin rounded-full h-4 w-4 border-b-2 border-white"></span>
            <span v-else class="text-lg">+</span>
            <span>{{ isSaving ? 'Creating...' : 'New Page' }}</span>
          </button>
        </div>

        <!-- Recent Pages Section -->
        <div class="flex flex-col gap-4">
          <div class="flex items-center gap-2 pb-2 border-b border-default">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-brand-blue" viewBox="0 0 20 20" fill="currentColor">
              <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm1-12a1 1 0 10-2 0v4a1 1 0 00.293.707l2.828 2.829a1 1 0 101.415-1.415L11 9.586V6z" clip-rule="evenodd"/>
            </svg>
            <h2 class="text-lg font-medium text-primary">Recent Pages</h2>
          </div>

          <!-- List of pages -->
          <div class="flex flex-col gap-2">
            <RouterLink
              v-for="page in sortedPages"
              :key="page.id"
              :to="`/documentation/${page.id}`"
              class="bg-surface rounded-lg overflow-hidden border border-default transition-all duration-200 hover:border-brand-blue/30 hover:-translate-y-0.5 group focus:outline-none focus:ring-2 focus:ring-brand-blue/50"
            >
              <div class="p-4">
                <div class="flex items-center gap-3">
                  <div class="text-2xl flex-shrink-0">{{ page.icon || '📄' }}</div>
                  <div class="flex-1 min-w-0">
                    <h3 class="text-primary font-medium group-hover:text-brand-blue transition-colors">
                      {{ page.title }}
                    </h3>
                    <p v-if="page.description" class="text-secondary text-sm mt-1 line-clamp-2">
                      {{ page.description }}
                    </p>
                    <div class="flex items-center gap-3 mt-2 text-xs text-tertiary">
                      <span>{{ formatDate(page.updated_at || page.lastUpdated || new Date().toISOString()) }}</span>
                      <span>·</span>
                      <span>{{ page.last_edited_by?.name || page.created_by?.name || page.author || 'Unknown' }}</span>
                    </div>
                  </div>
                </div>
              </div>
            </RouterLink>
          </div>
        </div>

        <!-- Empty state for no pages -->
        <div v-if="pages.length === 0" class="text-center p-10 bg-surface rounded-xl shadow-lg border border-default mt-4">
          <div class="flex flex-col items-center gap-4">
            <div class="text-5xl mb-4">📝</div>
            <h3 class="text-xl font-semibold text-primary mb-2">No documentation yet</h3>
            <p class="text-secondary mb-6 max-w-md mx-auto">
              Create your first documentation page to start building a knowledge base.
            </p>
            <button
              @click="createNewPage"
              class="bg-brand-blue hover:bg-brand-blue/80 text-white px-6 py-3 rounded-lg text-sm font-medium flex items-center gap-2 shadow-lg"
            >
              <span class="text-lg">+</span>
              Create your first page
            </button>
          </div>
        </div>
      </div>

      <!-- Document Content View -->
      <div v-else-if="article || page" class="flex flex-1 w-full items-stretch animate-fadeIn overflow-hidden">
        <!-- Main Content Area - Scrollable -->
        <div class="flex-1 overflow-auto">
          <div class="w-full max-w-3xl mx-auto px-4 sm:px-6 lg:px-8 py-6 sm:py-8 flex flex-col">
            <!-- Linked Ticket Indicator -->
            <div
              v-if="page?.ticket_id || article?.ticket_id"
              class="flex items-center gap-2 px-4 py-2.5 mb-6 bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800 rounded-lg"
            >
            <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-blue-600 dark:text-blue-400" viewBox="0 0 20 20" fill="currentColor">
              <path d="M9 4.804A7.968 7.968 0 005.5 4c-1.255 0-2.443.29-3.5.804v10A7.969 7.969 0 015.5 14c1.669 0 3.218.51 4.5 1.385A7.962 7.962 0 0114.5 14c1.255 0 2.443.29 3.5.804v-10A7.968 7.968 0 0014.5 4c-1.255 0-2.443.29-3.5.804V12a1 1 0 11-2 0V4.804z" />
            </svg>
            <span class="text-sm text-blue-800 dark:text-blue-200">
              This documentation page is linked to
              <RouterLink
                :to="`/tickets/${page?.ticket_id || article?.ticket_id}`"
                class="font-medium underline hover:text-blue-600 dark:hover:text-blue-300"
              >
                Ticket #{{ page?.ticket_id || article?.ticket_id }}
              </RouterLink>
              and shares its content
            </span>
          </div>

          <!-- Documentation Header - Clean, modern styling -->
          <div class="mb-6">
            <!-- Icon and Title -->
            <div class="flex items-start gap-3 mb-4">
              <div class="text-2xl sm:text-3xl flex-shrink-0 select-none">{{ (page || article)?.icon || '📄' }}</div>
              <div class="flex-1 min-w-0">
                <h1
                  contenteditable="true"
                  @blur="updateTitle(($event.target as HTMLElement).textContent || '')"
                  @keydown.enter.prevent="($event.target as HTMLElement).blur()"
                  class="text-2xl sm:text-3xl font-bold text-primary break-words leading-tight tracking-tight outline-none focus:ring-1 focus:ring-brand-blue/30 rounded px-1 -mx-1"
                >
                  {{ (page || article)?.title || 'Untitled' }}
                </h1>
              </div>
            </div>

            <!-- Metadata bar - Subtle, minimal -->
            <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3 pt-4 pb-2 border-t border-subtle">
              <!-- Metadata -->
              <div class="flex flex-wrap items-center gap-x-3 gap-y-2 text-xs text-tertiary">
                <!-- Created By -->
                <div v-if="page?.created_by || article?.created_by" class="flex items-center gap-1.5">
                  <span class="text-secondary">{{ (page || article)?.created_by?.name || 'Unknown' }}</span>
                </div>

                <!-- Separator -->
                <span v-if="(page?.created_by || article?.created_by) && (page?.updated_at || article?.updated_at)" class="text-subtle">·</span>

                <!-- Last Updated -->
                <div v-if="page?.updated_at || article?.updated_at" class="flex items-center gap-1.5">
                  <span>{{ formatDate((page || article)?.updated_at || new Date().toISOString()) }}</span>
                </div>

                <!-- Last Edited By -->
                <template v-if="page?.last_edited_by || article?.last_edited_by">
                  <span class="text-subtle">·</span>
                  <span>Edited by {{ (page || article)?.last_edited_by?.name || 'Unknown' }}</span>
                </template>
              </div>

              <!-- Action Buttons -->
              <div class="flex items-center gap-2">
                <!-- Revision History Toggle -->
                <button
                  @click="toggleRevisionHistory"
                  class="px-3 py-1.5 text-xs rounded-md hover:bg-surface-hover transition-colors flex items-center gap-1.5 text-secondary hover:text-primary"
                  :class="{ 'bg-surface-alt text-primary': showRevisionHistory }"
                  title="Revision history"
                >
                  <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                  </svg>
                  <span>History</span>
                </button>
              </div>
            </div>
          </div>

            <CollaborativeEditor
              ref="editorRef"
              v-model="editContent"
              :doc-id="docId"
              :hide-revision-history="true"
              placeholder="Enter documentation content here..."
              @update:modelValue="updateContent"
              class="w-full flex-1 flex flex-col"
            />
          </div>
        </div>

        <!-- Revision History Sidebar -->
        <RevisionHistory
          v-if="showRevisionHistory && (page?.id || article?.id)"
          :document-id="Number(page?.id || article?.id)"
          @close="handleCloseRevisionHistory"
          @select-revision="handleSelectRevision"
          @restored="handleRevisionRestored"
          class="absolute top-0 right-0 bottom-0 z-10"
        />
      </div>

      <!-- Loading State -->
      <div
        v-else-if="isLoading"
        class="flex justify-center items-center h-full"
      >
        <div class="flex flex-col items-center gap-4">
          <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-brand-blue"></div>
          <div class="text-brand-blue animate-pulse">Loading content...</div>
        </div>
      </div>

      <!-- Create from ticket form -->
      <div v-else-if="isCreateFromTicket" class="max-w-4xl mx-auto w-full p-6 bg-surface rounded-lg shadow-lg mt-8 border border-default animate-fadeIn">
        <h1 class="text-2xl font-bold text-primary mb-6">Create Documentation from Ticket</h1>

        <div class="mb-6">
          <label for="ticketId" class="block text-sm font-medium text-primary mb-2">Ticket ID</label>
          <input
            type="number"
            id="ticketId"
            v-model="selectedTicketId"
            class="w-full px-4 py-2 bg-surface-alt text-primary rounded-md focus:outline-none focus:ring-2 focus:ring-brand-blue border border-default"
            placeholder="Enter ticket ID"
          />
        </div>

        <div class="flex justify-end">
          <button
            @click="createFromTicket"
            class="px-6 py-3 bg-brand-blue text-white rounded-md hover:bg-brand-blue/80 focus:outline-none focus:ring-2 focus:ring-brand-blue transform transition-all duration-200 hover:scale-105 shadow-lg flex items-center gap-2"
            :disabled="!selectedTicketId"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
              <path fill-rule="evenodd" d="M10 3a1 1 0 00-1 1v5H4a1 1 0 100 2h5v5a1 1 0 102 0v-5h5a1 1 0 100-2h-5V4a1 1 0 00-1-1z" clip-rule="evenodd" />
            </svg>
            Create Documentation
          </button>
        </div>
      </div>

      <!-- Not Found State -->
      <div v-else class="p-8 text-center text-secondary flex flex-col items-center gap-4 animate-fadeIn">
        <svg xmlns="http://www.w3.org/2000/svg" class="h-16 w-16 text-tertiary mb-2" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9.172 16.172a4 4 0 015.656 0M9 10h.01M15 10h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <h2 class="text-xl font-semibold text-primary">Document not found</h2>
        <p class="text-secondary max-w-md">The document you're looking for doesn't exist or has been moved.</p>
        <RouterLink to="/documentation" class="mt-4 text-brand-blue hover:text-brand-blue/80">
          Go to Documentation Home
        </RouterLink>
      </div>
    </div>

    <!-- Success message toast -->
    <div
      v-if="showSuccessMessage"
      class="fixed bottom-4 right-4 bg-status-success text-white px-4 py-2 rounded-md shadow-lg flex items-center gap-2 animate-fadeIn"
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
        <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z" clip-rule="evenodd" />
      </svg>
      {{ saveMessage }}
    </div>
  </div>
</template>

<style scoped>
@keyframes fadeIn {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}

.animate-fadeIn {
  animation: fadeIn 0.2s ease-out forwards;
}
</style>