<script setup lang="ts">
import AssetImg from "@/components/common/AssetImg.vue";
import { RouterLink, useRoute } from "vue-router";
import { workspacePath } from "@/router/workspaceRouting";
import DocumentationNav from "@/components/documentationComponents/DocumentationNav.vue";
import RecentTickets from "@/components/RecentTickets.vue";
import CollapsibleSection from "@/components/common/CollapsibleSection.vue";
import LogoIcon from "@/components/icons/LogoIcon.vue";
import FaviconIcon from "@/components/icons/FaviconIcon.vue";
import { ref, computed, onMounted, onBeforeUnmount, nextTick, watch } from "vue";
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
import type { IconName } from "@/components/common/icons";
import UnreadBadge from "@/components/common/UnreadBadge.vue";
import NavLinkIcon from "@/components/NavLinkIcon.vue";
import Tooltip from "@/components/common/Tooltip.vue";
import Popover from "@/components/common/Popover.vue";
import { getSlotRegistrations } from "@/plugins/loader";
import { pluginPagePath } from "@/plugins/pluginPage";
import { useWorkspaceCapabilities } from "@/composables/useWorkspaceCapabilities";

// Global search
const { openSearch } = useGlobalSearch();

// Mobile bottom-nav Inbox tile borrows the bell's unread count
// from the same shared feed composable so the badge agrees with
// the inbox view's count without an extra round-trip. The query
// cache deduplicates, so this extra consumer is free.
const { unreadCount } = useNotificationFeed('all');

// Keyboard shortcut hint based on platform
const isMac = /Mac|iPhone|iPad|iPod/.test(navigator.userAgent);
const searchShortcut = isMac ? '\u2318K' : 'Ctrl+K';
// The same shortcut for assistive tech (aria-keyshortcuts).
const searchKeyshortcuts = isMac ? 'Meta+K' : 'Control+K';

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

const navbarRef = ref<HTMLElement | null>(null);

// Rail flyout for recent tickets, anchored on its icon button. Closes
// on navigation (opening a ticket) and when the rail expands.
const railRecentButton = ref<HTMLElement | null>(null);
const isRailRecentOpen = ref(false);
const railRecentAnchor = computed(() => ({ type: "element" as const, element: () => railRecentButton.value }));
const toggleRailRecent = () => { isRailRecentOpen.value = !isRailRecentOpen.value; };
const closeRailRecent = () => { isRailRecentOpen.value = false; };
watch(() => route.fullPath, closeRailRecent);
watch(isCollapsed, closeRailRecent);

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
    /** Registry icon name. Absent on plugin links, which carry `iconUrl`. */
    icon?: IconName;
    text: string;
    /** True for paths whose exact match should be the
     *  only "active" trigger. Without this, e.g. `/` would
     *  match every route (since every path starts with `/`). */
    exact?: boolean;
    /** Pre-resolved literal label (plugin nav items). When set, rendered
     *  instead of `$t(text)` — plugin labels aren't FTL keys. */
    rawLabel?: string;
    /** Icon URL (plugin nav items). When set, rendered as an `<img>` instead of
     *  the `icon` registry glyph. */
    iconUrl?: string;
    /** Links nested under this one in the expanded sidebar (plugin
     *  `nav.assets.item` contributions under Assets). */
    children?: NavLink[];
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
                icon: "home",
                text: "nav-dashboard",
                exact: true,
            },
            {
                to: "/tickets",
                icon: "ticket",
                text: "nav-tickets",
            },
            {
                to: "/projects",
                icon: "project",
                text: "nav-projects",
            },
        ],
    },
    {
        label: "nav-group-resources",
        links: [
            {
                to: "/assets",
                icon: "device",
                text: "nav-assets",
            },
            {
                to: "/users",
                icon: "team",
                text: "nav-users",
            },
            {
                to: "/documentation",
                icon: "book",
                text: "nav-documentation",
            },
        ],
    },
];

// Plugin `nav.item` contributions become a trailing "Plugins" group linking to
// each plugin's full-page surface. Labels are pre-resolved at load time; icons
// are plugin icon URLs (rendered as <img>, not a registry glyph). The group appears
// only when a plugin contributes one.
const pluginNavGroup = computed<NavGroup | null>(() => {
    const regs = getSlotRegistrations('nav.item');
    if (regs.length === 0) return null;
    return {
        label: "nav-group-plugins",
        links: regs.map((r) => ({
            to: pluginPagePath(r.pluginUuid, r.componentName),
            iconUrl: r.icon,
            text: "",
            rawLabel: r.label ?? r.pluginName,
        })),
    };
});

// A requester in a workspace that shows requesters no assets has no Assets
// page to go to.
const { assetsVisible } = useWorkspaceCapabilities();
const coreNavGroups = computed<NavGroup[]>(() =>
    assetsVisible.value
        ? staticNavGroups
        : staticNavGroups.map((g) => ({
              ...g,
              links: g.links.filter((l) => l.to !== "/assets"),
          })),
);

// Plugin `nav.assets.item` contributions nest under the Assets link, linking
// to the same full-page surface as `nav.item`.
const pluginAssetLinks = computed<NavLink[]>(() =>
    getSlotRegistrations('nav.assets.item').map((r) => ({
        to: pluginPagePath(r.pluginUuid, r.componentName),
        iconUrl: r.icon,
        text: "",
        rawLabel: r.label ?? r.pluginName,
    })),
);

const navGroups = computed<NavGroup[]>(() => {
    const children = pluginAssetLinks.value;
    const groups = children.length === 0
        ? coreNavGroups.value
        : coreNavGroups.value.map((g) => ({
            ...g,
            links: g.links.map((l) => (l.to === "/assets" ? { ...l, children } : l)),
        }));
    return pluginNavGroup.value ? [...groups, pluginNavGroup.value] : groups;
});

/** Flat list used by the collapsed / compact-nav layouts where
 *  section headers wouldn't fit. Kept derived so adding a new
 *  link only ever requires editing the grouped source. Nested
 *  links follow their parent. */
const navLinks = computed<NavLink[]>(() =>
    navGroups.value.flatMap((g) => g.links.flatMap((l) => [l, ...(l.children ?? [])])),
);

/** Synthetic NavLink for the inbox. Kept out of `navGroups` (which
 *  drives the desktop sidebar) because Inbox lives in the header
 *  cluster alongside the bell on desktop; the sidebar shouldn't
 *  carry a duplicate entry. The bottom-nav slot is the only place
 *  it surfaces from this file. */
const INBOX_MOBILE_LINK: NavLink = {
    to: '/inbox',
    icon: 'inbox',
    text: 'nav-inbox',
};

/** All routes a user can pin to the mobile bar. The static core routes plus the
 *  synthetic Inbox entry; Search stays a fixed cell on the bar (it's a button,
 *  not a route) and "More" is the overflow opener, also fixed. Plugin nav items
 *  are intentionally excluded — the mobile pin set is a curated core-route set. */
const pinnableLinks = computed<NavLink[]>(() => [
    INBOX_MOBILE_LINK,
    ...coreNavGroups.value.flatMap((g) => g.links),
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

// Whether a nav link's page is open. Links are bare app paths, so they are
// compared to the path inside the workspace: on Cloud `route.path` starts
// with the workspace slug (`/acme/tickets`).
const isRouteActive = (path: string, exact = false) => {
    const current = workspacePath(route);
    if (exact) {
        return current === path;
    }
    return current.startsWith(path);
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
        :aria-label="$t('nav-primary')"
        class="h-screen bg-surface border-r border-default flex flex-col gap-1 flex-shrink-0 print:hidden transition-[width] duration-300 ease-in-out overflow-hidden pt-[env(safe-area-inset-top)] pl-[env(safe-area-inset-left)]"
        :class="[isCollapsed ? 'w-16' : 'w-64', isMobile ? 'hidden' : '']"
    >
        <!-- Logo - swaps between full logo and icon based on collapsed state -->
        <div class="flex flex-col p-2 gap-2 flex-shrink-0">
            <RouterLink
                to="/"
                :aria-label="$t('nav-logo-alt')"
                class="sidebar-logo flex items-center justify-center h-12 hover:opacity-80 transition-opacity select-none"
            >
                <!-- Full logo when expanded -->
                <AssetImg
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
                <AssetImg
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


            <!-- Search, above the nav links. A field when expanded, a bare
                 icon with a tooltip in the rail. Expanded, its name is what it
                 shows ("Search Ctrl+K"), so a speech user can say it (WCAG
                 2.5.3); the bare icon is named by its label. -->
            <Tooltip :text="$t('nav-search-with-shortcut', { shortcut: searchShortcut })" side="right" :disabled="!isCollapsed">
                <button
                    type="button"
                    @click="() => openSearch()"
                    class="w-full h-7 rounded-md transition-colors duration-200 flex items-center text-secondary hover:bg-surface-hover hover:text-primary focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent"
                    :class="[
                        isCollapsed
                            ? 'px-2 justify-center'
                            : 'px-2.5 gap-2 justify-between bg-surface-alt border border-default hover:border-subtle',
                    ]"
                    :aria-label="isCollapsed ? $t('nav-search') : undefined"
                    :aria-keyshortcuts="searchKeyshortcuts"
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
            </Tooltip>

            <!--
              Three rendering modes for the nav links:

              1. Collapsed sidebar (w-16, icon-only): flat vertical
                 list of icons, each named by a tooltip.
              2. Compact nav (short viewports, see useNavbarState): a
                 flat, denser list that keeps the labels but drops the
                 group headers, giving the lower sections back about
                 100px without losing the words.
              3. Expanded sidebar (default): grouped sections with
                 small uppercase headers (Work / Resources).
            -->
            <div v-if="isCompactNav && !isCollapsed" class="flex flex-col gap-0.5">
                <RouterLink
                    v-for="link in navLinks"
                    :key="link.to"
                    :to="link.to"
                    v-prefetch="link.to"
                    class="rounded-md transition-colors duration-200 flex items-center px-2.5 py-0.5 gap-2.5 focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent"
                    :class="
                        isRouteActive(link.to, link.exact)
                            ? 'bg-surface-alt/80 text-primary font-medium'
                            : 'text-secondary hover:bg-surface-hover hover:text-primary'
                    "
                >
                    <NavLinkIcon :icon="link.icon" :icon-url="link.iconUrl" />
                    <span class="text-sm whitespace-nowrap">{{ link.rawLabel ?? $t(link.text) }}</span>
                </RouterLink>
            </div>

            <div v-else-if="isCollapsed" class="flex flex-col gap-0.5">
                <Tooltip
                    v-for="link in navLinks"
                    :key="link.to"
                    :text="link.rawLabel ?? $t(link.text)"
                    side="right"
                >
                    <RouterLink
                        :to="link.to"
                        v-prefetch="link.to"
                        class="rounded-md transition-colors duration-200 flex items-center px-2 py-1.5 justify-center focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent"
                        :class="
                            isRouteActive(link.to, link.exact)
                                ? 'bg-surface-alt/80 text-accent'
                                : 'text-secondary hover:bg-surface-hover hover:text-primary'
                        "
                        :aria-label="link.rawLabel ?? $t(link.text)"
                    >
                        <NavLinkIcon :icon="link.icon" :icon-url="link.iconUrl" />
                    </RouterLink>
                </Tooltip>
                <div class="border-t border-default/50 my-1"></div>
                <Tooltip :text="$t('nav-section-recent-tickets')" side="right">
                    <button
                        ref="railRecentButton"
                        type="button"
                        class="rounded-md transition-colors duration-200 flex items-center px-2 py-1.5 justify-center focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent"
                        :class="isRailRecentOpen ? 'bg-surface-alt/80 text-primary' : 'text-secondary hover:bg-surface-hover hover:text-primary'"
                        :aria-label="$t('nav-section-recent-tickets')"
                        aria-haspopup="dialog"
                        :aria-expanded="isRailRecentOpen"
                        @click="toggleRailRecent"
                    >
                        <Icon name="clock" />
                    </button>
                </Tooltip>
                <Popover
                    :open="isRailRecentOpen"
                    :anchor="railRecentAnchor"
                    placement="right-start"
                    :offset="8"
                    :auto-focus="false"
                    role="dialog"
                    :aria-label="$t('nav-section-recent-tickets')"
                    popover-class="w-64 max-h-[70vh] flex flex-col overflow-hidden rounded-lg border border-default bg-surface shadow-lg"
                    @close="closeRailRecent"
                >
                    <div class="px-2.5 h-7 flex items-center text-3xs font-semibold text-tertiary tracking-wide uppercase select-none">
                        {{ $t('nav-section-recent-tickets') }}
                    </div>
                    <div class="px-2 pb-1 min-h-0 flex-1 overflow-y-auto">
                        <RecentTickets />
                    </div>
                </Popover>
            </div>

            <div v-else class="flex flex-col gap-3">
                <div
                    v-for="group in navGroups"
                    :key="group.label"
                    class="flex flex-col gap-0.5"
                >
                    <h3
                        class="px-2.5 text-3xs font-semibold text-tertiary tracking-wide uppercase select-none"
                    >
                        {{ $t(group.label) }}
                    </h3>
                    <template v-for="link in group.links" :key="link.to">
                        <RouterLink
                            :to="link.to"
                            v-prefetch="link.to"
                            class="rounded-md transition-colors duration-200 flex items-center px-2.5 py-1 gap-2.5 focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent"
                            :class="
                                isRouteActive(link.to, link.exact)
                                    ? 'bg-surface-alt/80 text-primary font-medium'
                                    : 'text-secondary hover:bg-surface-hover hover:text-primary'
                            "
                        >
                            <NavLinkIcon :icon="link.icon" :icon-url="link.iconUrl" />
                            <span class="text-sm whitespace-nowrap">{{ link.rawLabel ?? $t(link.text) }}</span>
                        </RouterLink>
                        <!-- Nested links (plugin `nav.assets.item`), indented
                             under their parent. -->
                        <div v-if="link.children?.length" class="flex flex-col gap-0.5 pl-5">
                            <RouterLink
                                v-for="child in link.children"
                                :key="child.to"
                                :to="child.to"
                                class="rounded-md transition-colors duration-200 flex items-center px-2.5 py-0.5 gap-2 focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent"
                                :class="
                                    isRouteActive(child.to, child.exact)
                                        ? 'bg-surface-alt/80 text-primary font-medium'
                                        : 'text-secondary hover:bg-surface-hover hover:text-primary'
                                "
                            >
                                <NavLinkIcon :icon="child.icon" :icon-url="child.iconUrl" />
                                <span class="text-xs whitespace-nowrap truncate">{{ child.rawLabel ?? $t(child.text) }}</span>
                            </RouterLink>
                        </div>
                    </template>
                </div>
            </div>
        </div>

        <!-- Separator -->
        <div class="border-t border-default/50"></div>

        <!-- Spacer: Always present to push toggle button to bottom -->
        <div class="flex-1 min-h-0 flex flex-col overflow-hidden">
            <!-- Only show sections when navbar is expanded -->
            <div
                class="flex-1 min-h-0 flex flex-col overflow-hidden px-2"
                v-if="!isCollapsed"
            >
                <!-- Recent tickets: the newest few, sized to content;
                     Documentation takes the remainder. -->
                <CollapsibleSection
                    :title="$t('nav-section-recent-tickets')"
                    :is-collapsed="isTicketsCollapsed"
                    class="tickets-section flex-shrink-0"
                    @toggle="toggleTickets"
                >
                    <RecentTickets />
                </CollapsibleSection>

                <!-- Documentation section with collapsible header -->
                <CollapsibleSection
                    :title="$t('nav-section-documentation')"
                    :is-collapsed="isDocsCollapsed"
                    class="docs-section flex-1 min-h-0"
                    @toggle="toggleDocs"
                >
                    <DocumentationNav />
                </CollapsibleSection>
            </div>
        </div>

        <!-- Toggle button at the bottom of sidebar (hidden on mobile) -->
        <div class="flex-shrink-0 border-t border-default" v-if="!isMobile">
            <Tooltip :text="$t('nav-expand-sidebar')" side="right" :disabled="!isCollapsed">
                <button
                    type="button"
                    @click="toggleCollapsed"
                    class="w-full h-8 px-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-md transition-colors group flex items-center justify-center focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent"
                    :aria-label="isCollapsed ? $t('nav-expand-sidebar') : $t('nav-collapse-sidebar')"
                    :aria-expanded="!isCollapsed"
                >
                    <Icon
                        :name="isCollapsed ? 'chevronDoubleRight' : 'chevronDoubleLeft'"
                        class="group-hover:text-accent transition-colors"
                    />
                    <span
                        v-if="!isCollapsed"
                        class="ml-1.5 text-xs whitespace-nowrap"
                        >{{ $t('nav-collapse') }}</span
                    >
                </button>
            </Tooltip>
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
                <Icon v-if="link.icon" :name="link.icon" size="lg" />
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
                <Icon name="moreHorizontal" size="lg" />
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
                            <Icon v-if="link.icon" :name="link.icon" size="md" class="flex-shrink-0" />
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
                            <Icon :name="isPinned(link.to) ? 'starFilled' : 'star'" size="md" />
                        </button>
                    </li>
                </ul>
            </nav>
        </div>
    </dialog>
</template>

<style scoped>
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
