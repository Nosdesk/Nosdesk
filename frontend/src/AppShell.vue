<!-- AppShell.vue
     The signed-in app's frame: sidebar, header and the routed view, with the
     listeners that only matter inside it. It loads with the first route that
     uses it (see loadAppShell.ts), so a signed-out visitor's first screen doesn't
     wait for the sidebar, the sync runtime and the editor they pull in. -->
<script setup lang="ts">
import { RouterView, useRoute, useRouter } from 'vue-router'
import { warmAppViews } from '@/router'
import { whenIdle } from '@/utils/whenIdle'
import { computed, ref, nextTick, onBeforeUnmount, onMounted } from 'vue'
import { useFluent } from 'fluent-vue'
import { isTauriRuntime } from '@/platform'
import Navbar from './components/Navbar.vue'
import PageHeader from './components/SiteHeader.vue'
import MobileSearchBar from './components/MobileSearchBar.vue'
import TrialReminderBanner from './components/TrialReminderBanner.vue'
import LoadingSpinner from './components/common/LoadingSpinner.vue'
import { useWorkspaceSwitch } from '@/composables/useWorkspaceSwitch'
import { useTitleManager } from '@/composables/useTitleManager'
import { useMobileSearch } from '@/composables/useMobileSearch'
import { useNotificationSSE } from '@/composables/useNotificationSSE'
import { useTicketDeletionCleanup } from '@/composables/useTicketDeletionCleanup'
import { useDocDeletionCleanup } from '@/composables/useDocDeletionCleanup'
import { usePageActionsStore } from '@nosdesk/core/stores/pageActions'
import { setMentionNavigationHandler } from '@/plugins/prosemirror-mention-view'

// Swipe-back TARGET list views to KeepAlive so native back / popstate restores
// them (scroll + state) instead of re-mounting. Scoped on purpose: details are
// never cached.
const keepAliveViews = ['TicketsListView', 'AssetsListView']

// In the native app, WKWebView owns the back animation (its snapshot slide). The
// Vue `page` out-in transition then fights it: it holds the LEAVING view on screen
// while WebKit already revealed the destination, so the old page flashes back in.
// Disable the Vue transition natively (instant swap) and let WebKit animate. Web
// keeps the transition.
const usePageTransition = !isTauriRuntime()

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

const route = useRoute()
const router = useRouter()
const { isSwitchingWorkspace } = useWorkspaceSwitch()

// Accessibility: announce route changes to screen readers
const routeAnnouncement = ref('')
const stopAnnouncing = router.afterEach((to) => {
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
onBeforeUnmount(stopAnnouncing)

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
const titleManager = useTitleManager()

// Mobile search bar state - used for conditional padding
const { isActive: isMobileSearchActive } = useMobileSearch()

// Real-time notification handling
useNotificationSSE()
// App-level SSE listener that wipes local artefacts (collab IDB
// cache, comment draft, pending attachments) for any ticket the
// server announces as deleted, see useTicketDeletionCleanup.
useTicketDeletionCleanup()
// Likewise for documentation pages and collections the reader loses,
// see useDocDeletionCleanup.
useDocDeletionCleanup()

// Someone is signed in: load the views opened from inside others (a ticket,
// a docs page) once the page is idle, so opening one doesn't wait for it.
onMounted(() => whenIdle(warmAppViews))

// Computed property to determine if on a documentation page
const isDocumentationPage = computed(() => route.name === 'documentation-article')

// Computed property for the current page URL (for display purposes)
const currentPageUrl = computed(() => {
  // Only show URL for certain pages
  if (route.name === 'settings' || route.name === 'profile') {
    return window.location.href
  }
  return undefined
})

// Computed properties for create button from route meta.
// Prefer `createButtonTextKey` (FTL key) so the label translates with
// the user's locale; fall back to the legacy `createButtonText` string
// for routes that haven't been migrated yet.
const createButtonText = computed(() => {
  const key = route.meta.createButtonTextKey as string | undefined
  if (key) return t(key)
  return (route.meta.createButtonText as string | undefined) || t('header-create-ticket')
})

const createButtonIcon = computed(() => route.meta.createButtonIcon ?? 'plus')

// Title icon from route meta (e.g., 'pdf' for PDF viewer)
const titleIcon = computed(() => route.meta.titleIcon as string | undefined)

// Visibility stays driven by route meta so first paint shows the
// right label before the view's onMounted has fired. The handler
// itself is registered by the view via `usePageCreateAction(...)`,
// which writes to `pageActions.createAction`.
const showCreateButton = computed(() => !!(route.meta.createButtonTextKey || route.meta.createButtonText))

const pageActions = usePageActionsStore()
const handleCreateClick = () => {
  void pageActions.invokeCreate()
}
</script>

<template>
  <div v-twemoji class="flex w-full h-full bg-app overflow-hidden">
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
        :creating="pageActions.creating"
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

      <!-- Scrollable content. Below `sm` the padding clears the app's own bottom
           nav (plus the search bar when active) and the safe area under it. From
           `sm` up there is no bottom nav, but the safe area is still owed: on a
           tablet the window runs under Android's navigation bar, and `sm:pb-0`
           used to hand content straight to it. Responsive utilities are emitted
           after base ones, so the `sm:` rule wins without a duplicate base
           utility fighting it. -->
      <main
        class="flex min-h-0 flex-1 flex-col overflow-hidden sm:pb-[env(safe-area-inset-bottom)]"
        :class="isMobileSearchActive ? 'pb-[calc(6.5rem+env(safe-area-inset-bottom))]' : 'pb-[calc(3rem+env(safe-area-inset-bottom))]'"
      >
        <!-- Cloud trial reminder for the workspace owner. Renders nothing
             for anyone else, so the view below never shifts. -->
        <TrialReminderBanner />
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
        >
          <!-- Scoped KeepAlive: cache only the swipe-back TARGET list views, so
               returning (native WKWebView back / popstate) restores the live
               instance (scroll + state) instead of re-mounting. Detail views
               (SSE/collab-heavy) are deliberately NOT cached and unmount normally.

               Web wraps it in the `page` out-in transition. The NATIVE app does
               NOT: WKWebView owns the back animation, and an out-in transition
               holds the leaving view for a tick past `popstate`, which is exactly
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
</template>
