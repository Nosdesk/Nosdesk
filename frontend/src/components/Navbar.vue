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
import { useNotificationFeed } from "@/composables/useNotificationFeed";
import {
    useMobileNavPins,
    DEFAULT_MOBILE_PINS,
    MAX_MOBILE_PINS,
} from "@/composables/useMobileNavPins";
import { useBrandingStore } from "@/stores/branding";
import { useThemeStore } from "@/stores/theme";
import Icon from "@/components/common/Icon.vue";
import UnreadBadge from "@/components/common/UnreadBadge.vue";
import NavLinkIcon from "@/components/NavLinkIcon.vue";
import { getSlotRegistrations } from "@/plugins/loader";
import { pluginPagePath } from "@/plugins/pluginPage";

// Global search
const { openSearch } = useGlobalSearch();

// Mobile bottom-nav Inbox tile borrows the bell's unread count
// from the same shared feed composable so the badge agrees with
// the inbox view's count without an extra round-trip. The query
// cache deduplicates, so this extra consumer is free.
const { unreadCount } = useNotificationFeed();

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
    /** Pre-resolved literal label (plugin nav items). When set, rendered
     *  instead of `$t(text)` — plugin labels aren't FTL keys. */
    rawLabel?: string;
    /** Icon URL (plugin nav items). When set, rendered as an `<img>` instead of
     *  the `icon` SVG path. */
    iconUrl?: string;
}
interface NavGroup {
    label: string;
    links: NavLink[];
}
// `text` and `label` are FTL keys; templates render them via
// `$t(link.text)` / `$t(group.label)` so the nav re-renders
// when the active locale flips.
const staticNavGroups: NavGroup[] = [
    {
        label: "nav-group-work",
        links: [
            {
                to: "/",
                icon: "M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6",
                text: "nav-dashboard",
                exact: true,
            },
            {
                to: "/tickets",
                icon: "M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-6 9l2 2 4-4",
                text: "nav-tickets",
            },
            {
                to: "/projects",
                icon: "M4 4h4v16H4V4zm6 0h4v12h-4V4zm6 0h4v8h-4V4z",
                text: "nav-projects",
            },
        ],
    },
    {
        label: "nav-group-resources",
        links: [
            {
                to: "/assets",
                icon: "M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z",
                text: "nav-assets",
            },
            {
                to: "/users",
                icon: "M12 4.354a4 4 0 110 5.292M15 21H3v-1a6 6 0 0112 0v1zm0 0h6v-1a6 6 0 00-9-5.197M13 7a4 4 0 11-8 0 4 4 0 018 0z",
                text: "nav-users",
            },
            {
                to: "/documentation",
                icon: "M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253",
                text: "nav-documentation",
            },
        ],
    },
];

// Plugin `nav.item` contributions become a trailing "Plugins" group linking to
// each plugin's full-page surface. Labels are pre-resolved at load time; icons
// are plugin icon URLs (rendered as <img>, not an SVG path). The group appears
// only when a plugin contributes one.
const pluginNavGroup = computed<NavGroup | null>(() => {
    const regs = getSlotRegistrations('nav.item');
    if (regs.length === 0) return null;
    return {
        label: "nav-group-plugins",
        links: regs.map((r) => ({
            to: pluginPagePath(r.pluginUuid, r.componentName),
            icon: "",
            iconUrl: r.icon,
            text: "",
            rawLabel: r.label ?? r.pluginName,
        })),
    };
});

const navGroups = computed<NavGroup[]>(() =>
    pluginNavGroup.value ? [...staticNavGroups, pluginNavGroup.value] : staticNavGroups,
);

/** Flat list used by the collapsed / compact-nav layouts where
 *  section headers wouldn't fit. Kept derived so adding a new
 *  link only ever requires editing the grouped source. */
const navLinks = computed<NavLink[]>(() => navGroups.value.flatMap((g) => g.links));

/** Synthetic NavLink for the inbox. Kept out of `navGroups` (which
 *  drives the desktop sidebar) because Inbox lives in the header
 *  cluster alongside the bell on desktop; the sidebar shouldn't
 *  carry a duplicate entry. The bottom-nav slot is the only place
 *  it surfaces from this file. */
const INBOX_MOBILE_LINK: NavLink = {
    to: '/inbox',
    icon: 'M20 13V6a2 2 0 00-2-2H6a2 2 0 00-2 2v7m16 0v5a2 2 0 01-2 2H6a2 2 0 01-2-2v-5m16 0h-2.586a1 1 0 00-.707.293l-2.414 2.414a1 1 0 01-.707.293h-3.172a1 1 0 01-.707-.293l-2.414-2.414A1 1 0 006.586 13H4',
    text: 'nav-inbox',
};

/** All routes a user can pin to the mobile bar. The static core routes plus the
 *  synthetic Inbox entry; Search stays a fixed cell on the bar (it's a button,
 *  not a route) and "More" is the overflow opener, also fixed. Plugin nav items
 *  are intentionally excluded — the mobile pin set is a curated core-route set. */
const pinnableLinks = computed<NavLink[]>(() => [
    INBOX_MOBILE_LINK,
    ...staticNavGroups.flatMap((g) => g.links),
]);

const { pinnedPaths, isPinned, togglePin, resetToDefaults, remainingSlots } =
    useMobileNavPins();

/** Materialise the pinned-path list into NavLink rows for the bar.
 *  Falls back to the shipped defaults when a user's saved list
 *  resolves to nothing renderable (every path was removed from the
 *  product, etc.) so the bar never collapses to just Search +
 *  More. Order matches the user's pin order. */
const primaryMobileLinks = computed<NavLink[]>(() => {
    const lookup = new Map(pinnableLinks.value.map((l) => [l.to, l]));
    const resolved = pinnedPaths.value
        .map((p) => lookup.get(p))
        .filter((l): l is NavLink => !!l);
    if (resolved.length > 0) return resolved;
    return DEFAULT_MOBILE_PINS.map((p) => lookup.get(p)).filter(
        (l): l is NavLink => !!l,
    );
});

/** Everything pinnable that ISN'T currently pinned shows up in the
 *  overflow sheet. Order follows the natural navLinks order so an
 *  admin's pinning choice doesn't reshuffle the sheet too. */
const overflowMobileLinks = computed<NavLink[]>(() =>
    pinnableLinks.value.filter((l) => !pinnedPaths.value.includes(l.to)),
);

// Edit mode on the More sheet: while on, each row in the sheet
// renders a star toggle and a hint banner describes the cap. The
// state is local to this component instance — closing the sheet
// resets to false so a user who panics out doesn't reopen the
// sheet still in edit mode.
const isPinEditMode = ref(false);
function togglePinEditMode() {
    isPinEditMode.value = !isPinEditMode.value;
}

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
    // Edit mode never carries across an open/close cycle. If the
    // user dismissed the sheet via Esc or backdrop tap we'd
    // otherwise reopen still showing star toggles, which
    // disorients more than it helps.
    if (!open) isPinEditMode.value = false;
});

// Close the overflow sheet when the route changes (so following any
// link inside the sheet visually completes the navigation rather
// than leaving the sheet floating over the destination).
watch(() => route.path, () => {
    isMobileMoreOpen.value = false;
});

/** What rows render inside the sheet. Edit mode shows every
 *  pinnable destination (so the user can unpin pinned items too);
 *  read-only mode shows only items NOT already pinned (the
 *  classic "show me everything not in the bar" behaviour). */
const sheetRows = computed<NavLink[]>(() =>
    isPinEditMode.value ? pinnableLinks.value : overflowMobileLinks.value,
);

/** Intercept row taps so edit mode toggles the pin instead of
 *  navigating; without preventDefault the RouterLink would still
 *  fire its internal handler. Outside edit mode we fall through
 *  to the original close-the-sheet-after-nav behaviour. */
function onSheetRowClick(event: MouseEvent, link: NavLink) {
    if (isPinEditMode.value) {
        event.preventDefault();
        togglePin(link.to);
        return;
    }
    closeMobileMore();
}

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
                    :aria-label="$t('nav-logo-alt')"
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
                    :aria-label="$t('nav-logo-alt-collapsed')"
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
                :title="isCollapsed ? $t('nav-search') : ''"
            >
                <div class="flex items-center gap-2">
                    <Icon name="search" />
                    <span v-if="!isCollapsed" class="text-sm">{{ $t('nav-search') }}</span>
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
                    v-prefetch="link.to"
                    class="rounded-md transition-colors duration-200 flex items-center relative overflow-hidden px-2 py-1.5 justify-center"
                    :class="
                        isRouteActive(link.to, link.exact)
                            ? 'bg-surface-alt/80 text-primary font-medium'
                            : 'text-secondary hover:bg-surface-hover hover:text-primary'
                    "
                    :title="link.rawLabel ?? $t(link.text)"
                >
                    <div
                        v-if="isRouteActive(link.to, link.exact)"
                        class="absolute left-0 top-0 bottom-0 w-1 bg-accent w-full h-0.5 top-auto"
                    ></div>
                    <NavLinkIcon :icon="link.icon" :icon-url="link.iconUrl" />
                </RouterLink>
            </div>

            <div v-else-if="isCollapsed" class="flex flex-col gap-0.5">
                <RouterLink
                    v-for="link in navLinks"
                    :key="link.to"
                    :to="link.to"
                    v-prefetch="link.to"
                    class="rounded-md transition-colors duration-200 flex items-center relative overflow-hidden px-2 py-1.5 justify-center"
                    :class="
                        isRouteActive(link.to, link.exact)
                            ? 'bg-surface-alt/80 text-primary font-medium'
                            : 'text-secondary hover:bg-surface-hover hover:text-primary'
                    "
                    :title="link.rawLabel ?? $t(link.text)"
                >
                    <div
                        v-if="isRouteActive(link.to, link.exact)"
                        class="absolute left-0 top-0 bottom-0 w-1 bg-accent"
                    ></div>
                    <NavLinkIcon :icon="link.icon" :icon-url="link.iconUrl" />
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
                        {{ $t(group.label) }}
                    </h3>
                    <RouterLink
                        v-for="link in group.links"
                        :key="link.to"
                        :to="link.to"
                        v-prefetch="link.to"
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
                        <NavLinkIcon :icon="link.icon" :icon-url="link.iconUrl" />
                        <span class="text-sm whitespace-nowrap">{{ link.rawLabel ?? $t(link.text) }}</span>
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
                    :title="$t('nav-section-recent-tickets')"
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
                    :title="$t('nav-section-documentation')"
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
                :aria-label="$t('nav-toggle-sidebar')"
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
                    >{{ $t('nav-collapse') }}</span
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
                v-prefetch="link.to"
                class="relative flex items-center justify-center p-3 rounded-lg transition-all duration-200 active:scale-95 flex-1 min-h-[44px]"
                :class="
                    isRouteActive(link.to, link.exact) ? 'text-accent' : 'text-secondary'
                "
                :aria-label="link.to === '/inbox' && unreadCount > 0
                    ? `${$t(link.text)}, ${unreadCount} unread`
                    : $t(link.text)"
                :title="$t(link.text)"
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
                <!-- Unread badge: only the Inbox tile is wired to
                     the notification feed. Positioned over the
                     top-right of the icon (Twitter / GitHub Mobile
                     convention). The badge component handles the
                     hide-when-zero and 99+ cap rules. -->
                <UnreadBadge
                    v-if="link.to === '/inbox'"
                    :count="unreadCount"
                    class="absolute top-1.5 right-1/2 translate-x-[14px]"
                />
            </RouterLink>

            <button
                type="button"
                @click="() => openSearch()"
                class="flex items-center justify-center p-3 rounded-lg transition-all duration-200 active:scale-95 flex-1 min-h-[44px] text-secondary"
                :aria-label="$t('nav-search')"
                :title="$t('nav-search')"
            >
                <Icon name="search" size="lg" />
            </button>

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
                :aria-label="$t('nav-more')"
                :title="$t('nav-more')"
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
            <h2 id="mobile-nav-more-heading" class="sr-only">{{ $t('nav-more-heading') }}</h2>
            <!-- Header row: title, edit-mode toggle, reset link (only
                 visible while editing so the chrome stays minimal on
                 the common read-only path). -->
            <div class="flex items-center justify-between px-3 pt-3">
                <span class="text-sm font-medium text-secondary">
                    {{ isPinEditMode
                        ? $t('nav-pins-edit-hint', { max: MAX_MOBILE_PINS, remaining: remainingSlots })
                        : $t('nav-more-heading') }}
                </span>
                <div class="flex items-center gap-2">
                    <button
                        v-if="isPinEditMode"
                        type="button"
                        class="text-xs text-secondary hover:text-primary px-2 py-1 rounded transition-colors"
                        @click="resetToDefaults"
                    >
                        {{ $t('nav-pins-reset') }}
                    </button>
                    <button
                        type="button"
                        class="text-xs font-medium px-2 py-1 rounded transition-colors"
                        :class="isPinEditMode ? 'text-accent' : 'text-secondary hover:text-primary'"
                        @click="togglePinEditMode"
                    >
                        {{ isPinEditMode ? $t('nav-pins-done') : $t('nav-pins-edit') }}
                    </button>
                </div>
            </div>
            <nav :aria-label="$t('nav-secondary')">
                <ul class="grid grid-cols-2 gap-2 p-3">
                    <li v-for="link in sheetRows" :key="link.to" class="flex items-center gap-1">
                        <RouterLink
                            :to="link.to"
                            v-prefetch="link.to"
                            class="flex flex-1 items-center gap-3 px-3 py-3 rounded-lg min-h-[44px] transition-colors motion-safe:active:scale-[0.98]"
                            :class="
                                isRouteActive(link.to, link.exact)
                                    ? 'bg-accent/10 text-accent'
                                    : 'text-primary hover:bg-surface-hover'
                            "
                            @click="onSheetRowClick($event, link)"
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
                            <span class="text-sm font-medium truncate">{{ $t(link.text) }}</span>
                        </RouterLink>
                        <!-- Star toggle: only in edit mode. Disabled
                             when the cap is full AND this row isn't
                             already pinned (so the user can still
                             tap a filled star to unpin and free a
                             slot). aria-pressed conveys state. -->
                        <button
                            v-if="isPinEditMode"
                            type="button"
                            class="flex items-center justify-center w-10 h-10 rounded-lg transition-colors"
                            :class="
                                isPinned(link.to)
                                    ? 'text-accent hover:bg-accent/10'
                                    : remainingSlots === 0
                                        ? 'text-tertiary opacity-50 cursor-not-allowed'
                                        : 'text-secondary hover:bg-surface-hover'
                            "
                            :aria-pressed="isPinned(link.to)"
                            :aria-label="isPinned(link.to)
                                ? $t('nav-pins-unpin', { name: $t(link.text) })
                                : $t('nav-pins-pin', { name: $t(link.text) })"
                            :disabled="!isPinned(link.to) && remainingSlots === 0"
                            @click="togglePin(link.to)"
                        >
                            <svg
                                class="w-5 h-5"
                                viewBox="0 0 24 24"
                                :fill="isPinned(link.to) ? 'currentColor' : 'none'"
                                stroke="currentColor"
                                aria-hidden="true"
                            >
                                <path
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                    stroke-width="2"
                                    d="M11.049 2.927c.3-.921 1.603-.921 1.902 0l1.519 4.674a1 1 0 00.95.69h4.915c.969 0 1.371 1.24.588 1.81l-3.976 2.888a1 1 0 00-.363 1.118l1.518 4.674c.3.922-.755 1.688-1.538 1.118l-3.976-2.888a1 1 0 00-1.176 0l-3.976 2.888c-.783.57-1.838-.196-1.538-1.118l1.518-4.674a1 1 0 00-.363-1.118l-3.976-2.888c-.784-.57-.38-1.81.588-1.81h4.914a1 1 0 00.951-.69l1.519-4.674z"
                                />
                            </svg>
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
    background-color: color-mix(in srgb, var(--color-accent) 30%, transparent);
}

/* Keep the accent line indicator on hover/active, but make it more subtle */
.resizer-handle:hover::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    height: 0.5px; /* Thinner line on hover */
    background-color: color-mix(in srgb, var(--color-accent) 30%, transparent); /* Much more transparent accent */
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
    background-color: color-mix(in srgb, var(--color-accent) 50%, transparent); /* More visible when active */
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

/* Symmetric slide-up-in / slide-down-out. Transitioning the <dialog>
   itself (not the inner panel) with `@starting-style` + discrete
   `overlay`/`display` transitions lets the native element animate on
   BOTH open and close, instead of the panel-only entry that popped
   away instantly on close. Gated on motion-safe; reduced-motion users
   and engines without @starting-style just snap open/closed. */
@media (prefers-reduced-motion: no-preference) {
    .mobile-nav-sheet {
        transform: translateY(100%);
        transition:
            transform 260ms cubic-bezier(0.32, 0.72, 0, 1),
            overlay 260ms allow-discrete,
            display 260ms allow-discrete;
    }
    .mobile-nav-sheet[open] {
        transform: translateY(0);
    }
    @starting-style {
        .mobile-nav-sheet[open] {
            transform: translateY(100%);
        }
    }

    .mobile-nav-sheet::backdrop {
        opacity: 0;
        transition:
            opacity 260ms ease,
            overlay 260ms allow-discrete,
            display 260ms allow-discrete;
    }
    .mobile-nav-sheet[open]::backdrop {
        opacity: 1;
    }
    @starting-style {
        .mobile-nav-sheet[open]::backdrop {
            opacity: 0;
        }
    }
}
</style>
