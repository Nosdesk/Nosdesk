// App.vue
<script setup lang="ts">
import { RouterView, useRoute, useRouter } from 'vue-router'
import { computed, ref, onMounted } from 'vue'
import Navbar from './components/Navbar.vue'
import PageHeader from './components/SiteHeader.vue'
import MobileSearchBar from './components/MobileSearchBar.vue'
import { useTitleManager } from '@/composables/useTitleManager'
import { useMobileSearch } from '@/composables/useMobileSearch'
import { useCursorScanlines } from '@/composables/useCursorScanlines'
import { useCrtEffect } from '@/composables/useCrtEffect'
import authService from '@/services/authService'
import { useBrandingStore } from '@/stores/branding'

// Initialize branding store and load config
const brandingStore = useBrandingStore()

const route = useRoute()
const isBlankLayout = computed(() => route.meta.layout === 'blank')

// State for navbar collapse
const navbarCollapsed = ref(false)
const handleNavCollapse = (collapsed: boolean) => {
  navbarCollapsed.value = collapsed
}

// Use the centralized title manager
const titleManager = useTitleManager();

// Mobile search bar state - used for conditional padding
const { isActive: isMobileSearchActive } = useMobileSearch();

// Red-horizon theme effects
useCursorScanlines();  // Crosshair lines following cursor
useCrtEffect();        // Full-screen CRT monitor effect

// Handle route-based ticket information
const ticketInfo = computed(() => {
  // Handle ticket routes
  if (route.name === 'ticket' && route.params.id) {
    const title = route.meta.pageTitle as string || '';
    // Remove the ticket number from the title if it exists
    const titleWithoutNumber = title.replace(/^#\d+\s*/, '');
    return {
      id: Number(route.params.id),
      title: titleWithoutNumber
    }
  }
  
  return null
})

// Computed property to determine if we're on a documentation page
const isDocumentationPage = computed(() => {
  return route.name === 'documentation-article';
});

// Computed property for the current page URL (for display purposes)
const currentPageUrl = computed(() => {
  // Only show URL for certain pages
  if (route.name === 'settings' || route.name === 'profile') {
    return window.location.href;
  }
  return undefined;
});

// No need for complex computed properties - flexbox handles it automatically

// Security: Check if system requires initial setup on app initialization
const router = useRouter();
const initializationChecked = ref(false);

// Ref to access the current route view component
const currentViewComponent = ref<any>(null);

// Computed property for create button text from route meta
const createButtonText = computed(() => {
  return route.meta.createButtonText || 'Create Ticket';
});

// Handle create button click using route meta configuration
const handleCreateClick = () => {
  const actionName = route.meta.createButtonAction;

  // If route specifies an action and the component has that method, call it
  if (actionName && currentViewComponent.value?.[actionName]) {
    currentViewComponent.value[actionName]();
  }
  // Otherwise, the SiteHeader's default ticket creation will handle it
};

onMounted(async () => {
  // Load branding configuration (public endpoint, no auth required)
  brandingStore.loadBranding();

  // Security: Prevent multiple initialization checks
  if (initializationChecked.value) {
    return;
  }

  try {
    // Only check if we're not already on onboarding or login pages
    if (route.name !== 'onboarding' && route.name !== 'login') {
      console.log('🔄 App: Checking setup status on initialization...');
      const setupStatus = await authService.checkSetupStatus();
      if (setupStatus.requires_setup) {
        console.log('🔄 App: System requires setup, redirecting to onboarding');
        router.push({ name: 'onboarding' });
      }
    }
  } catch (error) {
    console.error('Failed to check setup status on app initialization:', error);
    // Security: Don't redirect on error - let the router guard handle it
  } finally {
    initializationChecked.value = true;
  }
});


</script>

<template>
  <!-- Blank layout for login -->
  <RouterView v-if="isBlankLayout" />

  <!-- Default layout with responsive navigation - Simple flexbox layout -->
  <div v-else v-twemoji class="flex w-full h-full bg-app overflow-hidden">
    <!-- Sidebar (includes both sidebar and mobile bottom nav) -->
    <Navbar @update:collapsed="handleNavCollapse" />

    <!-- Main content area - takes remaining space -->
    <div class="flex flex-col flex-1 min-w-0">
      <!-- Header - sticky at top of content area -->
      <PageHeader
        class="flex-shrink-0 border-b border-default bg-surface"
        :useRouteTitle="!isDocumentationPage"
        :title="titleManager.pageTitle.value"
        :showCreateButton="true"
        :createButtonText="createButtonText"
        :ticket="titleManager.currentTicket.value"
        :device="titleManager.currentDevice.value"
        :document="titleManager.currentDocument.value"
        :is-transitioning="titleManager.isTransitioning.value"
        :pageUrl="currentPageUrl"
        :navbarCollapsed="navbarCollapsed"
        @update-document-title="titleManager.updateDocumentTitle"
        @preview-document-title="titleManager.previewDocumentTitle"
        @update-document-icon="titleManager.updateDocumentIcon"
        @update-ticket-title="titleManager.updateTicketTitle"
        @create="handleCreateClick"
      />

      <!-- Mobile Search Bar (positioned above bottom nav) -->
      <MobileSearchBar />

      <!-- Scrollable content with bottom padding for mobile nav (+ search bar when active) -->
      <main
        class="flex-1 overflow-hidden sm:pb-0"
        :class="isMobileSearchActive ? 'pb-[calc(6.5rem+env(safe-area-inset-bottom))]' : 'pb-[calc(3rem+env(safe-area-inset-bottom))]'"
      >
        <RouterView
          v-slot="{ Component }"
          @update:ticket="titleManager.setTicket"
          @update:device="titleManager.setDevice"
          @update:document="titleManager.setDocument"
          @update:title="titleManager.setCustomTitle"
        >
          <KeepAlive :include="['TicketsListView', 'UsersListView', 'DevicesListView', 'ProjectsView']">
            <component :is="Component" :key="$route.name" ref="currentViewComponent" class="h-full overflow-auto" />
          </KeepAlive>
        </RouterView>
      </main>
    </div>
  </div>
</template>

<style>
/* Global styles - note: html/body height and overflow are set in main.css */

/* Custom scrollbar styles */
::-webkit-scrollbar {
  width: 0.875rem;  /* 14px at default font size */
  height: 0.875rem;
}

::-webkit-scrollbar-track {
  background: var(--color-bg-surface);
}

::-webkit-scrollbar-thumb {
  background: var(--color-text-tertiary);
  border-radius: 0.25rem;
}

::-webkit-scrollbar-thumb:hover {
  background: var(--color-text-secondary);
}

/* Firefox scrollbar styles */
* {
  scrollbar-width: auto; /* Changed from 'thin' to 'auto' for wider scrollbar */
  scrollbar-color: var(--color-text-tertiary) var(--color-bg-surface); /* thumb track */
}

/* Ensure reduced motion preferences are respected */
@media (prefers-reduced-motion: reduce) {
  ::-webkit-scrollbar-thumb {
    transition: none;
  }
}

/* Adjust scrollbar size for smaller screens */
@media (max-width: 640px) {
  ::-webkit-scrollbar {
    width: 0.75rem;  /* 12px at default font size */
    height: 0.75rem;
  }
}

/* Fade transition for page changes */
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.15s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>