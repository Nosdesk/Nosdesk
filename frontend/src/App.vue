// App.vue
<script setup lang="ts">
import { RouterView, useRoute, useRouter } from 'vue-router'
import { computed, ref, onMounted, watch, nextTick, defineAsyncComponent } from 'vue'
import { needsServerSelection } from '@/platform/serverGate'
import { isTauriRuntime } from '@/platform'
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
import PluginModalHost from '@/plugins/components/PluginModalHost.vue'
import { useTitleManager } from '@/composables/useTitleManager'
import { useMobileSearch } from '@/composables/useMobileSearch'
import { useCursorScanlines } from '@/composables/useCursorScanlines'
import { useCrtEffect } from '@/composables/useCrtEffect'
import { useSnowfall } from '@/composables/useSnowfall'
import { useFavicon } from '@/composables/useFavicon'
import { useNotificationSSE } from '@/composables/useNotificationSSE'
import { useTicketDeletionCleanup } from '@/composables/useTicketDeletionCleanup'
// Swipe-back TARGET list views to KeepAlive so native back / popstate restores
// them (scroll + state) instead of re-mounting. Scoped on purpose — details are
// never cached.
const keepAliveViews = ['TicketsListView', 'AssetsListView']

// In the native app, WKWebView owns the back animation (its snapshot slide). The
// Vue `page` out-in transition then fights it: it holds the LEAVING view on screen
// while WebKit already revealed the destination, so the old page flashes back in.
// Disable the Vue transition natively (instant swap) and let WebKit animate. Web
// keeps the transition.
const usePageTransition = !isTauriRuntime()
import { setMentionNavigationHandler } from '@/plugins/prosemirror-mention-view'
import authService from '@nosdesk/core/services/authService'
import { useBrandingStore } from '@/stores/branding'
import { loadPlugins, initializeEventDispatcher, startPluginLifecycleSync } from '@/plugins'
import { useAuthStore } from '@/stores/auth'
import { usePageActionsStore } from '@nosdesk/core/stores/pageActions'
import { useMyWorkspacesStore } from '@/stores/myWorkspaces'
import { activeWorkspaceSlug } from '@/services/activeWorkspace'
import { getWorkspaceRouting, fetchInstanceConfig } from '@nosdesk/core/services/instanceConfig'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

// Initialize branding store and load config
const brandingStore = useBrandingStore()

// Reactive favicon management - watches branding store for changes
useFavicon(() => brandingStore.faviconUrl)

const route = useRoute()
const { isSwitchingWorkspace, switchWorkspace } = useWorkspaceSwitch()
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

// Native app: deep-link when a push notification is tapped.
//
// The native side handles the tap (UNUserNotificationCenter delegate) and
// BUFFERS it — the single source of truth. We DRAIN that buffer (read-and-clear)
// via a plugin command (`invoke`, which works on iOS) from two lifecycle
// triggers, so no tap is lost and none double-fires:
//   • app mount          → a cold-start tap (app launched from the notification)
//   • foreground/visible → a warm tap (app was backgrounded, tap re-activates it)
// NB: we deliberately do NOT use the plugin's `notificationOpened` EVENT — the
// Tauri PluginManager event bus does not deliver plugin events to the webview
// on iOS, so an `await` on its listener would silently abort this whole block.
// Navigation is auth-aware: if the session isn't hydrated yet (cold start), we
// defer to the first authenticated state so the target survives the app's own
// startup/login navigation instead of racing it.
if (isTauriRuntime()) {
  onMounted(async () => {
    try {
      const { getPendingNotificationRoute } = await import('@nosdesk/mobile')
      await router.isReady()

      const navigate = (target: string) => {
        if (authStore.isAuthenticated) {
          void router.push(target)
        } else {
          // Cold start before auth hydration (or pre-login): apply once the
          // session resolves, after the app's own initial navigation.
          const stop = watch(
            () => authStore.isAuthenticated,
            (authed) => {
              if (authed) {
                stop()
                void nextTick(() => router.push(target))
              }
            }
          )
        }
      }

      // Read-and-clear the buffered tap; the native getter clears it, so whichever
      // trigger fires first wins and the rest are no-ops (no double navigation).
      const drain = async () => {
        const target = await getPendingNotificationRoute()
        if (target) navigate(target)
      }

      await drain()
      document.addEventListener('visibilitychange', () => {
        if (document.visibilityState === 'visible') void drain()
      })
    } catch {
      // Deep-linking is best-effort; never block app startup.
    }
  })
}

// Ticket deep links (iOS Universal Links / Android App Links). Same shape as
// the push route above: resolve where the tapped/scanned URL goes, waiting for
// the session on a cold start. v1 rule: a ticket link whose host is the server
// we're connected to opens the ticket in-app; a different tenant, or a server
// we're not connected to, opens in the system browser.
if (isTauriRuntime()) {
  onMounted(async () => {
    try {
      const { getInitialDeepLink, onDeepLink, openInBrowser, getStoredServer } =
        await import('@nosdesk/mobile')
      await router.isReady()

      const handleDeepLink = (rawUrl: string) => {
        let url: URL
        try {
          url = new URL(rawUrl)
        } catch {
          return
        }
        const isTicket = /\/tickets\/\d+/.test(url.pathname)
        let sameServer = false
        const server = getStoredServer()
        if (server) {
          try {
            sameServer = new URL(server).host === url.host
          } catch {
            /* malformed stored server: treat as not matching */
          }
        }
        if (!isTicket || !sameServer) {
          void openInBrowser(rawUrl)
          return
        }
        // Same server: open in-app. Navigate now if signed in, else once the
        // session resolves (cold start, or after signing into this server).
        const go = async () => {
          // In path mode the first path segment is the target workspace slug.
          // A link into a workspace other than the one currently loaded needs a
          // real switch (teardown + re-hydrate the sync pool for the target),
          // not a bare push, which would leave the pool keyed to the previous
          // tenant and 404 the ticket. Host mode carries no slug in the path;
          // same-origin already implies the same tenant, so a push is correct.
          await fetchInstanceConfig()
          if (getWorkspaceRouting() !== 'path') {
            void router.push(url.pathname)
            return
          }
          const targetSlug = url.pathname.split('/')[1] || null
          if (!targetSlug || targetSlug === activeWorkspaceSlug()) {
            void router.push(url.pathname)
            return
          }
          const store = useMyWorkspacesStore()
          if (store.workspaces.length === 0) {
            try {
              await store.refetch()
            } catch {
              /* fall through: the membership check below opens the web fallback */
            }
          }
          const entry = store.workspaces.find((w) => w.slug === targetSlug)
          if (!entry) {
            // Not a member of the target workspace: open on the web rather than
            // bouncing to no-workspace-access in-app.
            void openInBrowser(rawUrl)
            return
          }
          await switchWorkspace(entry, url.pathname)
        }
        if (authStore.isAuthenticated) {
          void go()
        } else {
          const stop = watch(
            () => authStore.isAuthenticated,
            (authed) => {
              if (authed) {
                stop()
                void nextTick(go)
              }
            },
          )
        }
      }

      const initial = await getInitialDeepLink()
      if (initial) handleDeepLink(initial)
      await onDeepLink(handleDeepLink)
    } catch {
      // Deep-linking is best-effort; never block app startup.
    }
  })
}

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
let pluginLifecycleCleanup: (() => void) | null = null;

// Watch auth state and load plugins when authenticated
// immediate: true handles initial state, watch handles subsequent changes
watch(
  () => authStore.isAuthenticated,
  async (isAuthenticated, wasAuthenticated) => {
    if (isAuthenticated && !wasAuthenticated) {
      try {
        await loadPlugins();
        eventDispatcherCleanup = initializeEventDispatcher();
        // Tear down / load plugins in this session as their state changes
        // server-side (disable / quarantine / uninstall / re-enable), not just
        // in the admin tab that flipped the state.
        pluginLifecycleCleanup = startPluginLifecycleSync();
      } catch (error) {
        console.error('Failed to initialize plugins:', error);
      }
    } else if (!isAuthenticated && wasAuthenticated) {
      // Cleanup on logout
      eventDispatcherCleanup?.();
      eventDispatcherCleanup = null;
      pluginLifecycleCleanup?.();
      pluginLifecycleCleanup = null;
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
      <!-- Header - sticky at top of content area (hidden on print). The
           safe-area-inset-top padding lets the header's own background fill the
           notch / Dynamic Island area on iOS (the standard top-bar pattern);
           a no-op on the web, where the inset is 0. -->
      <PageHeader
        class="print:hidden flex-shrink-0 border-b border-default bg-surface pt-[env(safe-area-inset-top)]"
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
        class="flex min-h-0 flex-1 flex-col overflow-hidden sm:pb-0"
        :class="isMobileSearchActive ? 'pb-[calc(6.5rem+env(safe-area-inset-bottom))]' : 'pb-[calc(3rem+env(safe-area-inset-bottom))]'"
      >
        <!-- Positioning context for the swipe layers. A flex child fills main's
             content box, so the absolute layers below respect main's mobile-nav
             bottom padding instead of sliding under the bottom nav. -->
        <div class="relative min-h-0 flex-1">
        <!-- Workspace switch in flight: mask the content so neither the old
             workspace's data (being torn down) nor the new one's empty state
             flashes while the sync pool re-hydrates. -->
        <div
          v-if="isSwitchingWorkspace"
          class="flex h-full w-full items-center justify-center text-secondary"
        >
          <LoadingSpinner size="md" />
        </div>
        <!-- Plain single-view RouterView. The iOS swipe-back is handled natively
             by WKWebView (allowsBackForwardNavigationGestures), so no JS gesture
             or view stack is needed; desktop is unaffected. -->
        <RouterView
          v-else
          v-slot="{ Component, route: viewRoute }"
          @update:ticket="titleManager.setTicket"
          @update:device="titleManager.setDevice"
          @update:document="titleManager.setDocument"
          @update:title="titleManager.setCustomTitle"
        >
          <!-- Scoped KeepAlive: cache only the swipe-back TARGET list views, so
               returning (native WKWebView back / popstate) restores the live
               instance (scroll + state) instead of re-mounting. Detail views
               (SSE/collab-heavy) are deliberately NOT cached and unmount normally.

               Web wraps it in the `page` out-in transition. The NATIVE app does
               NOT: WKWebView owns the back animation, and an out-in transition
               holds the leaving view for a tick past `popstate` — which is exactly
               the frame WebKit lifts its snapshot on, causing the outgoing-page
               flash (WebKit bug 187506). No transition => immediate swap. -->
          <Transition v-if="usePageTransition" name="page" mode="out-in">
            <KeepAlive :include="keepAliveViews">
              <component
                :is="Component"
                :key="viewRoute.meta.key ?? viewRoute.matched[0]?.path ?? viewRoute.fullPath"
                v-scroll-restore="viewRoute.fullPath"
                class="h-full overflow-auto"
              />
            </KeepAlive>
          </Transition>
          <KeepAlive v-else :include="keepAliveViews">
            <component
              :is="Component"
              :key="viewRoute.meta.key ?? viewRoute.matched[0]?.path ?? viewRoute.fullPath"
              v-scroll-restore="viewRoute.fullPath"
              class="h-full overflow-auto"
            />
          </KeepAlive>
        </RouterView>
        </div>
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

  <!-- On-demand plugin modal surface (opened by plugin action contributions) -->
  <PluginModalHost />
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