// App.vue
<script setup lang="ts">
import { RouterView, useRoute, useRouter } from 'vue-router'
import { computed, ref, onMounted, watch, nextTick, defineAsyncComponent } from 'vue'
import { needsServerSelection } from '@/platform/serverGate'
// Native first-run server picker; lazy so the web bundle doesn't carry it.
const ConnectServerView = defineAsyncComponent(() => import('@/views/ConnectServerView.vue'))
import { useFluent } from 'fluent-vue'
import Navbar from './components/Navbar.vue'
import PageHeader from './components/SiteHeader.vue'
import MobileSearchBar from './components/MobileSearchBar.vue'
import ToastContainer from './components/common/ToastContainer.vue'
import RouteProgress from './components/common/RouteProgress.vue'
import LoadingSpinner from './components/common/LoadingSpinner.vue'
import { useWorkspaceSwitch } from '@/composables/useWorkspaceSwitch'
import { GlobalSearchModal } from './components/GlobalSearch'
import { useTitleManager } from '@/composables/useTitleManager'
import { useMobileSearch } from '@/composables/useMobileSearch'
import { useCursorScanlines } from '@/composables/useCursorScanlines'
import { useCrtEffect } from '@/composables/useCrtEffect'
import { useSnowfall } from '@/composables/useSnowfall'
import { useFavicon } from '@/composables/useFavicon'
import { useNotificationSSE } from '@/composables/useNotificationSSE'
import { useTicketDeletionCleanup } from '@/composables/useTicketDeletionCleanup'
import { setMentionNavigationHandler } from '@/plugins/prosemirror-mention-view'
import authService from '@nosdesk/core/services/authService'
import { useBrandingStore } from '@/stores/branding'
import { loadPlugins, initializeEventDispatcher } from '@/plugins'
import { useAuthStore } from '@/stores/auth'
import { usePageActionsStore } from '@nosdesk/core/stores/pageActions'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

// Initialize branding store and load config
const brandingStore = useBrandingStore()

// Reactive favicon management - watches branding store for changes
useFavicon(() => brandingStore.faviconUrl)

const route = useRoute()
const { isSwitchingWorkspace } = useWorkspaceSwitch()
const router = useRouter()
const isBlankLayout = computed(() => route.meta.layout === 'blank')

// Accessibility: announce route changes to screen readers
const routeAnnouncement = ref('')
router.afterEach((to) => {
  nextTick(() => {
    // Prefer `titleKey` (translatable); fall back to legacy `title`,
    // then to whatever the title manager wrote into document.title.
    const titleKey = to.meta?.titleKey as string | undefined
    const titleKeyArgs = to.meta?.titleKeyArgs as Record<string, string | number> | undefined
    const title = titleKey
      ? t(titleKey, titleKeyArgs)
      : (to.meta?.title as string) || document.title.replace(/\s*\|.*$/, '')
    routeAnnouncement.value = ''
    // Reset then set to trigger aria-live announcement
    requestAnimationFrame(() => {
      routeAnnouncement.value = t('nav-route-announcement', { title })
    })
  })
})

// Set up global mention click navigation
setMentionNavigationHandler((uuid: string) => {
  router.push(`/users/${uuid}`)
})

// State for navbar collapse
const navbarCollapsed = ref(false)
const handleNavCollapse = (collapsed: boolean) => {
  navbarCollapsed.value = collapsed
}

// Use the centralized title manager
const titleManager = useTitleManager();

// Mobile search bar state - used for conditional padding
const { isActive: isMobileSearchActive } = useMobileSearch();

// Theme-specific visual effects
useCursorScanlines();  // Red-horizon: Crosshair lines following cursor
useCrtEffect();        // Red-horizon: Full-screen CRT monitor effect
useSnowfall();         // Christmas: Ambient falling snow

// Real-time notification handling
useNotificationSSE();
// App-level SSE listener that wipes local artefacts (collab IDB
// cache, comment draft, pending attachments) for any ticket the
// server announces as deleted, see useTicketDeletionCleanup.
useTicketDeletionCleanup();

// Computed property to determine if on a documentation page
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
const initializationChecked = ref(false);

// Computed properties for create button from route meta.
// Prefer `createButtonTextKey` (FTL key) so the label translates with
// the user's locale; fall back to the legacy `createButtonText` string
// for routes that haven't been migrated yet.
const createButtonText = computed(() => {
  const key = route.meta.createButtonTextKey as string | undefined;
  if (key) return t(key);
  return (route.meta.createButtonText as string | undefined) || t('header-create-ticket');
});

const createButtonIcon = computed(() => route.meta.createButtonIcon ?? 'plus');

// Title icon from route meta (e.g., 'pdf' for PDF viewer)
const titleIcon = computed(() => route.meta.titleIcon as string | undefined);

// Visibility stays driven by route meta so first paint shows the
// right label before the view's onMounted has fired. The handler
// itself is registered by the view via `usePageCreateAction(...)`,
// which writes to `pageActions.createAction`. This replaces the
// old `defineExpose({ handleCreate }) + currentViewComponent.value
// ?.[methodName]?.()` indirection.
const showCreateButton = computed(() => !!(route.meta.createButtonTextKey || route.meta.createButtonText));

const pageActions = usePageActionsStore();
const handleCreateClick = () => {
  void pageActions.invokeCreate();
};

// Initialize plugins after authentication
const authStore = useAuthStore();
let eventDispatcherCleanup: (() => void) | null = null;

// Watch auth state and load plugins when authenticated
// immediate: true handles initial state, watch handles subsequent changes
watch(
  () => authStore.isAuthenticated,
  async (isAuthenticated, wasAuthenticated) => {
    if (isAuthenticated && !wasAuthenticated) {
      try {
        await loadPlugins();
        eventDispatcherCleanup = initializeEventDispatcher();
      } catch (error) {
        console.error('Failed to initialize plugins:', error);
      }
    } else if (!isAuthenticated && wasAuthenticated) {
      // Cleanup on logout
      eventDispatcherCleanup?.();
      eventDispatcherCleanup = null;
    }
  },
  { immediate: true }
);

onMounted(async () => {
  // Load branding configuration (public endpoint, no auth required)
  brandingStore.loadBranding();

  // Security: Prevent multiple initialization checks
  if (initializationChecked.value) {
    return;
  }

  try {
    // Only check if not already on onboarding or login pages
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
  <!-- Native app first run: choose a Nosdesk server before anything else.
       Always false on the web. -->
  <ConnectServerView v-if="needsServerSelection" />

  <!-- Blank layout for login -->
  <RouterView v-else-if="isBlankLayout" />

  <!-- Default layout with responsive navigation - Simple flexbox layout -->
  <div v-else v-twemoji class="flex w-full h-full bg-app overflow-hidden">
    <!-- Sidebar (includes both sidebar and mobile bottom nav, hidden on print) -->
    <Navbar class="print:hidden" @update:collapsed="handleNavCollapse" />

    <!-- Screen reader route announcements -->
    <div aria-live="polite" aria-atomic="true" class="sr-only">{{ routeAnnouncement }}</div>

    <!-- Main content area - takes remaining space -->
    <div class="flex flex-col flex-1 min-w-0">
      <!-- Header - sticky at top of content area (hidden on print) -->
      <PageHeader
        class="print:hidden flex-shrink-0 border-b border-default bg-surface"
        :useRouteTitle="!isDocumentationPage"
        :title="titleManager.pageTitle.value"
        :titleIcon="titleIcon"
        :showCreateButton="showCreateButton"
        :createButtonText="createButtonText"
        :createButtonIcon="createButtonIcon"
        :ticket="titleManager.currentTicket.value"
        :device="titleManager.currentDevice.value"
        :document="titleManager.currentDocument.value"
        :custom-title-editable="titleManager.isCustomTitleEditable.value"
        :device-title-editable="titleManager.isDeviceTitleEditable.value"
        :is-transitioning="titleManager.isTransitioning.value"
        :pageUrl="currentPageUrl"
        :navbarCollapsed="navbarCollapsed"
        @update-document-title="titleManager.updateDocumentTitle"
        @preview-document-title="titleManager.previewDocumentTitle"
        @update-document-icon="titleManager.updateDocumentIcon"
        @update-ticket-title="titleManager.updateTicketTitle"
        @preview-ticket-title="titleManager.previewTicketTitle"
        @update-device-title="titleManager.updateDeviceTitle"
        @preview-device-title="titleManager.previewDeviceTitle"
        @update-custom-title="titleManager.updateCustomTitle"
        @create="handleCreateClick"
      />

      <!-- Mobile Search Bar (positioned above bottom nav, hidden on print) -->
      <MobileSearchBar class="print:hidden" />

      <!-- Scrollable content with bottom padding for mobile nav (+ search bar when active) -->
      <main
        class="flex-1 overflow-hidden sm:pb-0"
        :class="isMobileSearchActive ? 'pb-[calc(6.5rem+env(safe-area-inset-bottom))]' : 'pb-[calc(3rem+env(safe-area-inset-bottom))]'"
      >
        <!-- Workspace switch in flight: mask the content so neither the old
             workspace's data (being torn down) nor the new one's empty state
             flashes while the sync pool re-hydrates. -->
        <div
          v-if="isSwitchingWorkspace"
          class="flex h-full w-full items-center justify-center text-secondary"
        >
          <LoadingSpinner size="md" />
        </div>
        <RouterView
          v-else
          v-slot="{ Component, route: viewRoute }"
          @update:ticket="titleManager.setTicket"
          @update:device="titleManager.setDevice"
          @update:document="titleManager.setDocument"
          @update:title="titleManager.setCustomTitle"
        >
          <Transition name="page" mode="out-in">
            <!--
              No `<KeepAlive>`. Every view's stateful concerns are
              owned by stores keyed by route param so the component
              itself is fully unmountable:
                * Tickets/Users/Devices/Projects/Docs URL-sync
                  filters; Pinia Colada serves cached data instantly
                  on remount.
                * TicketView's Yjs doc, WebsocketProvider, and
                  PermanentUserData live in `useCollabSessionStore`
                  refcounted by docId, with IndexedDB persistence
                  for instant cold-load.
                * Comment drafts live in `useTicketDraftsStore`
                  (localStorage) and pending attachments in
                  `useTicketUiStore` (in-memory).
              Avoiding KeepAlive sidesteps three documented core
              bugs (vuejs/core#5386, #5323, #12786) and removes the
              activate/deactivate gymnastics from every composable.

              `:key` reads from `route.meta.key`, which routes set in
              `beforeEnter` (e.g. `to.meta.key = to.params.id` for
              ticket / project detail). Forces a fresh mount when
              navigating directly between two records of the same
              route (`/tickets/1` → `/tickets/2`) so per-record
              stateful composables (collab session, ticket SSE) tear
              down and rebuild against the new id, instead of being
              left wired to the first id.

              The fallback uses the *top-level* matched route's path
              instead of the leaf `fullPath`. Routes that share a
              parent layout (e.g. every admin child sits under the
              `/admin` parent that mounts AdminLayout) collapse to a
              single key — the parent layout stays mounted across
              child navigations, only the nested RouterView inside
              the layout re-renders. Without this, navigating from
              `/admin/groups` to `/admin/categories` would unmount
              the entire AdminLayout (sidebar included) and run it
              through the `page` transition, which reads as the
              sidebar flashing in and out.
            -->
            <component
              :is="Component"
              :key="viewRoute.meta.key ?? viewRoute.matched[0]?.path ?? viewRoute.fullPath"
              class="h-full overflow-auto"
            />
          </Transition>
        </RouterView>
      </main>
    </div>
  </div>

  <!-- Global async-activity progress bar. Subscribes to the
       operations registry; renders a thin top-of-viewport bar
       whenever any tracked op or mutation is in flight. One
       indicator for the whole app. -->
  <RouteProgress class="print:hidden" />

  <!-- Global Toast Container (hidden on print) -->
  <ToastContainer class="print:hidden" />

  <!-- Global Search Modal -->
  <GlobalSearchModal />
</template>

<style>
/* Global styles - note: html/body height and overflow are set in main.css */

/*
 * Global scrollbar baseline. Thin, subtle, never the loudest
 * thing on screen. Sharp corners over fully-rounded — the rest
 * of Nosdesk's chrome leans utilitarian (square table cells,
 * sharp status pills, instrument-panel feel) and a pill-shape
 * scrollbar would read as out-of-place consumer-soft chrome.
 * A 2px transparent border with `background-clip: padding-box`
 * gives the thumb a "floating inside the gutter" look so the
 * track doesn't feel like a heavy frame.
 *
 * Per-surface overrides can still tighten further, but the
 * default no longer needs to be fought to look modern.
 */
::-webkit-scrollbar {
  width: 10px;
  height: 10px;
}

::-webkit-scrollbar-track {
  background: transparent;
}

::-webkit-scrollbar-thumb {
  background-color: var(--color-border-default);
  border: 2px solid transparent;
  background-clip: padding-box;
  transition: background-color 120ms ease;
}

/* Highlight the thumb when the scrollable host is hovered.
 * Pseudo-elements are terminal in the CSS spec, so the original
 * `:hover > ::-webkit-scrollbar-thumb` form was invalid (Lightning
 * CSS rejects it; esbuild + browsers used to silently no-op it).
 * The :hover::-webkit-scrollbar-thumb form expresses the same intent
 * correctly. Direct thumb hover is handled by the rule just below. */
:hover::-webkit-scrollbar-thumb {
  background-color: var(--color-text-tertiary);
}

::-webkit-scrollbar-thumb:hover {
  background-color: var(--color-text-secondary);
}

::-webkit-scrollbar-corner {
  background: transparent;
}

/* Firefox scrollbar styles. `thin` matches the WebKit width
 * above; `transparent` track keeps the gutter feeling like
 * negative space rather than a separate UI element. */
* {
  scrollbar-width: thin;
  scrollbar-color: var(--color-border-default) transparent;
}

@media (prefers-reduced-motion: reduce) {
  ::-webkit-scrollbar-thumb {
    transition: none;
  }
}

@media (max-width: 640px) {
  ::-webkit-scrollbar {
    width: 8px;
    height: 8px;
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

/* Page transition for route navigation */
.page-enter-active {
  transition: opacity 0.15s ease-out;
}

.page-leave-active {
  transition: opacity 0.1s ease-in;
}

.page-enter-from,
.page-leave-to {
  opacity: 0;
}

/* Respect reduced motion preferences */
@media (prefers-reduced-motion: reduce) {
  .page-enter-active,
  .page-leave-active {
    transition: none;
  }
}
</style>