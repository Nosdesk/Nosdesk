<script setup lang="ts">
import { RouterLink, useRoute, useRouter } from "vue-router";
import DocumentationNav from "@/components/documentationComponents/DocumentationNav.vue";
import RecentTickets from "@/components/RecentTickets.vue";
import CollapsibleSection from "@/components/common/CollapsibleSection.vue";
import {
    ref,
    watch,
    computed,
    onMounted,
    onBeforeUnmount,
    nextTick,
} from "vue";
import { useResizableSidebar } from "@/composables/useResizableSidebar";
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

// Computed favicon URL for collapsed state
const faviconUrl = computed(() => {
    return brandingStore.faviconUrl || '/favicon.svg';
});

const route = useRoute();
const router = useRouter();
const searchTerm = ref("");

// State for collapsed/expanded navbar
const isCollapsed = ref(false);
const isMobile = ref(false); // <640px - shows bottom nav (phones)
const isTablet = ref(false); // 640-1023px - shows collapsed sidebar (tablets/landscape phones)
const isDesktop = ref(false); // ≥1024px - shows expandable sidebar

// State for section collapsing
const isDocsCollapsed = ref(false);
const isTicketsCollapsed = ref(false);

// State for compact nav mode (small viewport height)
const isCompactNav = ref(false);

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

// Provide/inject for sharing with App.vue
const emit = defineEmits(["update:collapsed"]);

// Toggle navbar collapsed state
const toggleNav = () => {
    // Don't allow toggling on mobile (bottom nav is shown instead)
    if (isMobile.value) return;

    isCollapsed.value = !isCollapsed.value;
    emit("update:collapsed", isCollapsed.value);
    // Store preference in localStorage
    localStorage.setItem("navbarCollapsed", isCollapsed.value.toString());
};

// Toggle documentation section
const toggleDocs = () => {
    isDocsCollapsed.value = !isDocsCollapsed.value;
    localStorage.setItem("docsCollapsed", isDocsCollapsed.value.toString());
};

// Toggle tickets section
const toggleTickets = () => {
    isTicketsCollapsed.value = !isTicketsCollapsed.value;
    localStorage.setItem(
        "ticketsCollapsed",
        isTicketsCollapsed.value.toString(),
    );
};

// Check screen size and set navbar state accordingly
const checkScreenSize = () => {
    const width = window.innerWidth;
    const height = window.innerHeight;
    const previousMobile = isMobile.value;
    const previousTablet = isTablet.value;
    const previousDesktop = isDesktop.value;

    // Determine current screen size category
    isMobile.value = width < 640; // sm breakpoint (phones only)
    isTablet.value = width >= 640 && width < 1024; // sm to lg (tablets and landscape phones)
    isDesktop.value = width >= 1024; // lg and above

    // Check if viewport height is small (compact nav mode)
    isCompactNav.value = height < 750;

    // Get stored user preference
    const storedPref = localStorage.getItem("navbarCollapsed");

    // Only update collapsed state when transitioning between size categories
    if (isMobile.value && !previousMobile) {
        // Just became mobile: hide sidebar
        isCollapsed.value = true;
    } else if (isTablet.value && !previousTablet) {
        // Just became tablet: always collapse
        isCollapsed.value = true;
    } else if (isDesktop.value && !previousDesktop) {
        // Just became desktop: respect user preference (default expanded)
        isCollapsed.value = storedPref === "true";
    }

    // Emit the current state
    emit("update:collapsed", isCollapsed.value);
};

// Initialize on mount
onMounted(() => {
    // Load stored preferences
    const storedPref = localStorage.getItem("navbarCollapsed");
    const storedDocsCollapsed = localStorage.getItem("docsCollapsed");
    const storedTicketsCollapsed = localStorage.getItem("ticketsCollapsed");

    // Determine initial screen size
    const width = window.innerWidth;
    const height = window.innerHeight;
    isMobile.value = width < 640;
    isTablet.value = width >= 640 && width < 1024;
    isDesktop.value = width >= 1024;
    isCompactNav.value = height < 750;

    // Set initial collapsed state based on screen size
    if (isMobile.value) {
        // Mobile: hidden (bottom nav shows)
        isCollapsed.value = true;
    } else if (isTablet.value) {
        // Tablet: always collapsed
        isCollapsed.value = true;
    } else {
        // Desktop: respect user preference (default expanded)
        isCollapsed.value = storedPref === "true";
    }

    // Set sections collapsed state from localStorage
    isDocsCollapsed.value = storedDocsCollapsed === "true";
    isTicketsCollapsed.value = storedTicketsCollapsed === "true";

    // Emit initial state
    emit("update:collapsed", isCollapsed.value);

    // Add resize listener for screen size changes
    window.addEventListener("resize", checkScreenSize);

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
    window.removeEventListener("resize", checkScreenSize);
    // Global listeners for resizing are handled by the composable's onBeforeUnmount
});

// Computed property to check if we're on a documentation page
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

// Navigation links data
const navLinks = [
    {
        to: "/",
        icon: "M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6",
        text: "Dashboard",
        exact: true,
        color: "#FDBD10",
    },
    {
        to: "/tickets",
        icon: "M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-6 9l2 2 4-4",
        text: "Tickets",
        color: "#2C80FF",
    },
    {
        to: "/projects",
        icon: "M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z",
        text: "Projects",
        color: "#00C951",
    },
    {
        to: "/users",
        icon: "M12 4.354a4 4 0 110 5.292M15 21H3v-1a6 6 0 0112 0v1zm0 0h6v-1a6 6 0 00-9-5.197M13 7a4 4 0 11-8 0 4 4 0 018 0z",
        text: "Users",
        color: "#FF66B3",
    },
    {
        to: "/devices",
        icon: "M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z",
        text: "Devices",
        color: "#99A1AF",
    },
    {
        to: "/documentation",
        icon: "M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253",
        text: "Documentation",
        color: "#8B5CF6",
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
                class="flex items-center justify-center h-12 mb-5 hover:opacity-80 transition-opacity select-none"
            >
                <!-- Full logo when expanded -->
                <img
                    v-if="!isCollapsed && logoUrl"
                    :alt="brandingStore.appName + ' Logo'"
                    class="h-8 max-w-full object-contain"
                    :src="logoUrl"
                />
                <img
                    v-else-if="!isCollapsed"
                    alt="Nosdesk Logo"
                    class="h-8"
                    src="@/assets/logo.svg"
                />
                <!-- Favicon/icon when collapsed -->
                <img v-else :alt="brandingStore.appName" class="h-8 w-8 object-contain" :src="faviconUrl" />
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
                    <!-- Active indicator bar -->
                    <div
                        v-if="isRouteActive(link.to, link.exact)"
                        class="absolute left-0 top-0 bottom-0 w-1"
                        :class="{ 'w-full h-0.5 top-auto': isCompactNav && !isCollapsed }"
                        :style="{ backgroundColor: link.color }"
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
                    accent-color="#2C80FF"
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
                    accent-color="#8B5CF6"
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
                @click="toggleNav"
                class="w-full h-8 px-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-md transition-colors group flex items-center justify-center"
                aria-label="Toggle sidebar"
            >
                <svg
                    xmlns="http://www.w3.org/2000/svg"
                    class="h-3.5 w-3.5 group-hover:text-brand-gold transition-colors"
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
                    isRouteActive(link.to, link.exact) ? '' : 'text-secondary'
                "
                :style="
                    isRouteActive(link.to, link.exact)
                        ? { color: link.color }
                        : {}
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
    height: 4px;
    margin: 0;
    cursor: ns-resize;
    background-color: var(--color-border-subtle);
    border-top: 1px solid var(--color-border-default);
    border-bottom: 1px solid var(--color-border-default);
}

.resizer-handle:hover {
    background-color: var(--color-border-default);
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
