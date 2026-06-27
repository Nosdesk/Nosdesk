<script setup lang="ts">
/// <reference types="node" />
import { computed, onMounted, onUnmounted, watch, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useFluent } from "fluent-vue";
import { useAuthStore } from "@/stores/auth";
import { PRIORITY_OPTIONS } from "@nosdesk/core/constants/ticketOptions";
import { categoryService } from "@nosdesk/core/services/categoryService";
import type { TicketCategory } from "@nosdesk/core/types/category";
import type { Ticket } from "@nosdesk/core/types/ticket";

// Composables
import { useTicketDetail } from "@/sync/stores/ticketDetail";
import { subscribe } from "@/sync/lifecycle";
import { useTicketUiStore } from "@/stores/ticketUi";
import { useTicketSSE } from "@/composables/useTicketSSE";
import { useTitleManager } from "@/composables/useTitleManager";
import { useTicketDrag, shouldSuppressTicketDrop } from "@/composables/useTicketDrag";
import { parseTicketUrl } from "@/components/editor/ticketLinkPlugin";

// Components
import PresenceStack from "@/components/PresenceStack.vue";
import CollaborativeTicketArticle from "@/components/ticketComponents/CollaborativeTicketArticle.vue";
import TicketDetails from "@/components/ticketComponents/TicketDetails.vue";
import TicketActivity from "@/components/ticketComponents/TicketActivity.vue";
import DeviceSelectionModal from "@/components/ticketComponents/AssetSelectionModal.vue";
import CommentsAndAttachments from "@/components/ticketComponents/CommentsAndAttachments.vue";
import MergedIntoBanner from "@/components/ticketComponents/MergedIntoBanner.vue";
import SpamBanner from "@/components/ticketComponents/SpamBanner.vue";
import MergedInField from "@/components/ticketComponents/MergedInField.vue";
import TicketPickerModal from "@/components/ticketComponents/TicketPickerModal.vue";
import ProjectSelectionModal from "@/components/ticketComponents/ProjectSelectionModal.vue";
import TicketGapFlag from "@/components/ticketComponents/TicketGapFlag.vue";
import TicketLoansCard from "@/components/ticketComponents/TicketLoansCard.vue";
import documentationService from "@nosdesk/core/services/documentationService";
import { docUrl } from "@nosdesk/core/utils/docUrl";
import { pageTicketLinkKeys } from "@/composables/usePageTicketLinks";
import { useFlagTicketMutation } from "@/composables/useKnowledgeGaps";
import { useQueryCache } from "@pinia/colada";
import BackButton from "@/components/common/BackButton.vue";
import Popover from "@/components/common/Popover.vue";
import MenuList, { type MenuItem } from "@/components/common/MenuList.vue";
import Icon from "@/components/common/Icon.vue";
import Modal from "@/components/Modal.vue";
import NotFoundIllustration from "@/components/common/NotFoundIllustration.vue";
import SectionCard from "@/components/common/SectionCard.vue";
import PluginSlot from "@/plugins/components/PluginSlot.vue";
import { getActionRegistrations } from "@/plugins/loader";
import { useCreateTicketAction } from "@/composables/useCreateTicketAction";


const route = useRoute();
const router = useRouter();
const authStore = useAuthStore();
const titleManager = useTitleManager();
const fluent = useFluent();
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args);

// Resolve the registry's `{ value, labelKey }` shape into the
// `{ value, label }` shape TicketDetails expects. Computed so a
// locale flip re-renders the dropdowns without remounting.
const localizedPriorityOptions = computed(() =>
    PRIORITY_OPTIONS.map((opt) => ({ value: opt.value, label: t(opt.labelKey) })),
);

const ticketId = computed(() =>
    route.params.id ? Number(route.params.id) : undefined,
);

// Categories (reference data; resolves the category chip + dropdown).
const categories = ref<TicketCategory[]>([]);
const categoryOptions = computed(() => [
    { value: '', label: t('tickets-category-none') },
    ...categories.value.map(cat => ({
        value: String(cat.id),
        label: cat.name,
        color: cat.color || undefined
    }))
]);

const loadCategories = async () => {
    try {
        categories.value = await categoryService.getCategories();
    } catch (err) {
        console.error('Failed to load categories:', err);
    }
};

// Pool-native ticket detail view-model: reads + optimistic writes all
// flow through the sync object pool (fed by the `ticket:<id>`
// bootstrap + live stream), replacing the REST-fetch + discrete-SSE
// composables (useTicketData / Comments / Relationships / Assets).
const {
    ticket,
    comments,
    devices,
    selectedPriority,
    selectedCategory,
    selectedWorkflowStateId,
    formattedCreatedDate,
    formattedModifiedDate,
    recentlyAddedCommentIds,
    updateWorkflowState,
    updatePriority,
    updateCategory,
    updateRequester,
    updateAssignee,
    updateTitle,
    updateDueDate,
    updateRecurrenceRule,
    updateResolutionNotes,
    updateTags,
    toggleWatch,
    deleteTicket,
    addComment,
    deleteComment,
    deleteAttachment,
    showLinkedTicketModal,
    showProjectModal,
    linkTicket,
    unlinkTicket,
    addToProject,
    removeFromProject,
    showDeviceModal,
    addDevice,
    removeDevice,
    recordView,
} = useTicketDetail(ticketId, categories);

// `error` is set when a subscribed ticket never lands in the pool
// (deleted, or no read access — the bootstrap silently streams
// nothing). Drives the not-found illustration.
const error = ref<string | null>(null);

// Sidebar's bell toggle emits without arguments — the facade needs the
// current user uuid to know whose watch flag to flip. Wrapping here
// keeps that lookup out of the child component.
function handleToggleWatch() {
    const uuid = authStore.user?.uuid;
    if (!uuid) return;
    void toggleWatch(uuid);
}

// Presence + live field preview over the per-ticket SSE topic.
const {
    isConnected,
    otherViewers,
} = useTicketSSE(ticketId);

// Drag-to-link state
const { dragState } = useTicketDrag();
const isLinkDropTarget = ref(false);
let inactiveTimeout: ReturnType<typeof setTimeout> | null = null;

// Show drop zone affordance when dragging a ticket (that isn't this ticket)
const showDropAffordance = computed(() => {
    return dragState.value.isDragging &&
           dragState.value.ticket?.id !== ticket.value?.id;
});

// Unified "+ Add" menu state — plugin activation counters live in
// `useTicketUiStore` keyed by ticket id so they survive component
// unmount (Phase 4 will drop KeepAlive on TicketView; the store is
// the new home for this state). Falls back to an empty map for
// pre-route or transitional states.
const ticketUi = useTicketUiStore();
const pluginActionActivatedMap = computed(() =>
    ticketId.value !== undefined
        ? ticketUi.getPluginActivations(ticketId.value)
        : new Map<string, number>()
);

// Plugins consume the canonical `Ticket` shape. The pool-derived
// view-model is structurally narrower (denormalised workflow_state,
// no heavy email fields), so cast at the slot boundary — plugins read
// the common fields (id, title, status, assignee, ...) which the
// assembled object carries.
const pluginTicket = computed(() => ticket.value as unknown as Ticket);

// Overflow menu state: trigger sits in the page header next to
// the connection-status indicator. Hosts ticket-level actions
// (Flag for documentation, plugin-contributed actions) plus the
// destructive Delete option. Centralising these here keeps the
// sidebar focused on properties and removes the standalone
// red-Delete button from prime real estate.
const overflowMenuOpen = ref(false);
const overflowTriggerRef = ref<HTMLElement | null>(null);
const overflowAnchor = computed(() => ({
    type: 'element' as const,
    element: () => overflowTriggerRef.value,
}));
const showDeleteConfirm = ref(false);

const overflowMenuItems = computed<MenuItem[]>(() => {
    const items: MenuItem[] = [
        {
            id: 'flag-for-docs',
            label: t('tickets-menu-flag-for-docs'),
            icon: 'M3 21v-4m0 0V5a2 2 0 012-2h6.5l1 1H21l-3 6 3 6h-8.5l-1-1H5a2 2 0 00-2 2zm9-13.5V9',
        },
    ];

    const pluginActions = getActionRegistrations('ticket-sidebar');
    pluginActions.forEach((action, idx) => {
        items.push({
            id: `plugin:${action.pluginUuid}:${action.componentName}`,
            label: action.label,
            iconUrl: action.icon,
            trailing: action.componentLabel || action.pluginName,
            // Divider above the first plugin item so plugin
            // contributions read as a distinct group.
            divider: idx === 0,
        });
    });

    items.push({
        id: 'delete',
        label: t('tickets-menu-delete'),
        icon: 'M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z',
        danger: true,
        divider: true,
    });

    return items;
});

const queryCache = useQueryCache();
const flagMutation = useFlagTicketMutation();

/** Mark this ticket as a knowledge gap. Idempotent: the backend
 *  attaches a fresh manual_flag signal to an existing open gap if
 *  one already covers the ticket, otherwise creates a new gap. */
const handleFlagForDocs = async () => {
    if (ticketId.value === undefined) return;
    await flagMutation.mutateAsync({ ticketId: ticketId.value });
};

/**
 * Promote the current ticket to a documentation page. Creates a
 * draft page anchored to this ticket via a 'resolves' link
 * (the backend handler does both in one call), invalidates the
 * ticket's linked-docs query so the sidebar refreshes, and routes
 * the user to the new doc so they can edit straight away.
 */
const handleSaveAsDoc = async () => {
    if (!ticket.value || ticketId.value === undefined) return;
    const titleSeed = ticket.value.title?.trim() || `Ticket #${ticketId.value}`;
    const created = await documentationService.createPageFromTicket(ticketId.value, {
        title: titleSeed,
        icon: '📄',
    });
    if (!created) return;
    queryCache.invalidateQueries({
        key: pageTicketLinkKeys.forTicket(ticketId.value),
    });
    router.push(docUrl({ slug: created.slug, id: created.id as number }));
};

const handleOverflowSelect = (itemId: string) => {
    overflowMenuOpen.value = false;
    if (itemId === 'flag-for-docs') {
        handleFlagForDocs();
    } else if (itemId === 'delete') {
        // Two-step destructive flow: opening the modal is the
        // first click, the modal's confirm button is the second.
        showDeleteConfirm.value = true;
    } else if (itemId.startsWith('plugin:') && ticketId.value !== undefined) {
        const key = itemId.replace('plugin:', '');
        ticketUi.activatePluginAction(ticketId.value, key);
    }
};

const confirmDeleteTicket = () => {
    showDeleteConfirm.value = false;
    deleteTicket();
};

/** Internal-note comments on this ticket, in chronological order.
 *  Threaded into TicketDetails so the Resolution section can offer
 *  a one-click "Draft from notes" affordance that appends them to
 *  the resolution textarea. Reuses the comments ref so no extra
 *  API call is needed. */
const internalComments = computed(() => {
    return (comments.value ?? []).filter((c) => c.is_internal === true);
});

// Check if there are any comments with actual content (for print visibility)
const hasCommentsWithContent = computed(() => {
    if (!comments.value || comments.value.length === 0) return false;
    // Check if any comment has content or attachments
    return comments.value.some(comment =>
        (comment.content && comment.content.trim().length > 0) ||
        (comment.attachments && comment.attachments.length > 0)
    );
});

// Print visibility for the ticket article. The article body lives in
// the collaborative editor (Yjs over WebSocket), not the sync pool, so
// the detail view no longer has a cheap "is it empty?" signal to gate
// the print card on. Always render it; an empty article card on print
// is a minor cost versus pulling the doc body onto the change log.
const hasArticleContent = computed(() => true);

const setDropTargetActive = () => {
    isLinkDropTarget.value = true;
    if (inactiveTimeout) {
        clearTimeout(inactiveTimeout);
        inactiveTimeout = null;
    }
};

const setDropTargetInactive = () => {
    // Debounce to prevent flickering when moving over child elements
    inactiveTimeout = setTimeout(() => {
        isLinkDropTarget.value = false;
    }, 50);
};

const resetDropState = () => {
    isLinkDropTarget.value = false;
    if (inactiveTimeout) {
        clearTimeout(inactiveTimeout);
        inactiveTimeout = null;
    }
};

const handleLinkDrop = async (event: DragEvent) => {
    event.preventDefault();
    resetDropState();

    if (shouldSuppressTicketDrop()) {
        return;
    }

    // Try to get ticket ID from JSON data first
    let droppedTicketId: number | null = null;

    const jsonData = event.dataTransfer?.getData('application/json');
    if (jsonData) {
        try {
            const data = JSON.parse(jsonData);
            if (data.ticketId) {
                droppedTicketId = data.ticketId;
            }
        } catch {
            // Invalid JSON
        }
    }

    // Fallback to URL parsing
    if (!droppedTicketId) {
        const text = event.dataTransfer?.getData('text/plain');
        if (text) {
            droppedTicketId = parseTicketUrl(text.trim());
        }
    }

    if (droppedTicketId && droppedTicketId !== ticket.value?.id) {
        await linkTicket(droppedTicketId);
    }
};

// Document-level dragend cleanup (fires when drag ends anywhere)
const handleDocumentDragEnd = () => {
    resetDropState();
};

onMounted(() => {
    document.addEventListener('dragend', handleDocumentDragEnd);

    // Register save handler for SiteHeader title edits. Optimistic
    // through the pool's push queue (same path as every other field).
    titleManager.onTicketTitleSave(async (title: string) => {
        await updateTitle(title);
    });
});

onUnmounted(() => {
    document.removeEventListener('dragend', handleDocumentDragEnd);
    titleManager.onTicketTitleSave(null);
    if (inactiveTimeout) {
        clearTimeout(inactiveTimeout);
    }
});

// Title editing pipeline. Two channels:
//   - preview: broadcast every typing pause to other viewers via
//     SSE. No DB write, no activity row.
//   - commit:  persist the final value via PATCH, which is what
//     stamps the activity log. Fires on idle (3s of no typing),
//     on blur, or on a hard 8s cap to bound unsaved work.
// Title editing moved to the SiteHeader (commit handler wired
// via `titleManager.onTicketTitleSave` further up). The previous
// sidebar Title field was deleted as part of the sidebar's flat-
// property-panel redesign; if SSE typing-preview ever returns,
// rewire SiteHeader through a `useFieldAutoSave` pipeline here.

// Emit ticket updates - pass the full reactive ticket object
const emit = defineEmits<{
    (e: "update:ticket", ticket: Ticket | null): void;
}>();

watch(
    ticket,
    (newTicket) => {
        // Only emit when valid ticket data exists - prevents title flash during loading
        // Clearing is handled by App.vue on route leave
        if (newTicket) {
            emit("update:ticket", newTicket as unknown as Ticket);
        }
    },
    { immediate: true, deep: true }, // deep: true to watch nested property changes
);

// Load ticket on mount and route change
// Subscribe to the ticket's sync group so the pool bootstraps the
// ticket + its comments / attachments / device + ticket links /
// project memberships, then resolve the not-found case (the bootstrap
// silently streams nothing for a ticket the caller can't read or that
// doesn't exist). Reads come from the pool reactively; live changes
// arrive on the sync stream.
const loadTicket = async (id: number) => {
    error.value = null;
    await subscribe(`ticket:${id}`);
    if (!ticket.value) {
        error.value = t('ticket-data-load-failed');
        return;
    }
    recordView();
};

onMounted(async () => {
    // Reference data loads in parallel with the ticket subscription.
    loadCategories();

    if (ticketId.value !== undefined) {
        await loadTicket(ticketId.value);
    }
});

watch(
    () => ticketId.value,
    async (newId) => {
        if (newId !== undefined) {
            await loadTicket(newId);
        }
    },
);

useCreateTicketAction();
</script>

<template>
    <div class="flex-1">
        <!-- Error state -->
        <div v-if="error" class="flex flex-col items-center justify-center min-h-[calc(100vh-8rem)] px-4 gap-4">
            <NotFoundIllustration />
            <router-link
                to="/tickets"
                class="px-4 py-2 bg-accent text-on-accent rounded-lg hover:bg-accent-hover transition-colors"
            >
                Back to Tickets
            </router-link>
        </div>

        <!-- Ticket content (layout always rendered; skeletons swap to real components) -->
        <div v-else class="flex flex-col">
            <!-- Navigation and actions bar (hidden on print) -->
            <div class="print:hidden pt-4 px-4 sm:px-6 flex justify-between items-center">
                <div class="flex items-center gap-4">
                    <template v-if="ticket">
                        <BackButton fallbackRoute="/tickets" />

                        <!-- Presence + connection status. In the steady
                             case (connected, no other viewers) this
                             collapses to empty space: nothing to say,
                             so don't say anything. The yellow dot
                             only surfaces during a degraded
                             connection; PresenceStack only renders
                             when at least one other viewer is here. -->
                        <div class="flex items-center gap-3 text-sm">
                            <div
                                v-if="!isConnected"
                                class="flex items-center gap-2"
                                :title="$t('ticket-detail-reconnecting-title')"
                            >
                                <div class="w-2 h-2 rounded-full bg-status-warning animate-pulse"></div>
                                <span class="text-secondary">{{ $t('ticket-detail-connecting') }}</span>
                            </div>
                            <PresenceStack :viewers="otherViewers" />
                        </div>
                    </template>
                    <!-- Skeleton back button -->
                    <div v-else class="h-8 w-24 bg-surface-alt rounded-lg animate-pulse"></div>
                </div>

                <button
                    v-if="ticket"
                    ref="overflowTriggerRef"
                    type="button"
                    class="inline-flex items-center justify-center w-8 h-8 rounded-md text-tertiary hover:text-primary hover:bg-surface-hover transition-colors cursor-pointer"
                    :aria-expanded="overflowMenuOpen"
                    aria-haspopup="menu"
                    :title="$t('ticket-detail-more-actions')"
                    :aria-label="$t('ticket-detail-more-actions')"
                    @click="overflowMenuOpen = !overflowMenuOpen"
                >
                    <Icon name="more" class="w-5 h-5" />
                </button>

                <Popover
                    :open="overflowMenuOpen"
                    :anchor="overflowAnchor"
                    placement="bottom-end"
                    react-to-scroll="reposition"
                    :auto-focus="false"
                    role="menu"
                    popover-class="bg-surface border border-default rounded-lg shadow-lg py-1 min-w-[224px]"
                    @close="overflowMenuOpen = false"
                >
                    <MenuList :items="overflowMenuItems" @select="handleOverflowSelect" />
                </Popover>
            </div>

            <div class="flex flex-col gap-4 px-4 py-4 sm:px-6 mx-auto w-full max-w-8xl">
                <!-- Grid Container with named areas -->
                <div class="ticket-grid items-start">
                    <!-- Narrow column: metadata + sidebar sections. -->
                    <div class="ticket-details-column">
                        <!-- Details Sidebar -->
                        <div class="ticket-details flex flex-col gap-3">

                        <!-- Skeleton: Details -->
                        <SectionCard v-if="!ticket" content-padding="p-3">
                            <template #title>{{ $t('ticket-detail-section-details') }}</template>
                            <!-- Content (matches TicketDetails inner layout) -->
                            <div class="flex flex-col gap-3">
                                <!-- Title -->
                                <div class="flex flex-col gap-1.5">
                                    <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ $t('ticket-detail-prop-title') }}</h3>
                                    <div class="bg-surface-alt rounded-lg border border-subtle min-h-[1.75rem] px-2 py-1">
                                        <div class="h-4 w-3/4 bg-surface-hover rounded animate-pulse"></div>
                                    </div>
                                </div>
                                <!-- Requester / Assignee -->
                                <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                                    <div class="flex flex-col gap-1.5">
                                        <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ $t('ticket-detail-prop-requester') }}</h3>
                                        <div class="bg-surface-alt rounded-lg border border-subtle min-h-[44px] sm:min-h-[40px] flex items-center px-2.5 sm:px-3">
                                            <div class="w-7 h-7 sm:w-6 sm:h-6 rounded-full bg-surface-hover animate-pulse shrink-0"></div>
                                            <div class="h-4 w-20 bg-surface-hover rounded animate-pulse ml-2"></div>
                                        </div>
                                    </div>
                                    <div class="flex flex-col gap-1.5">
                                        <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ $t('ticket-detail-prop-assignee') }}</h3>
                                        <div class="bg-surface-alt rounded-lg border border-subtle min-h-[44px] sm:min-h-[40px] flex items-center px-2.5 sm:px-3">
                                            <div class="w-7 h-7 sm:w-6 sm:h-6 rounded-full bg-surface-hover animate-pulse shrink-0"></div>
                                            <div class="h-4 w-20 bg-surface-hover rounded animate-pulse ml-2"></div>
                                        </div>
                                    </div>
                                </div>
                                <!-- Status / Priority -->
                                <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                                    <div class="flex flex-col gap-1.5">
                                        <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ $t('ticket-detail-prop-status') }}</h3>
                                        <div class="bg-surface-alt rounded-lg border border-subtle min-h-[44px] sm:min-h-[40px] flex items-center px-3">
                                            <div class="h-4 w-16 bg-surface-hover rounded animate-pulse"></div>
                                        </div>
                                    </div>
                                    <div class="flex flex-col gap-1.5">
                                        <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ $t('ticket-detail-prop-priority') }}</h3>
                                        <div class="bg-surface-alt rounded-lg border border-subtle min-h-[44px] sm:min-h-[40px] flex items-center px-3">
                                            <div class="h-4 w-16 bg-surface-hover rounded animate-pulse"></div>
                                        </div>
                                    </div>
                                </div>
                                <!-- Category -->
                                <div class="flex flex-col gap-1.5">
                                    <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ $t('ticket-detail-prop-category') }}</h3>
                                    <div class="bg-surface-alt rounded-lg border border-subtle min-h-[44px] sm:min-h-[40px] flex items-center px-3">
                                        <div class="h-4 w-24 bg-surface-hover rounded animate-pulse"></div>
                                    </div>
                                </div>
                                <!-- Timestamps -->
                                <div class="pt-2 border-t border-default">
                                    <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                                        <div class="flex flex-col gap-1">
                                            <span class="text-xs text-tertiary uppercase tracking-wide font-medium">{{ $t('ticket-detail-prop-created') }}</span>
                                            <div class="h-5 w-28 bg-surface-hover rounded animate-pulse"></div>
                                        </div>
                                        <div class="flex flex-col gap-1">
                                            <span class="text-xs text-tertiary uppercase tracking-wide font-medium">{{ $t('ticket-detail-prop-last-modified') }}</span>
                                            <div class="h-5 w-28 bg-surface-hover rounded animate-pulse"></div>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </SectionCard>

                        <!-- Drag-to-link wrapper. Drop events bubble from the
                             Linked Tickets row inside TicketDetails — wrapping
                             the whole sidebar means the user can release
                             anywhere in the sidebar and still link the
                             ticket, which matches the existing affordance. -->
                        <div
                            v-else
                            @dragenter.prevent="setDropTargetActive"
                            @dragover.prevent="setDropTargetActive"
                            @dragleave.prevent="setDropTargetInactive"
                            @drop.prevent="handleLinkDrop"
                            class="contents"
                        >
                            <MergedIntoBanner
                                v-if="ticket.merged_into_ticket_id != null"
                                :target-id="ticket.merged_into_ticket_id"
                                :actor="ticket.merged_by_user_uuid"
                                :when="ticket.merged_at"
                            />
                            <SpamBanner
                                v-if="ticket.spam_suspected"
                                :ticket-id="ticket.id"
                                @delete="showDeleteConfirm = true"
                            />
                            <MergedInField v-if="ticket" :ticket-id="ticket.id" />
                            <TicketDetails
                                :ticket="ticket"
                                :created-date="formattedCreatedDate"
                                :modified-date="formattedModifiedDate"
                                :selected-priority="selectedPriority"
                                :selected-category="selectedCategory"
                                :selected-workflow-state-id="selectedWorkflowStateId"
                                :priority-options="localizedPriorityOptions"
                                :category-options="categoryOptions"
                                :devices="devices"
                                :show-link-drop-affordance="showDropAffordance"
                                :is-link-drop-target="isLinkDropTarget"
                                :link-drop-drag-label="dragState.ticket ? `#${dragState.ticket.id} ${dragState.ticket.title}` : null"
                                :internal-comments="internalComments"
                                @update:selectedWorkflowStateId="updateWorkflowState"
                                @update:selectedPriority="updatePriority"
                                @update:selectedCategory="updateCategory"
                                @update:requester="updateRequester"
                                @update:assignee="updateAssignee"
                                @update:dueDate="updateDueDate"
                                @update:recurrenceRule="updateRecurrenceRule"
                                @update:resolutionNotes="updateResolutionNotes"
                                @update:tag-ids="updateTags"
                                @toggle-watch="handleToggleWatch"
                                @add-device="showDeviceModal = true"
                                @remove-device="removeDevice"
                                @add-linked-ticket="showLinkedTicketModal = true"
                                @remove-linked-ticket="unlinkTicket"
                                @add-project="showProjectModal = true"
                                @remove-project="removeFromProject"
                                @save-as-doc="handleSaveAsDoc"
                            />
                        </div>

                        <!-- Sidebar surfaces that aren't part of the
                             property list: knowledge-gap pill plus
                             the plugin component slot. Page-level
                             actions live in the header overflow menu. -->
                        <template v-if="ticket">
                            <TicketGapFlag :ticket-id="ticket.id" />

                            <TicketLoansCard :ticket-id="ticket.id" :requester-uuid="ticket.requester" :has-devices="devices.length > 0" />

                            <PluginSlot slot-name="ticket-sidebar" :ticket="pluginTicket" :actionActivatedMap="pluginActionActivatedMap" />
                        </template>

                        </div>
                        <!-- Activity timeline (sync_actions event log: status,
                             assignee, priority, category changes + comments).
                             One instance, kept as a sibling of the details card
                             in the sidebar track so it sits under the metadata
                             on tablet/desktop. On mobile the columns flatten
                             (display:contents) and `order` drops it to the very
                             bottom, below the conversation — pure CSS, so
                             resizing never remounts it or refetches. Hidden on
                             print. -->
                        <div v-if="ticket" class="ticket-activity print:hidden">
                            <TicketActivity :ticket-id="ticket.id" />
                        </div>
                    </div>

                    <!--
                      Wide content column: ticket article on top, then
                      the comment timeline. The article is the original
                      message a customer sent (when the ticket was
                      created from email) or the running notes for the
                      ticket; visually it's the first entry in the
                      conversation.
                    -->
                    <div class="ticket-content-column">
                        <!-- Article -->
                        <!-- Skeleton: Article (matches CollaborativeTicketArticle) -->
                        <div v-if="!ticket" class="ticket-article rounded-xl print:hidden">
                            <SectionCard content-padding="p-4">
                                <template #title>{{ $t('ticket-detail-section-notes') }}</template>
                                <template #headerActions>
                                    <div class="w-5 h-5 rounded bg-surface-hover animate-pulse"></div>
                                    <div class="w-5 h-5 rounded bg-surface-hover animate-pulse"></div>
                                    <div class="w-5 h-5 rounded bg-surface-hover animate-pulse"></div>
                                </template>
                                <!-- Content area (matches min-h-[300px]) -->
                                <div class="flex-grow min-h-[300px] flex flex-col gap-3">
                                    <div class="h-4 w-full bg-surface-hover rounded animate-pulse"></div>
                                    <div class="h-4 w-5/6 bg-surface-hover rounded animate-pulse"></div>
                                    <div class="h-4 w-full bg-surface-hover rounded animate-pulse"></div>
                                    <div class="h-4 w-4/6 bg-surface-hover rounded animate-pulse"></div>
                                    <div class="h-4 w-0"></div>
                                    <div class="h-4 w-full bg-surface-hover rounded animate-pulse"></div>
                                    <div class="h-4 w-3/4 bg-surface-hover rounded animate-pulse"></div>
                                    <div class="h-4 w-full bg-surface-hover rounded animate-pulse"></div>
                                    <div class="h-4 w-2/3 bg-surface-hover rounded animate-pulse"></div>
                                </div>
                            </SectionCard>
                        </div>
                        <div
                            v-else
                            class="ticket-article rounded-xl"
                            :class="{ 'print:hidden': !hasArticleContent }"
                        >
                            <CollaborativeTicketArticle
                                :key="`article-${ticket.id}`"
                                :initial-content="''"
                                :ticket-id="ticket.id"
                            />
                        </div>

                        <!-- Comments timeline -->
                        <!-- Skeleton: Comments (matches CommentsAndAttachments / SectionCard) -->
                        <div v-if="!ticket" class="ticket-comments rounded-xl print:hidden">
                            <SectionCard content-padding="p-3">
                                <template #title>{{ $t('ticket-detail-section-comments') }}</template>
                                <!-- Content -->
                                <div class="flex flex-col gap-3">
                                    <!-- Comment input (matches SimpleEditor min-height: 60px) -->
                                    <div class="bg-surface rounded-lg" style="min-height: 60px;">
                                        <div class="p-3">
                                            <div class="h-4 w-48 bg-surface-hover rounded animate-pulse"></div>
                                        </div>
                                    </div>
                                    <!-- Button row (matches Add + voice + file buttons) -->
                                    <div class="flex gap-2">
                                        <div class="flex-1 h-10 bg-surface-hover rounded-md animate-pulse"></div>
                                        <div class="h-10 w-11 bg-surface-alt border border-default rounded-md animate-pulse"></div>
                                        <div class="h-10 w-11 bg-surface-alt border border-default rounded-md animate-pulse"></div>
                                    </div>
                                </div>
                            </SectionCard>
                        </div>
                        <!-- Hidden on print if no comments exist -->
                        <div
                            v-else
                            class="ticket-comments rounded-xl"
                            :class="{ 'print:hidden': !hasCommentsWithContent }"
                        >
                            <CommentsAndAttachments
                                :ticket-id="ticketId"
                                :comments="comments"
                                :current-user="
                                    authStore.user?.uuid || 'Unknown User'
                                "
                                :recently-added-comment-ids="
                                    recentlyAddedCommentIds
                                "
                                :readonly="ticket.merged_into_ticket_id != null"
                                @add-comment="addComment"
                                @delete-attachment="deleteAttachment"
                                @delete-comment="deleteComment"
                            />
                        </div>
                    </div>
                </div>
            </div>
        </div>

        <!-- Modals (hidden on print) -->
        <div class="print:hidden">
            <DeviceSelectionModal
                v-if="ticket"
                :show="showDeviceModal"
                :current-ticket-id="ticket.id"
                :existing-device-ids="devices.map((d) => d.id)"
                :requester-uuid="ticket.requester"
                @close="showDeviceModal = false"
                @select-device="addDevice"
            />

            <TicketPickerModal
                v-if="ticket"
                :show="showLinkedTicketModal"
                :exclude-ids="[ticket.id, ...ticket.linkedTickets]"
                @close="showLinkedTicketModal = false"
                @select="(t) => linkTicket(t.id)"
            />

            <ProjectSelectionModal
                v-if="ticket"
                :show="showProjectModal"
                :existing-project-ids="
                    ticket.projects?.map(id => Number(id)) || []
                "
                @close="showProjectModal = false"
                @select-project="addToProject"
            />

            <!-- Delete confirmation. Triggered from the page-header
                 overflow menu; two-step flow keeps a destructive
                 action behind an explicit confirm gesture. -->
            <Modal
                :show="showDeleteConfirm"
                :title="$t('ticket-detail-delete-title')"
                @close="showDeleteConfirm = false"
            >
                <div class="flex flex-col items-center gap-4">
                    <div
                        class="mx-auto flex items-center justify-center h-12 w-12 rounded-full bg-status-error/20 mb-4"
                    >
                        <svg
                            class="h-6 w-6 text-status-error"
                            fill="none"
                            viewBox="0 0 24 24"
                            stroke="currentColor"
                        >
                            <path
                                stroke-linecap="round"
                                stroke-linejoin="round"
                                stroke-width="2"
                                d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"
                            />
                        </svg>
                    </div>
                    <h3 class="text-2xl font-medium text-primary mb-2">{{ $t('ticket-detail-delete-confirm-heading') }}</h3>
                    <p class="text-base text-secondary mb-6">
                        {{ $t('ticket-detail-delete-confirm-body') }}
                    </p>
                    <div class="flex justify-center gap-4">
                        <button
                            type="button"
                            class="px-4 py-2 bg-surface text-primary rounded-lg hover:bg-surface-hover transition-colors"
                            @click="showDeleteConfirm = false"
                        >
                            {{ $t('ticket-detail-delete-cancel') }}
                        </button>
                        <button
                            type="button"
                            class="px-4 py-2 bg-status-error text-white rounded-lg hover:opacity-90 transition-colors"
                            @click="confirmDeleteTicket"
                        >
                            {{ $t('ticket-detail-delete-confirm') }}
                        </button>
                    </div>
                </div>
            </Modal>
        </div>
    </div>
</template>

<style scoped>
/*
 * Three breakpoints, three layouts:
 *
 *   Mobile (<1024px)       — single column, details quick-scan first.
 *   Tablet  (1024-1535px)  — two columns: details left, conversation
 *                            (article + comments folded) right wide.
 *   Desktop (≥1536px)      — three columns: details | article | comments
 *                            with comments taking the wide track. The
 *                            content-column wrapper dissolves via
 *                            `display: contents` so article and comments
 *                            become first-class grid items again.
 *
 * At every breakpoint the conversation gets the widest available
 * track. The earlier 3-column layout split it across two narrow
 * columns; this one keeps the metadata slim on the left and lets
 * the content stretch.
 */
.ticket-grid {
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
    width: 100%;
}

/* Mobile: dissolve the column wrappers so the details card, the
 * conversation, and the activity log become one flat flow we can order,
 * with the activity pinned to the very bottom. The activity is a single
 * instance that simply reorders here — no teleport, no remount, so
 * resizing the viewport never refetches it. */
.ticket-details-column,
.ticket-content-column {
    display: contents;
}

.ticket-details,
.ticket-article,
.ticket-comments,
.ticket-activity {
    width: 100%;
    min-width: 0;
}

.ticket-details  { order: 1; }
.ticket-article  { order: 2; }
.ticket-comments { order: 3; }
.ticket-activity { order: 4; }

/* Tablet (lg): 2 columns — details (with the activity beneath it) on
 * the left, conversation on the right. The column wrappers come back as
 * real flex columns; the order values above still apply within each, so
 * the activity stays under the metadata. */
@media (min-width: 1024px) {
    .ticket-grid {
        flex-direction: row;
        align-items: flex-start;
    }

    /* Narrow sidebar column on the left. Fixed-width, not a
     * fraction — metadata fields look cramped when scaled wide, and
     * pinning the width keeps the content column's growth
     * predictable. */
    .ticket-details-column {
        display: flex;
        flex-direction: column;
        gap: 1.5rem;
        flex: 0 0 360px;
        max-width: 360px;
        min-width: 320px;
    }

    /* Wide content column on the right, takes whatever's left after
     * the sidebar. `min-width: 0` so children that overflow (an
     * email body with a wide table, a `<pre>` with a long line)
     * scroll inside their container instead of pushing the column. */
    .ticket-content-column {
        display: flex;
        flex-direction: column;
        gap: 1.5rem;
        flex: 1 1 0;
        min-width: 0;
    }
}

/* Desktop (xl): 3 columns — details | article | comments. Only the
 * conversation wrapper dissolves via `display: contents` so the article
 * and comments get their own tracks. The details wrapper stays a real
 * column in track 1, so the activity log remains glued beneath the
 * metadata rather than dropping to the bottom of a tall shared row. */
@media (min-width: 1536px) {
    .ticket-grid {
        display: grid;
        grid-template-columns: 360px minmax(420px, 1fr) minmax(0, 1.5fr);
        gap: 1.5rem;
    }

    .ticket-details-column {
        grid-column: 1;
        max-width: none;
    }

    .ticket-content-column {
        display: contents;
    }

    .ticket-article {
        grid-column: 2;
        grid-row: 1;
        width: auto;
        min-width: 0;
    }

    .ticket-comments {
        grid-column: 3;
        grid-row: 1;
        width: auto;
        min-width: 0;
    }
}
</style>
