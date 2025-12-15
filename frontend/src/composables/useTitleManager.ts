import { ref, computed, watch } from 'vue';
import { useRoute } from 'vue-router';
import ticketService from '@/services/ticketService';
import { useRecentTicketsStore } from '@/stores/recentTickets';
import apiClient from '@/services/apiConfig';
import { useDocumentationNavStore } from '@/stores/documentationNav';

export interface TitleableDocument {
  id: string;
  title: string;
  icon: string;
  slug?: string;
}

export interface TitleableTicket {
  id: number;
  title: string;
  [key: string]: any; // Allow other properties from the reactive ticket object
}

export interface TitleableDevice {
  id: number;
  hostname: string;
  [key: string]: any; // Allow other properties from the reactive device object
}

// Singleton state - shared across all useTitleManager() calls
// This ensures all components see the same reactive state (Vue 3 best practice for shared state)
const currentTicket = ref<TitleableTicket | null>(null);
const currentDevice = ref<TitleableDevice | null>(null);
const currentDocument = ref<TitleableDocument | null>(null);
const documentationTitle = ref<string | null>(null);
const customTitle = ref<string | null>(null);
const isTransitioning = ref(false);

export function useTitleManager() {
  const route = useRoute();

  // Computed properties
  const isTicketView = computed(() => currentTicket.value !== null);
  const isDeviceView = computed(() => currentDevice.value !== null);
  const isDocumentView = computed(() => currentDocument.value !== null);
  
  const pageTitle = computed(() => {
    // Custom title takes precedence if set
    if (customTitle.value) {
      return customTitle.value;
    }

    // Documentation title is next in priority
    if (isDocumentView.value && documentationTitle.value) {
      return documentationTitle.value;
    }

    // Ticket title with ID
    if (isTicketView.value && currentTicket.value) {
      return `#${currentTicket.value.id} ${currentTicket.value.title}`;
    }

    // Device title with ID
    if (isDeviceView.value && currentDevice.value) {
      return `#${currentDevice.value.id} ${currentDevice.value.hostname}`;
    }

    // Get title from route meta
    const routeTitle = route.meta?.title as string;

    // Update document title
    const finalTitle = routeTitle || 'Nosdesk';
    document.title = `${finalTitle} | Nosdesk`;

    return finalTitle;
  });
  
  // Watch for route changes to clear stale state
  // Only clear when navigating to routes that don't have their own title management
  const titleManagedRoutes = ['ticket', 'device', 'documentation-article'];
  watch(
    () => route.name,
    (newRouteName) => {
      // Only clear all state when navigating to a route without title management
      // This prevents flash during transitions between title-managed routes
      if (!titleManagedRoutes.includes(newRouteName as string)) {
        currentTicket.value = null;
        currentDevice.value = null;
        currentDocument.value = null;
        documentationTitle.value = null;
        customTitle.value = null;
      }
    }
  );

  // Watch for changes in the current ticket's title (reactive updates)
  watch(
    () => currentTicket.value?.title,
    (newTitle) => {
      if (currentTicket.value && newTitle !== undefined) {
        // Automatically update the document title when the ticket title changes
        document.title = `#${currentTicket.value.id} ${newTitle} | Nosdesk`;

        // Also update the recent tickets store
        const recentTicketsStore = useRecentTicketsStore();
        recentTicketsStore.updateTicketData(currentTicket.value.id, {
          title: newTitle
        });
      }
    }
  );
  
  // Methods
  const setCustomTitle = (title: string | null) => {
    customTitle.value = title;
    if (title) {
      document.title = `${title} | Nosdesk`;
    }
  };
  
  const setTicket = (ticketData: TitleableTicket | null) => {
    // Store the actual reactive ticket object reference
    currentTicket.value = ticketData;
    // The document title will automatically update when ticketData.title changes
    // since we're watching the reactive object
    if (ticketData) {
      document.title = `#${ticketData.id} ${ticketData.title} | Nosdesk`;
    }
  };
  
  const setDevice = (deviceData: TitleableDevice | null) => {
    // Store the actual reactive device object reference
    currentDevice.value = deviceData;
    if (deviceData) {
      document.title = `#${deviceData.id} ${deviceData.hostname} | Nosdesk`;
    }
  };

  const setDocument = (documentData: TitleableDocument | null) => {
    currentDocument.value = documentData;
    if (documentData) {
      documentationTitle.value = documentData.title;
      setCustomTitle(documentData.title);
    }
    // Note: Clearing is handled by clearDocument() called from App.vue on route leave
  };
  
  // Preview the ticket title as the user types (real-time updates)
  const previewTicketTitle = (newTitle: string) => {
    if (currentTicket.value) {
      // Update the title in the reactive ticket object
      // The watcher will automatically handle document title and recent tickets updates
      currentTicket.value.title = newTitle;
    }
  };
  
  // Preview the document title as the user types (real-time updates)
  const previewDocumentTitle = (newTitle: string) => {
    if (currentDocument.value) {
      // Update the title in the current document for UI display
      currentDocument.value.title = newTitle;
      documentationTitle.value = newTitle;
      
      // Update the document title for real-time feedback
      document.title = `${newTitle} | Nosdesk`;
    }
  };
  
  const updateTicketTitle = async (newTitle: string) => {
    if (currentTicket.value) {
      if (import.meta.env.DEV) {
        console.log(`useTitleManager: Updating ticket #${currentTicket.value.id} title to "${newTitle}"`);
      }

      // Update the title in the reactive ticket object
      // The watcher will automatically handle document title and recent tickets updates
      currentTicket.value.title = newTitle;

      // Save the title change to the backend
      try {
        await ticketService.updateTicket(currentTicket.value.id, { title: newTitle });
        if (import.meta.env.DEV) {
          console.log(`useTitleManager: Successfully updated ticket title in backend`);
        }
      } catch (error) {
        console.error(`Error updating ticket title:`, error);
        // If the backend update fails, we don't revert the UI changes
        // because the user might still be editing the title
      }
    }
  };
  
  const updateDocumentTitle = async (newTitle: string) => {
    if (currentDocument.value) {
      if (import.meta.env.DEV) {
        console.log(`useTitleManager: Updating document title to "${newTitle}"`);
      }

      // Update the title in the current document for UI display
      currentDocument.value.title = newTitle;
      documentationTitle.value = newTitle;
      setCustomTitle(newTitle);

      // Generate a slug from the title
      const newSlug = newTitle.toLowerCase().replace(/\s+/g, '-').replace(/[^a-z0-9-]/g, '');

      try {
        const currentDocId = currentDocument.value.id;

        // Use simple PUT request with only title and slug - this triggers SSE broadcast
        await apiClient.put(`/documentation/pages/${currentDocId}`, {
          title: newTitle,
          slug: newSlug,
        });

        if (import.meta.env.DEV) {
          console.log(`useTitleManager: Successfully updated document title and slug in backend`);
        }

        // Update the slug in the current document
        if (currentDocument.value) {
          currentDocument.value.slug = newSlug;
        }

        // Update sidebar reactively for the current user (SSE won't do this since it filters out same-user updates)
        const documentationNavStore = useDocumentationNavStore();
        documentationNavStore.updatePageField(currentDocId, 'title', newTitle);
        documentationNavStore.updatePageField(currentDocId, 'slug', newSlug);
      } catch (error) {
        console.error('Error updating document title and slug:', error);
      }
    }
  };
  
  const updateDocumentIcon = async (newIcon: string) => {
    if (currentDocument.value) {
      if (import.meta.env.DEV) {
        console.log(`useTitleManager: Updating document icon to "${newIcon}"`);
      }

      // Update the icon in the current document for UI display
      currentDocument.value.icon = newIcon;

      try {
        const currentDocId = currentDocument.value.id;

        // Use simple PUT request with only icon - this triggers SSE broadcast
        await apiClient.put(`/documentation/pages/${currentDocId}`, {
          icon: newIcon,
        });

        if (import.meta.env.DEV) {
          console.log(`useTitleManager: Successfully updated document icon in backend`);
        }

        // Update sidebar reactively for the current user (SSE won't do this since it filters out same-user updates)
        const documentationNavStore = useDocumentationNavStore();
        documentationNavStore.updatePageField(currentDocId, 'icon', newIcon);
      } catch (error) {
        console.error('Error updating document icon:', error);
      }
    }
  };
  
  const startTransition = () => {
    isTransitioning.value = true;
  };
  
  const endTransition = () => {
    isTransitioning.value = false;
  };
  
  const clearTicket = () => {
    currentTicket.value = null;
  };
  
  const clearDevice = () => {
    currentDevice.value = null;
  };

  const clearDocument = () => {
    currentDocument.value = null;
    documentationTitle.value = null;
    customTitle.value = null;
  };

  return {
    // State
    currentTicket,
    currentDevice,
    currentDocument,
    documentationTitle,
    isTransitioning,

    // Computed
    pageTitle,
    isTicketView,
    isDeviceView,
    isDocumentView,

    // Methods
    setCustomTitle,
    setTicket,
    setDevice,
    setDocument,
    previewTicketTitle,
    previewDocumentTitle,
    updateTicketTitle,
    updateDocumentTitle,
    updateDocumentIcon,
    startTransition,
    endTransition,
    clearTicket,
    clearDevice,
    clearDocument
  };
} 