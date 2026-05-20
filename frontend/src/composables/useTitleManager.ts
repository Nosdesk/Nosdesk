import { ref, computed, watch, type ComputedRef } from 'vue';
import { useRoute } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useRecentTicketsStore } from '@/stores/recentTickets';
import { useBrandingStore } from '@/stores/branding';

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
  name: string;
  attributes?: Record<string, unknown>;
  [key: string]: any; // Allow other properties from the reactive device object
}

// Save handler types - view components register these to handle persistence
type TicketTitleSaveHandler = (title: string) => Promise<void>;
type DocumentTitleSaveHandler = (title: string) => Promise<void>;
type DocumentIconSaveHandler = (icon: string) => Promise<void>;

// Singleton state - shared across all useTitleManager() calls
const currentTicket = ref<TitleableTicket | null>(null);
const currentDevice = ref<TitleableDevice | null>(null);
const currentDocument = ref<TitleableDocument | null>(null);
const documentationTitle = ref<string | null>(null);
const customTitle = ref<string | null>(null);
const isTransitioning = ref(false);

// Save handler refs - view components register callbacks for persistence
const ticketTitleSaveHandler = ref<TicketTitleSaveHandler | null>(null);
const documentTitleSaveHandler = ref<DocumentTitleSaveHandler | null>(null);
const documentIconSaveHandler = ref<DocumentIconSaveHandler | null>(null);

// Module-level computeds (no route/component dependency)
const isTicketView = computed(() => currentTicket.value !== null);
const isDeviceView = computed(() => currentDevice.value !== null);
const isDocumentView = computed(() => currentDocument.value !== null);

// Module-level watcher for recent tickets store (no route dependency)
watch(
  () => currentTicket.value?.title,
  (newTitle) => {
    if (currentTicket.value && newTitle !== undefined) {
      const recentTicketsStore = useRecentTicketsStore();
      recentTicketsStore.updateTicketData(currentTicket.value.id, {
        title: newTitle
      });
    }
  }
);

// Guard: route-dependent watchers and computed are created only once
let pageTitle: ComputedRef<string> | null = null;

export function useTitleManager() {
  const route = useRoute();
  const brandingStore = useBrandingStore();
  const fluent = useFluent();
  const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args);
  const getAppName = () => brandingStore.appName;

  // Create route-dependent computed + watchers only on first call
  if (!pageTitle) {
    pageTitle = computed(() => {
      if (customTitle.value) {
        return customTitle.value;
      }
      if (isDocumentView.value && documentationTitle.value) {
        return documentationTitle.value;
      }
      if (isTicketView.value && currentTicket.value) {
        return `#${currentTicket.value.id} ${currentTicket.value.title}`;
      }
      if (isDeviceView.value && currentDevice.value) {
        return `#${currentDevice.value.id} ${(currentDevice.value.attributes?.hostname as string | undefined)}`;
      }
      // Prefer `titleKey` (translatable). Routes that set a literal
      // `title` (e.g. a document title containing user content) still
      // fall through. Final fallback is the app name.
      const titleKey = route.meta?.titleKey as string | undefined;
      const titleKeyArgs = route.meta?.titleKeyArgs as
        | Record<string, string | number>
        | undefined;
      if (titleKey) {
        return t(titleKey, titleKeyArgs);
      }
      return (route.meta?.title as string) || getAppName();
    });

    // Single watcher for document.title updates
    watch(pageTitle, (title) => {
      document.title = `${title} | ${getAppName()}`;
    }, { immediate: true });

    // Watch for route changes to clear stale state
    const titleManagedRoutes = ['ticket', 'device', 'documentation-article'];
    watch(
      () => route.name,
      (newRouteName) => {
        if (!titleManagedRoutes.includes(newRouteName as string)) {
          currentTicket.value = null;
          currentDevice.value = null;
          currentDocument.value = null;
          documentationTitle.value = null;
          customTitle.value = null;
        }
      }
    );
  }

  // Methods
  const setCustomTitle = (title: string | null) => {
    customTitle.value = title;
  };

  const setTicket = (ticketData: TitleableTicket | null) => {
    currentTicket.value = ticketData;
  };

  const setDevice = (deviceData: TitleableDevice | null) => {
    currentDevice.value = deviceData;
  };

  const setDocument = (documentData: TitleableDocument | null) => {
    currentDocument.value = documentData;
    if (documentData) {
      documentationTitle.value = documentData.title;
      setCustomTitle(documentData.title);
    }
  };

  const previewTicketTitle = (newTitle: string) => {
    if (currentTicket.value) {
      currentTicket.value.title = newTitle;
    }
  };

  const previewDocumentTitle = (newTitle: string) => {
    if (currentDocument.value) {
      currentDocument.value.title = newTitle;
      documentationTitle.value = newTitle;
    }
  };

  const updateTicketTitle = async (newTitle: string) => {
    if (currentTicket.value) {
      currentTicket.value.title = newTitle;
      await ticketTitleSaveHandler.value?.(newTitle);
    }
  };

  const updateDocumentTitle = async (newTitle: string) => {
    if (currentDocument.value) {
      currentDocument.value.title = newTitle;
      documentationTitle.value = newTitle;
      setCustomTitle(newTitle);
      await documentTitleSaveHandler.value?.(newTitle);
    }
  };

  const updateDocumentIcon = async (newIcon: string) => {
    if (currentDocument.value) {
      currentDocument.value.icon = newIcon;
      await documentIconSaveHandler.value?.(newIcon);
    }
  };

  const onTicketTitleSave = (handler: TicketTitleSaveHandler | null) => {
    ticketTitleSaveHandler.value = handler;
  };

  const onDocumentTitleSave = (handler: DocumentTitleSaveHandler | null) => {
    documentTitleSaveHandler.value = handler;
  };

  const onDocumentIconSave = (handler: DocumentIconSaveHandler | null) => {
    documentIconSaveHandler.value = handler;
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
    pageTitle: pageTitle!,
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
    onTicketTitleSave,
    onDocumentTitleSave,
    onDocumentIconSave,
    startTransition,
    endTransition,
    clearTicket,
    clearDevice,
    clearDocument
  };
}
