<script setup lang="ts">
import { RouterLink, useRoute, useRouter } from "vue-router";
import DocumentationNav from "@/components/documentationComponents/DocumentationNav.vue";
import RecentTickets from "@/components/RecentTickets.vue";
import CollapsibleSection from "@/components/common/CollapsibleSection.vue";
import LogoIcon from "@/components/icons/LogoIcon.vue";
import FaviconIcon from "@/components/icons/FaviconIcon.vue";
import { ref, computed, onMounted, onBeforeUnmount, nextTick, watch } from "vue";
import { useResizableSidebar } from "@/composables/useResizableSidebar";
import { useNavbarState } from "@/composables/useNavbarState";
import { useBrandingStore } from "@/stores/branding";
import { useThemeStore } from "@/stores/theme";

// Get branding and theme stores
const brandingStore = useBrandingStore();
const themeStore = useThemeStore();

// Computed logo URL based on current theme
const logoUrl = computed(() => {
    const customLogo = brandingStore.getLogoUrl(themeStore.isDarkMode);
    return customLogo || null;
});

const route = useRoute();
const router = useRouter();
const searchTerm = ref("");

// Use centralized navbar state composable
const {
    isCollapsed,
    isMobile,
    isTablet,
    isDesktop,
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

// Computed property to check if on a documentation page
const isDocumentationPage = computed(() => {
    return route.path.startsWith("/documentation");
});

// Handle documentation search
const handleDocSearch = (query: string) => {
    if (isDocumentationPage.value) {
        // If already on documentation page, update the search query
        searchTerm.value = query;
    } else {
        // If not on documentation page, navigate to documentation with search query
        router.push({
            path: "/documentation",
            query: { search: query },
        });
    }
};

// Navigation links data - colors removed, will use theme accent
const navLinks = [
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
        to: "/projects",
        icon: "M4 4h4v16H4V4zm6 0h4v12h-4V4zm6 0h4v8h-4V4z",
        text: "Projects",
    },
    {
        to: "/users",
        icon: "M12 4.354a4 4 0 110 5.292M15 21H3v-1a6 6 0 0112 0v1zm0 0h6v-1a6 6 0 00-9-5.197M13 7a4 4 0 11-8 0 4 4 0 018 0z",
        text: "Users",
    },
    {
        to: "/devices",
        icon: "M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z",
        text: "Devices",
    },
    {
        to: "/documentation",
        icon: "M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253",
        text: "Documentation",
    },
];

// Helper function to check if a route is active
const isRouteActive = (path: string, exact = false) => {
    if (exact) {
        return route.path === path;
    }
    return route.path.startsWith(path);
};
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

            <div
                class="mb-2"
                :class="[
                    isCompactNav && !isCollapsed
                        ? 'grid grid-cols-6 gap-0.5'
                        : 'flex flex-col gap-1'
                ]"
            >
                <RouterLink
                    v-for="link in navLinks"
                    :key="link.to"
                    :to="link.to"
                    class="rounded-md transition-colors duration-200 flex items-center relative overflow-hidden"
                    :class="[
                        isRouteActive(link.to, link.exact)
                            ? 'bg-surface-alt/80 text-primary font-medium'
                            : 'text-secondary hover:bg-surface-hover hover:text-primary',
                        isCollapsed || isCompactNav
                            ? 'px-2 py-1.5 justify-center'
                            : 'px-3 py-2 gap-3',
                    ]"
                    :title="isCollapsed || isCompactNav ? link.text : ''"
                >
                    <!-- Active indicator bar - uses theme accent color -->
                    <div
                        v-if="isRouteActive(link.to, link.exact)"
                        class="absolute left-0 top-0 bottom-0 w-1 bg-accent"
                        :class="{ 'w-full h-0.5 top-auto': isCompactNav && !isCollapsed }"
                    ></div>

                    <svg
                        class="w-4 h-4"
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
                    <span
                        v-if="!isCollapsed && !isCompactNav"
                        class="text-sm whitespace-nowrap"
                        >{{ link.text }}</span
                    >
                </RouterLink>
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
                    @mousedown="startResize"
                    @touchstart.prevent="startResize"
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
                    <DocumentationNav @search="handleDocSearch" />
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

    <!-- Mobile Bottom Navigation (only on mobile) -->
    <nav
        class="fixed bottom-0 left-0 right-0 bg-surface-alt border-t border-default z-20 sm:hidden print:hidden pb-[env(safe-area-inset-bottom)]"
        v-if="isMobile"
    >
        <div class="flex justify-around items-center h-12">
            <RouterLink
                v-for="link in navLinks"
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
        </div>
    </nav>

    <!-- Overlay not needed - sidebar is hidden on mobile, visible but docked on tablet/desktop -->
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
</style>
