// App.vue
<script setup lang="ts">
import { RouterView, useRoute, useRouter } from 'vue-router'
import { computed, ref, onMounted, watch, nextTick, defineAsyncComponent } from 'vue'
import { needsServerSelection } from '@/platform/serverGate'
import { isTauriRuntime } from '@/platform'
// Native first-run server picker; lazy so the web bundle doesn't carry it.
const ConnectServerView = defineAsyncComponent(() => import('@/views/ConnectServerView.vue'))
import { ConfigProvider, TooltipProvider } from 'reka-ui'
import { useDateStore } from '@nosdesk/core/stores/dateStore'
import ToastContainer from './components/common/ToastContainer.vue'
import RouteProgress from './components/common/RouteProgress.vue'
import { useWorkspaceSwitch } from '@/composables/useWorkspaceSwitch'
import { useCursorScanlines } from '@/composables/useCursorScanlines'
import { useCrtEffect } from '@/composables/useCrtEffect'
import { useSnowfall } from '@/composables/useSnowfall'
import { useFavicon } from '@/composables/useFavicon'
import { loadAppShell } from './loadAppShell'
// The signed-in frame and the overlays only a signed-in person opens load on
// demand, so the sign-in page's first paint doesn't wait for them.
const AppShell = defineAsyncComponent(loadAppShell)
const GlobalSearchModal = defineAsyncComponent(() => import('./components/GlobalSearch/GlobalSearchModal.vue'))
const PdfOverlay = defineAsyncComponent(() => import('@/components/ticketComponents/PdfOverlay.vue'))
const PluginModalHost = defineAsyncComponent(() => import('@/plugins/components/PluginModalHost.vue'))
const PluginDialogHost = defineAsyncComponent(() => import('@/plugins/components/PluginDialogHost.vue'))
import authService from '@nosdesk/core/services/authService'
import { useBrandingStore } from '@/stores/branding'
import { loadPlugins, initializeEventDispatcher, startPluginLifecycleSync } from '@/plugins'
import { useAuthStore } from '@/stores/auth'
import { useMyWorkspacesStore } from '@/stores/myWorkspaces'
import { activeWorkspaceSlug, activeWorkspaceSlugRef, workspaceReadyRef } from '@/services/activeWorkspace'
import { getWorkspaceRouting, fetchInstanceConfig } from '@nosdesk/core/services/instanceConfig'

// Reka UI primitives read locale (number and date fields), reading
// direction and the portal target from one provider. Locale follows the
// user's setting so Reka formats the way the rest of the app does.
const dateStore = useDateStore()
const rekaLocale = computed(() => dateStore.locale)

// Initialize branding store and load config
const brandingStore = useBrandingStore()

// Reactive favicon management - watches branding store for changes
useFavicon(() => brandingStore.faviconUrl)

const route = useRoute()
const { switchWorkspace } = useWorkspaceSwitch()
const router = useRouter()
const isBlankLayout = computed(() => route.meta.layout === 'blank')

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

// Register this device for push whenever a session goes live.
//
// This used to hang off the OIDC login button alone, so signing in with a
// password, MFA, passkey or recovery code registered nothing and the user
// silently received no push at all. Watching the auth flag instead covers every
// login path, plus cold start with a restored session — which also picks up
// APNs/FCM token rotation, since a rotated token otherwise leaves the device
// permanently silent with no way back.
//
// It also waits for the workspace: a device belongs to one workspace, and in
// path mode the workspace travels as a header that isn't set until just after
// sign-in, so registering on auth alone was refused and never retried. Watching
// the slug re-registers on a switch, so push follows the active workspace.
//
// Idempotent server-side (the endpoint upserts), so re-calling costs one
// request. Best-effort by design: a push-registration failure must never block
// getting into the app.
if (isTauriRuntime()) {
  onMounted(() => {
    watch(
      () => [authStore.isAuthenticated, workspaceReadyRef.value, activeWorkspaceSlugRef.value] as const,
      async ([authed, ready]) => {
        if (!authed || !ready) return
        try {
          const { registerForPush } = await import('@nosdesk/mobile')
          await registerForPush()
        } catch (error) {
          console.warn('[push] device registration failed', error)
        }
      },
      { immediate: true }
    )
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
        const isTicket = /\/tickets\/(?:id\/)?\d+/.test(url.pathname)
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

// Theme-specific visual effects
useCursorScanlines();  // Red-horizon: Crosshair lines following cursor
useCrtEffect();        // Red-horizon: Full-screen CRT monitor effect
useSnowfall();         // Christmas: Ambient falling snow

// Security: Check if system requires initial setup on app initialization
const initializationChecked = ref(false);

// Initialize plugins after authentication
const authStore = useAuthStore();
let eventDispatcherCleanup: (() => void) | null = null;
let pluginLifecycleCleanup: (() => void) | null = null;

// Watch auth state and load plugins when authenticated
// immediate: true handles initial state, watch handles subsequent changes
watch(
  () => authStore.isAuthenticated,
  async (isAuthenticated, wasAuthenticated) => {
    // The "Confirm it's you" popup page closes itself; it runs no plugins.
    if (route.name === 'auth-confirmed') return;
    if (isAuthenticated && !wasAuthenticated) {
      try {
        // Path mode: the workspace guard loads plugins when it publishes a
        // slug (and reloads them on a switch), since the enabled set is the
        // workspace's. Host mode has one workspace per origin, so load here.
        await fetchInstanceConfig();
        if (getWorkspaceRouting() !== 'path') await loadPlugins();
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
      const setupStatus = await authService.checkSetupStatus();
      if (setupStatus.requires_setup) {
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
  <!-- Portals land in `#overlays` (index.html), outside `#app`'s stacking
       contexts. Tooltips share one delay so moving between adjacent icon
       buttons opens the next hint at once. -->
  <ConfigProvider :locale="rekaLocale" teleport-to="#overlays">
  <TooltipProvider :delay-duration="200" :skip-delay-duration="300">
  <!-- Native app first run: choose a Nosdesk server before anything else.
       Always false on the web. -->
  <ConnectServerView v-if="needsServerSelection" />

  <!-- Signing out: the session is gone, so the signed-in views give way at
       once rather than render again for no one before the login page. -->
  <div v-else-if="authStore.signingOut" class="h-full w-full bg-app" aria-busy="true" />

  <!-- Blank layout for login -->
  <RouterView v-else-if="isBlankLayout" />

  <!-- The signed-in app -->
  <AppShell v-else />

  <!-- Global async-activity progress bar. Subscribes to the
       operations registry; renders a thin top-of-viewport bar
       whenever any tracked op or mutation is in flight. One
       indicator for the whole app. -->
  <RouteProgress class="print:hidden" />

  <!-- Global Toast Container (hidden on print) -->
  <ToastContainer />

  <!-- Overlays only a signed-in person opens -->
  <template v-if="authStore.isAuthenticated">
    <!-- Global Search Modal -->
    <GlobalSearchModal />

    <!-- PDF viewer, over whatever page opened it (`?pdf=` on the route) -->
    <PdfOverlay />

    <!-- On-demand plugin modal surface (opened by plugin action contributions) -->
    <PluginModalHost />

    <!-- Dialogs plugins ask the host to draw (confirm, pickers); stack over the plugin modal -->
    <PluginDialogHost />
  </template>
  </TooltipProvider>
  </ConfigProvider>
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