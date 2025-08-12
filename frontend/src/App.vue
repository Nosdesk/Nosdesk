// App.vue
<script setup lang="ts">
import { RouterView, useRoute, useRouter } from 'vue-router'
import { computed, ref, onMounted } from 'vue'
import Navbar from './components/Navbar.vue'
import PageHeader from './components/SiteHeader.vue'
import { useTitleManager } from '@/composables/useTitleManager'
import authService from '@/services/authService'

const route = useRoute()
const isBlankLayout = computed(() => route.meta.layout === 'blank')

// State for navbar collapse
const navbarCollapsed = ref(false)
const handleNavCollapse = (collapsed: boolean) => {
  navbarCollapsed.value = collapsed
}

// Use the centralized title manager
const titleManager = useTitleManager();

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

// Computed properties for responsive layout
const contentPadding = computed(() => {
  // Only apply left padding on large screens (lg)
  if (navbarCollapsed.value) {
    return 'lg:pl-16'; // 4rem when navbar is collapsed on desktop
  } else {
    return 'lg:pl-64'; // 16rem when navbar is expanded on desktop
  }
})

const headerLeft = computed(() => {
  // On desktop, position header relative to sidebar
  // On mobile, position from edge of screen
  if (navbarCollapsed.value) {
    return 'lg:left-16 left-0'; // 4rem when navbar is collapsed on desktop
  } else {
    return 'lg:left-64 left-0'; // 16rem when navbar is expanded on desktop
  }
})

// Security: Check if system requires initial setup on app initialization
const router = useRouter();
const initializationChecked = ref(false);

onMounted(async () => {
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

  <!-- Default layout with responsive navigation -->
  <div v-else class="flex w-full h-screen bg-slate-900 overflow-hidden">
    <!-- Navbar component (includes both desktop sidebar and mobile bottom nav) -->
    <Navbar @update:collapsed="handleNavCollapse" />
    
    <!-- Main content area with responsive padding -->
    <div class="flex flex-col w-full h-screen transition-all duration-300 ease-in-out" :class="contentPadding">
      <!-- Fixed header that adjusts with navbar -->
      <PageHeader 
        class="not-print:fixed top-0 z-10 border-b border-slate-600 bg-slate-800 transition-all duration-300 ease-in-out right-0"
        :class="[
          { 'left-0': true },
          { 'lg:left-16': navbarCollapsed },
          { 'lg:left-64': !navbarCollapsed }
        ]"
        :useRouteTitle="!isDocumentationPage"
        :title="titleManager.pageTitle.value"
        :showCreateButton="true"
        :ticket="titleManager.currentTicket.value"
        :document="titleManager.currentDocument.value"
        :is-transitioning="titleManager.isTransitioning.value"
        :pageUrl="currentPageUrl"
        :navbarCollapsed="navbarCollapsed"
        @update-ticket-title="titleManager.updateTicketTitle"
        @preview-ticket-title="titleManager.previewTicketTitle"
        @update-document-title="titleManager.updateDocumentTitle"
        @preview-document-title="titleManager.previewDocumentTitle"
        @update-document-icon="titleManager.updateDocumentIcon"
      />
      
      <!-- Scrollable content with bottom padding for mobile nav -->
      <main class="flex-1 not-print:pt-16 overflow-hidden pb-16 lg:pb-0">
        <RouterView 
          v-slot="{ Component }" 
          @update:ticket="titleManager.setTicket"
          @update:document="titleManager.setDocument"
          @update:title="titleManager.setCustomTitle"
        >
          <Transition 
            name="fade" 
            mode="out-in"
            @before-enter="titleManager.startTransition"
            @after-enter="titleManager.endTransition"
            @before-leave="titleManager.startTransition"
            @after-leave="() => {
              if (route.name !== 'ticket') titleManager.clearTicket();
              if (route.name !== 'documentation-article') titleManager.clearDocument();
              titleManager.endTransition();
            }"
          >
            <component :is="Component" :key="$route.fullPath" class="h-full overflow-auto" />
          </Transition>
        </RouterView>
      </main>
    </div>
  </div>
</template>

<style>
/* Global styles */
html, body {
  background-color: rgb(15 23 42); /* bg-slate-900 */
  min-height: 100vh;
}

/* Custom scrollbar styles */
::-webkit-scrollbar {
  width: 0.875rem;  /* 14px at default font size */
  height: 0.875rem;
}

::-webkit-scrollbar-track {
  background: rgb(30 41 59); /* bg-slate-800 */
}

::-webkit-scrollbar-thumb {
  background: rgb(100 116 139); /* bg-slate-500 */
  border-radius: 0.25rem;
}

::-webkit-scrollbar-thumb:hover {
  background: rgb(148 163 184); /* bg-slate-400 */
}

/* Firefox scrollbar styles */
* {
  scrollbar-width: auto; /* Changed from 'thin' to 'auto' for wider scrollbar */
  scrollbar-color: rgb(100 116 139) rgb(30 41 59); /* thumb track */
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