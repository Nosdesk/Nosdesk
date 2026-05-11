<script setup lang="ts">
import { RouterLink, useRoute } from "vue-router";
import DocumentationNav from "@/components/documentationComponents/DocumentationNav.vue";
import RecentTickets from "@/components/RecentTickets.vue";
import CollapsibleSection from "@/components/common/CollapsibleSection.vue";
import LogoIcon from "@/components/icons/LogoIcon.vue";
import FaviconIcon from "@/components/icons/FaviconIcon.vue";
import { ref, computed, onMounted, onBeforeUnmount, nextTick, watch } from "vue";
import { useResizableSidebar } from "@/composables/useResizableSidebar";
import { useNavbarState } from "@/composables/useNavbarState";
import { useGlobalSearch } from "@/composables/useGlobalSearch";
import { useBrandingStore } from "@/stores/branding";
import { useThemeStore } from "@/stores/theme";
import Icon from "@/components/common/Icon.vue";

// Global search
const { openSearch } = useGlobalSearch();

// Keyboard shortcut hint based on platform
const isMac = /Mac|iPhone|iPad|iPod/.test(navigator.userAgent);
const searchShortcut = isMac ? '\u2318K' : 'Ctrl+K';

// Get branding and theme stores
const brandingStore = useBrandingStore();
const themeStore = useThemeStore();

// Computed logo URL based on current theme
const logoUrl = computed(() => {
    const customLogo = brandingStore.getLogoUrl(themeStore.isDarkMode);
    return customLogo || null;
});

const route = useRoute();

// Use centralized navbar state composable
const {
    isCollapsed,
    isMobile,
    isCompactNav,
    isDocsCollapsed,
    isTicketsCollapsed,
    toggleCollapsed,
    toggleDocs,
    toggleTickets,
    initialize: initNavbarState,
    cleanup: cleanupNavbarState
} = useNavbarState();

// Refs for DOM elements - These will be passed to the composable
const navbarRef = ref<HTMLElement | null>(null);
const resizerRef = ref<HTMLElement | null>(null);

// Component refs for CollapsibleSection instances
const ticketsSectionComponent = ref<InstanceType<typeof CollapsibleSection> | null>(null);
const docsSectionComponent = ref<InstanceType<typeof CollapsibleSection> | null>(null);

// Computed refs that extract DOM elements from component instances
const ticketsSectionRef = computed(() => ticketsSectionComponent.value?.$el || null);
const docsSectionRef = computed(() => docsSectionComponent.value?.$el || null);

// Define locally for check in onMounted, or expose from composable if preferred
const MIN_SECTION_HEIGHT = 60;

// Use the composable for resizing logic
const {
    ticketsHeight, // The reactive height value from the composable
    isResizing, // The reactive resizing status from the composable
    startResize, // The function to start resizing, attach to resizer handle
    equalizeHeights, // Utility function to equalize heights
} = useResizableSidebar(
    navbarRef,
    ticketsSectionRef,
    docsSectionRef,
    resizerRef,
);

// Emit collapsed state changes to parent (App.vue)
const emit = defineEmits(["update:collapsed"]);

// Watch for collapsed state changes and emit to parent
watch(isCollapsed, (value) => {
    emit("update:collapsed", value);
}, { immediate: true });

// Initialize on mount
onMounted(() => {
    // Initialize navbar state (handles localStorage and resize listener)
    initNavbarState();

    // Set initial sizes after mount
    nextTick(() => {
        if (!ticketsHeight.value || ticketsHeight.value < MIN_SECTION_HEIGHT) {
            if (
                !isCollapsed.value &&
                !isTicketsCollapsed.value &&
                !isDocsCollapsed.value
            ) {
                equalizeHeights();
            }
        }
    });
});

// Clean up on unmount
onBeforeUnmount(() => {
    cleanupNavbarState();
});

// Navigation grouped into two sections — Work (active task
// surfaces) and Resources (reference / inventory). The grouping
// only renders when the sidebar is expanded; in collapsed
// (icon-only) and compact-nav (grid) modes the items render as
// a flat list because section headers don't fit / would just
// fragment a visual icon row. The flat `navLinks` computed
// below preserves that path.
interface NavLink {
    to: string;
    icon: string;
    text: string;
    /** True for paths whose `route.path === path` should be the
     *  only "active" trigger. Without this, e.g. `/` would
     *  match every route (since every path starts with `/`). */
    exact?: boolean;
}
interface NavGroup {
    label: string;
    links: NavLink[];
}
const navGroups: NavGroup[] = [
    {
        label: "Work",
        links: [
            {
                to: "/",
                icon: "M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6",
                text: "Dashboard",
                exact: true,
            },
            {
                to: "/tickets",
                icon: "M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-6 9l2 2 4-4",
                text: "Tickets",
            },
            {
                to: "/cycles",
                // Calendar / iteration glyph: small block with a marker line.
                icon: "M8 7V3m8 4V3m-9 8h10M5 21h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z",
                text: "Cycles",
            },
            {
                to: "/projects",
                icon: "M4 4h4v16H4V4zm6 0h4v12h-4V4zm6 0h4v8h-4V4z",
                text: "Projects",
            },
        ],
    },
    {
        label: "Resources",
        links: [
            {
                to: "/devices",
                icon: "M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z",
                text: "Devices",
            },
            {
                to: "/assets",
                // Stacked-layers / planning glyph distinct from the
                // single-monitor Devices icon above.
                icon: "M3 7l9-4 9 4-9 4-9-4zm0 5l9 4 9-4M3 17l9 4 9-4",
                text: "Assets",
            },
            {
                to: "/users",
                icon: "M12 4.354a4 4 0 110 5.292M15 21H3v-1a6 6 0 0112 0v1zm0 0h6v-1a6 6 0 00-9-5.197M13 7a4 4 0 11-8 0 4 4 0 018 0z",
                text: "Users",
            },
            {
                to: "/documentation",
                icon: "M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253",
                text: "Documentation",
            },
        ],
    },
];

/** Flat list used by the collapsed / compact-nav layouts where
 *  section headers wouldn't fit. Kept derived so adding a new
 *  link only ever requires editing the grouped source. */
const navLinks: NavLink[] = navGroups.flatMap((g) => g.links);

/** Synthetic NavLink for the inbox. Kept out of `navGroups` (which
 *  drives the desktop sidebar) because Inbox lives in the header
 *  cluster alongside the bell on desktop; the sidebar shouldn't
 *  carry a duplicate entry. The bottom-nav slot is the only place
 *  it surfaces from this file. */
const INBOX_MOBILE_LINK: NavLink = {
    to: '/inbox',
    icon: 'M20 13V6a2 2 0 00-2-2H6a2 2 0 00-2 2v7m16 0v5a2 2 0 01-2 2H6a2 2 0 01-2-2v-5m16 0h-2.586a1 1 0 00-.707.293l-2.414 2.414a1 1 0 01-.707.293h-3.172a1 1 0 01-.707-.293l-2.414-2.414A1 1 0 006.586 13H4',
    text: 'Inbox',
};

/** Routes that get a permanent slot in the mobile bottom bar.
 *  Everything else lives behind the "More" overflow sheet so each
 *  primary tap target stays comfortably above the 44px floor on a
 *  360px-wide phone. Order here is the rendered order. Inbox earns
 *  a primary slot per the same logic that drives the bell-routes-
 *  to-inbox behaviour on mobile: notifications are thumb-reach
 *  high-traffic in productivity workflows. Search moves into the
 *  overflow sheet (still one tap away, just no longer competing
 *  with Inbox for the same bar real estate). */
const PRIMARY_MOBILE_PATHS = ['/', '/tickets', '/inbox'] as const;

const primaryMobileLinks = computed<NavLink[]>(() =>
    PRIMARY_MOBILE_PATHS.map((p) =>
        navLinks.find((l) => l.to === p) ??
        (p === '/inbox' ? INBOX_MOBILE_LINK : undefined),
    ).filter((l): l is NavLink => !!l),
);

const overflowMobileLinks = computed<NavLink[]>(() =>
    navLinks.filter((l) => !PRIMARY_MOBILE_PATHS.includes(l.to as typeof PRIMARY_MOBILE_PATHS[number])),
);

// Overflow sheet driven by the native <dialog> element via
// .showModal() / .close(). The native API gives us focus trap, Esc-
// to-dismiss, focus restoration on close, and proper `inert` on
// background content (Safari's `aria-modal` alone is unreliable);
// we just sync the imperative state with a reactive ref.
const isMobileMoreOpen = ref(false);
const mobileMoreDialogRef = ref<HTMLDialogElement | null>(null);

function toggleMobileMore() {
    isMobileMoreOpen.value = !isMobileMoreOpen.value;
}
function closeMobileMore() {
    isMobileMoreOpen.value = false;
}

watch(isMobileMoreOpen, async (open) => {
    await nextTick();
    const dialog = mobileMoreDialogRef.value;
    if (!dialog) return;
    if (open && !dialog.open) {
        dialog.showModal();
    } else if (!open && dialog.open) {
        dialog.close();
    }
});

// Close the overflow sheet when the route changes (so following any
// link inside the sheet visually completes the navigation rather
// than leaving the sheet floating over the destination).
watch(() => route.path, () => {
    isMobileMoreOpen.value = false;
});

// Backdrop tap on a native <dialog>: any click whose target is the
// dialog element itself (not its children) is on the backdrop.
function onDialogBackdropClick(event: MouseEvent) {
    if (event.target === mobileMoreDialogRef.value) {
        closeMobileMore();
    }
}

// Helper function to check if a route is active
const isRouteActive = (path: string, exact = false) => {
    if (exact) {
        return route.path === path;
    }
    return route.path.startsWith(path);
};

/** Active when at least one overflow link matches the current route,
 *  so the More button gets the same accent treatment as a direct
 *  primary link would. Without this the bar appears "nothing
 *  selected" while the user sits on a Devices / Users / Docs page. */
const isOverflowRouteActive = computed(() =>
    overflowMobileLinks.value.some((l) => isRouteActive(l.to, l.exact)),
);
</script>

<template>
    <!-- Sidebar - Flex item in document flow, hidden on mobile -->
    <nav
        ref="navbarRef"
        class="h-screen bg-surface border-r border-default flex flex-col flex-shrink-0 print:hidden transition-all duration-300 ease-in-out overflow-hidden"
        :class="[isCollapsed ? 'w-16' : 'w-64', isMobile ? 'hidden' : '']"
    >
        <!-- Logo - swaps between full logo and icon based on collapsed state -->
        <div class="flex flex-col p-2 px-2 flex-shrink-0 gap-1">
            <RouterLink
                to="/"
                class="sidebar-logo flex items-center justify-center h-12 mb-5 hover:opacity-80 transition-opacity select-none"
            >
                <!-- Full logo when expanded -->
                <img
                    v-if="!isCollapsed && logoUrl"
                    :alt="brandingStore.appName + ' Logo'"
                    class="h-8 max-w-full object-contain"
                    :src="logoUrl"
                />
                <LogoIcon
                    v-else-if="!isCollapsed"
                    class="h-8 text-accent"
                    aria-label="Nosdesk Logo"
                />
                <!-- Favicon/icon when collapsed -->
                <img
                    v-else-if="brandingStore.faviconUrl"
                    :alt="brandingStore.appName"
                    class="h-6 w-6 object-contain"
                    :src="brandingStore.faviconUrl"
                />
                <FaviconIcon
                    v-else
                    class="text-accent"
                    aria-label="Nosdesk"
                />
            </RouterLink>

            <!-- Search Button - above nav links -->
            <button
                @click="() => openSearch()"
                class="w-full mb-1.5 rounded-md transition-colors duration-200 flex items-center bg-surface-alt border border-default text-secondary hover:bg-surface-hover hover:text-primary hover:border-subtle"
                :class="[
                    isCollapsed
                        ? 'px-2 py-1.5 justify-center'
                        : 'px-2.5 py-1 gap-2 justify-between',
                ]"
                :title="isCollapsed ? 'Search' : ''"
            >
                <div class="flex items-center gap-2">
                    <Icon name="search" />
                    <span v-if="!isCollapsed" class="text-sm">Search</span>
                </div>
                <kbd
                    v-if="!isCollapsed"
                    class="hidden sm:inline-flex items-center px-1 py-0 text-xs font-mono bg-surface rounded border border-default text-tertiary"
                >
                    {{ searchShortcut }}
                </kbd>
            </button>

            <!--
              Three rendering modes for the nav links:

              1. Collapsed sidebar (w-16, icon-only): flat
                 vertical list of icons. Headers wouldn't fit
                 horizontally and would just steal vertical
                 space from the link icons themselves.
              2. Compact nav (a 6-column icon grid that fires on
                 short viewports — see useNavbarState): also
                 flat. The grid is the visual rhythm; section
                 headers would fragment it into two rows of 3
                 columns each, which reads as broken.
              3. Expanded sidebar (default): grouped sections
                 with small uppercase headers (`Work` /
                 `Resources`). One step of visual hierarchy,
                 matching Linear / Notion / GitHub sidebar
                 conventions.
            -->
            <div v-if="isCompactNav && !isCollapsed" class="grid grid-cols-6 gap-0.5">
                <RouterLink
                    v-for="link in navLinks"
                    :key="link.to"
                    :to="link.to"
                    class="rounded-md transition-colors duration-200 flex items-center relative overflow-hidden px-2 py-1.5 justify-center"
                    :class="
                        isRouteActive(link.to, link.exact)
                            ? 'bg-surface-alt/80 text-primary font-medium'
                            : 'text-secondary hover:bg-surface-hover hover:text-primary'
                    "
                    :title="link.text"
                >
                    <div
                        v-if="isRouteActive(link.to, link.exact)"
                        class="absolute left-0 top-0 bottom-0 w-1 bg-accent w-full h-0.5 top-auto"
                    ></div>
                    <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor">
                        <path
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            stroke-width="2"
                            :d="link.icon"
                        />
                    </svg>
                </RouterLink>
            </div>

            <div v-else-if="isCollapsed" class="flex flex-col gap-0.5">
                <RouterLink
                    v-for="link in navLinks"
                    :key="link.to"
                    :to="link.to"
                    class="rounded-md transition-colors duration-200 flex items-center relative overflow-hidden px-2 py-1.5 justify-center"
                    :class="
                        isRouteActive(link.to, link.exact)
                            ? 'bg-surface-alt/80 text-primary font-medium'
                            : 'text-secondary hover:bg-surface-hover hover:text-primary'
                    "
                    :title="link.text"
                >
                    <div
                        v-if="isRouteActive(link.to, link.exact)"
                        class="absolute left-0 top-0 bottom-0 w-1 bg-accent"
                    ></div>
                    <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor">
                        <path
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            stroke-width="2"
                            :d="link.icon"
                        />
                    </svg>
                </RouterLink>
            </div>

            <div v-else class="flex flex-col gap-2.5">
                <div
                    v-for="group in navGroups"
                    :key="group.label"
                    class="flex flex-col gap-0.5"
                >
                    <h3
                        class="px-2.5 text-[10px] font-semibold text-tertiary tracking-wide uppercase select-none"
                    >
                        {{ group.label }}
                    </h3>
                    <RouterLink
                        v-for="link in group.links"
                        :key="link.to"
                        :to="link.to"
                        class="rounded-md transition-colors duration-200 flex items-center relative overflow-hidden px-2.5 py-1 gap-2.5"
                        :class="
                            isRouteActive(link.to, link.exact)
                                ? 'bg-surface-alt/80 text-primary font-medium'
                                : 'text-secondary hover:bg-surface-hover hover:text-primary'
                        "
                    >
                        <div
                            v-if="isRouteActive(link.to, link.exact)"
                            class="absolute left-0 top-0 bottom-0 w-1 bg-accent"
                        ></div>
                        <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor">
                            <path
                                stroke-linecap="round"
                                stroke-linejoin="round"
                                stroke-width="2"
                                :d="link.icon"
                            />
                        </svg>
                        <span class="text-sm whitespace-nowrap">{{ link.text }}</span>
                    </RouterLink>
                </div>
            </div>
        </div>

        <!-- Separator -->
        <div class="border-t border-default/50 my-1"></div>

        <!-- Spacer: Always present to push toggle button to bottom -->
        <div class="flex-1 min-h-0 flex flex-col overflow-hidden">
            <!-- Only show sections when navbar is expanded -->
            <div
                class="flex-1 min-h-0 flex flex-col overflow-hidden"
                v-if="!isCollapsed"
            >
                <!-- Recent Tickets section with collapsible header -->
                <CollapsibleSection
                    ref="ticketsSectionComponent"
                    title="Recent Tickets"
                    :is-collapsed="isTicketsCollapsed"
                    icon="clock"
                    class="tickets-section flex-shrink-0 transition-all duration-200"
                    :style="{
                        maxHeight: isTicketsCollapsed
                            ? '32px'
                            : `${ticketsHeight}px`,
                    }"
                    @toggle="toggleTickets"
                >
                    <RecentTickets />
                </CollapsibleSection>

                <!-- Resizer between sections -->
                <div
                    ref="resizerRef"
                    class="resizer-handle group relative mx-1 flex items-center justify-center select-none"
                    @pointerdown="startResize"
                    :class="{ active: isResizing }"
                >
                    <!-- Equalize button removed -->
                    <!-- Drag indicator lines removed -->
                </div>

                <!-- Documentation section with collapsible header -->
                <CollapsibleSection
                    ref="docsSectionComponent"
                    title="Documentation"
                    :is-collapsed="isDocsCollapsed"
                    icon="book"
                    class="docs-section flex-1 min-h-0 transition-all duration-200 -mt-px"
                    @toggle="toggleDocs"
                >
                    <DocumentationNav />
                </CollapsibleSection>
            </div>
        </div>

        <!-- Toggle button at the bottom of sidebar (hidden on mobile) -->
        <div class="flex-shrink-0 border-t border-default" v-if="!isMobile">
            <button
                @click="toggleCollapsed"
                class="w-full h-8 px-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-md transition-colors group flex items-center justify-center"
                aria-label="Toggle sidebar"
            >
                <svg
                    xmlns="http://www.w3.org/2000/svg"
                    class="h-3.5 w-3.5 group-hover:text-accent transition-colors"
                    fill="none"
                    viewBox="0 0 24 24"
                    stroke="currentColor"
                >
                    <path
                        v-if="isCollapsed"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M13 5l7 7-7 7M5 5l7 7-7 7"
                    />
                    <path
                        v-else
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M11 19l-7-7 7-7m8 14l-7-7 7-7"
                    />
                </svg>
                <span
                    v-if="!isCollapsed"
                    class="ml-1.5 text-xs whitespace-nowrap"
                    >Collapse</span
                >
            </button>
        </div>
    </nav>

    <!-- Mobile Bottom Navigation (only on mobile). Two pinned route
         links + Search + "More" (overflow sheet) so each tap target
         stays well above 44 CSS pixels on a 360px viewport. The full
         link list still ships via the overflow sheet so a returning
         user never finds a destination missing — only re-homed. -->
    <nav
        class="fixed bottom-0 left-0 right-0 bg-surface-alt border-t border-default z-20 sm:hidden print:hidden pb-[env(safe-area-inset-bottom)] pl-[env(safe-area-inset-left)] pr-[env(safe-area-inset-right)]"
        v-if="isMobile"
    >
        <div class="flex justify-around items-center h-12">
            <RouterLink
                v-for="link in primaryMobileLinks"
                :key="link.to"
                :to="link.to"
                class="flex items-center justify-center p-3 rounded-lg transition-all duration-200 active:scale-95 flex-1 min-h-[44px]"
                :class="
                    isRouteActive(link.to, link.exact) ? 'text-accent' : 'text-secondary'
                "
                :aria-label="link.text"
                :title="link.text"
            >
                <svg
                    class="w-6 h-6"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                >
                    <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        :d="link.icon"
                    />
                </svg>
            </RouterLink>

            <button
                type="button"
                @click="toggleMobileMore"
                class="flex items-center justify-center p-3 rounded-lg transition-all duration-200 active:scale-95 flex-1 min-h-[44px]"
                :class="
                    isMobileMoreOpen || isOverflowRouteActive
                        ? 'text-accent'
                        : 'text-secondary'
                "
                :aria-expanded="isMobileMoreOpen"
                aria-controls="mobile-nav-more-sheet"
                aria-label="More navigation"
                title="More"
            >
                <svg
                    class="w-6 h-6"
                    viewBox="0 0 24 24"
                    fill="currentColor"
                >
                    <circle cx="5" cy="12" r="2" />
                    <circle cx="12" cy="12" r="2" />
                    <circle cx="19" cy="12" r="2" />
                </svg>
            </button>
        </div>
    </nav>

    <!-- Overflow sheet rendered as a native <dialog>. The browser's
         top layer renders above every page surface (no z-index
         juggling), and `.showModal()` gives focus trap, Esc-to-
         dismiss, focus restoration on close, and proper `inert`
         semantics on background content for free. We sync the
         imperative open/close to a reactive ref via a watcher. -->
    <dialog
        v-if="isMobile"
        ref="mobileMoreDialogRef"
        id="mobile-nav-more-sheet"
        class="mobile-nav-sheet sm:hidden print:hidden"
        aria-labelledby="mobile-nav-more-heading"
        @close="closeMobileMore"
        @click="onDialogBackdropClick"
    >
        <div
            class="mobile-nav-sheet-panel bg-surface border-t border-default rounded-t-2xl shadow-xl"
        >
            <h2 id="mobile-nav-more-heading" class="sr-only">More navigation</h2>
            <nav aria-label="Secondary navigation">
                <ul class="grid grid-cols-2 gap-2 p-3">
                    <li v-for="link in overflowMobileLinks" :key="link.to">
                        <RouterLink
                            :to="link.to"
                            class="flex items-center gap-3 px-3 py-3 rounded-lg min-h-[44px] transition-colors motion-safe:active:scale-[0.98]"
                            :class="
                                isRouteActive(link.to, link.exact)
                                    ? 'bg-accent/10 text-accent'
                                    : 'text-primary hover:bg-surface-hover'
                            "
                            @click="closeMobileMore"
                        >
                            <svg
                                class="w-5 h-5 flex-shrink-0"
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                aria-hidden="true"
                            >
                                <path
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                    stroke-width="2"
                                    :d="link.icon"
                                />
                            </svg>
                            <span class="text-sm font-medium truncate">{{ link.text }}</span>
                        </RouterLink>
                    </li>
                    <!-- Global search lives in the overflow sheet
                         alongside the navigation routes: not a
                         RouterLink because it opens a modal rather
                         than navigating. Close the sheet first so
                         the search palette doesn't surface stacked
                         under it. -->
                    <li>
                        <button
                            type="button"
                            class="w-full text-left flex items-center gap-3 px-3 py-3 rounded-lg min-h-[44px] transition-colors motion-safe:active:scale-[0.98] text-primary hover:bg-surface-hover"
                            @click="closeMobileMore(); openSearch();"
                        >
                            <Icon name="search" size="md" class="flex-shrink-0" />
                            <span class="text-sm font-medium truncate">Search</span>
                        </button>
                    </li>
                </ul>
            </nav>
        </div>
    </dialog>
</template>

<style scoped>
/* Optimize resizable sections with hardware acceleration hints */
.tickets-section,
.docs-section {
    will-change: max-height;
    transform: translateZ(0); /* Force GPU acceleration */
    backface-visibility: hidden;
    perspective: 1000px;
    transition: max-height 0.2s cubic-bezier(0.25, 1, 0.5, 1); /* Optimized easing function */
}

/* Remove transition during active resizing to prevent lag */
:global(.resize-active) .tickets-section,
:global(.resize-active) .docs-section {
    transition: none !important;
}

/* Styles for resizer handle, active state, etc. */
.resizer-handle {
    touch-action: none;
    position: relative;
    z-index: 1;
    height: 5px;
    margin: 0;
    cursor: ns-resize;
    background-color: var(--color-surface);
    border-top: 1px solid var(--color-border-default);
    border-bottom: 1px solid var(--color-border-default);
}

.resizer-handle:hover {
    background-color: var(--color-surface-hover);
}

.resizer-handle:active,
.resizer-handle.active {
    background-color: rgba(96, 165, 250, 0.3);
}

/* Keep the blue line indicator on hover/active, but make it more subtle */
.resizer-handle:hover::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    height: 0.5px; /* Thinner line on hover */
    background-color: rgba(96, 165, 250, 0.3); /* Much more transparent blue */
    top: 50%;
    transform: translateY(-50%);
    opacity: 0.5; /* Lower opacity */
    z-index: 5;
    pointer-events: none;
}

/* Slightly more visible but still subtle when actively resizing */
.resizer-handle.active::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    height: 0.5px;
    background-color: rgba(96, 165, 250, 0.5); /* More visible when active */
    top: 50%;
    transform: translateY(-50%);
    opacity: 0.6;
    z-index: 5;
    pointer-events: none;
}

/* Visual feedback for resize cursor position */
:global(.resize-active) {
    cursor: ns-resize !important;
    user-select: none !important;
}

:global(.resize-active *) {
    user-select: none !important;
    pointer-events: none !important;
}

/* Ensure the resizer itself remains interactive during resize */
:global(.resize-active .resizer-handle) {
    pointer-events: auto !important;
}

/* Mobile overflow sheet rendered as a native <dialog>. The element
   defaults to a centered margin:auto box; we reposition to a full-
   width bottom anchor so it reads as a sheet rather than a modal.
   The browser's top-layer rendering means we don't need a z-index. */
.mobile-nav-sheet {
    margin: 0;
    padding: 0;
    border: 0;
    background: transparent;
    width: 100%;
    max-width: 100%;
    max-height: 100%;
    position: fixed;
    top: auto;
    bottom: 0;
    left: 0;
    right: 0;
}

.mobile-nav-sheet::backdrop {
    background-color: rgba(0, 0, 0, 0.4);
}

.mobile-nav-sheet-panel {
    padding-bottom: env(safe-area-inset-bottom);
    padding-left: env(safe-area-inset-left);
    padding-right: env(safe-area-inset-right);
}

/* Slide-up entry, fade-out exit. Animations gated on motion-safe so
   users with prefers-reduced-motion get the dialog instantly. */
@media (prefers-reduced-motion: no-preference) {
    .mobile-nav-sheet[open] .mobile-nav-sheet-panel {
        animation: mobile-nav-sheet-slide-up 240ms cubic-bezier(0.2, 0, 0, 1);
    }
    .mobile-nav-sheet[open]::backdrop {
        animation: mobile-nav-sheet-backdrop-fade 180ms ease-out;
    }
}

@keyframes mobile-nav-sheet-slide-up {
    from { transform: translateY(100%); }
    to   { transform: translateY(0); }
}
@keyframes mobile-nav-sheet-backdrop-fade {
    from { background-color: rgba(0, 0, 0, 0); }
    to   { background-color: rgba(0, 0, 0, 0.4); }
}
</style>
