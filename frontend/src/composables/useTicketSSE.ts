import { ref, onMounted, onUnmounted, type Ref } from "vue";
import { useSSE } from "@/services/sseService";
import { useAuthStore } from "@/stores/auth";
import { useTitleManager } from "@/composables/useTitleManager";
import { useRecentTicketsStore } from "@/stores/recentTickets";
import { useTicketMutations } from "@/composables/useTicketMutations";
import * as deviceService from "@/services/deviceService";
import type { TicketStatus, TicketPriority } from "@/constants/ticketOptions";
import type { Ticket } from "@/types/ticket";
import type { CommentWithAttachments } from "@/types/comment";
import {
  unwrapEventData,
  type TicketUpdatedEventData,
  type CommentAddedEventData,
  type CommentDeletedEventData,
  type DeviceLinkEventData,
  type DeviceUpdatedEventData,
  type TicketLinkEventData,
  type ProjectEventData,
  type ViewerCountEventData,
} from "@/types/sse";

// Enable debug logging only in development
const DEBUG_SSE = import.meta.env.DEV && import.meta.env.VITE_DEBUG_SSE === 'true';

/**
 * Extended ticket type for detail view with UI-specific fields
 */
interface TicketWithDetails extends Ticket {
  commentsAndAttachments?: CommentWithAttachments[];
}

/**
 * Composable for handling SSE events for tickets
 *
 * Uses centralized mutations from useTicketMutations for consistent
 * array operations with built-in deduplication.
 */
export function useTicketSSE(
  ticket: Ref<TicketWithDetails | null>,
  ticketId: Ref<number | undefined>,
  selectedStatus: Ref<TicketStatus>,
  selectedPriority: Ref<TicketPriority>,
) {
  const {
    addEventListener,
    removeEventListener,
    isConnected,
    connect,
    disconnect,
  } = useSSE();

  const authStore = useAuthStore();
  const titleManager = useTitleManager();
  const recentTicketsStore = useRecentTicketsStore();
  const mutations = useTicketMutations(ticket);

  const recentlyAddedCommentIds = ref<Set<number>>(new Set());
  const activeViewerCount = ref<number>(0);

  // Track fields currently being edited locally (skip SSE updates for these)
  const editingFields = ref<Set<string>>(new Set());

  // Track pending updates with timestamps to handle out-of-order delivery
  const pendingUpdates = ref<Map<string, number>>(new Map());

  /**
   * Mark a field as being edited locally.
   * While editing, SSE updates for this field will be skipped to prevent conflicts.
   */
  function startEditing(field: string): void {
    editingFields.value.add(field);
  }

  /**
   * Mark a field as no longer being edited.
   * Also records a timestamp to ignore any stale SSE updates still in flight.
   */
  function stopEditing(field: string): void {
    editingFields.value.delete(field);
    // Record when we stopped editing to ignore stale updates for a short window
    pendingUpdates.value.set(field, Date.now());
    // Clean up after 2 seconds (enough time for any in-flight updates to arrive)
    setTimeout(() => {
      pendingUpdates.value.delete(field);
    }, 2000);
  }

  /**
   * Check if an SSE update should be applied.
   * Skips updates from current user or for fields being edited.
   */
  function shouldApplyUpdate(field: string, updatedBy?: string): boolean {
    // Skip if this field is currently being edited locally
    if (editingFields.value.has(field)) {
      if (DEBUG_SSE) console.log(`[SSE] Skipping ${field} update - field is being edited`);
      return false;
    }

    // Skip updates from current user (we already have them via optimistic update)
    const currentUserUuid = authStore.user?.uuid;
    if (currentUserUuid && updatedBy === currentUserUuid) {
      if (DEBUG_SSE) console.log(`[SSE] Skipping ${field} update - from current user`);
      return false;
    }

    // Skip if we recently stopped editing this field (stale update protection)
    const pendingTime = pendingUpdates.value.get(field);
    if (pendingTime && Date.now() - pendingTime < 1000) {
      if (DEBUG_SSE) console.log(`[SSE] Skipping ${field} update - recent local edit`);
      return false;
    }

    return true;
  }

  // Highlight comment
  function highlightComment(commentId: number): void {
    recentlyAddedCommentIds.value.add(commentId);
    setTimeout(() => {
      recentlyAddedCommentIds.value.delete(commentId);
    }, 3000);
  }

  // Handle ticket updated
  function handleTicketUpdated(eventData: unknown): void {
    const data = unwrapEventData(eventData as TicketUpdatedEventData);
    if (!ticket.value || data.ticket_id !== ticket.value.id) return;

    if (DEBUG_SSE) {
      console.log('[SSE] ticket-updated:', data.field, data.value, 'by:', data.updated_by);
    }

    // Check if we should apply this update
    if (!shouldApplyUpdate(data.field, data.updated_by)) {
      return;
    }

    // Use direct mutation to preserve object reference - prevents component remounts
    if (data.field === "title" && typeof data.value === "string") {
      ticket.value.title = data.value;
      titleManager.setTicket(ticket.value);
    } else if (data.field === "status") {
      const statusValue = data.value as TicketStatus;
      ticket.value.status = statusValue;
      selectedStatus.value = statusValue;
    } else if (data.field === "priority") {
      const priorityValue = data.value as TicketPriority;
      ticket.value.priority = priorityValue;
      selectedPriority.value = priorityValue;
    } else if (data.field === "modified" && typeof data.value === "string") {
      ticket.value.modified = data.value;
    } else if (data.field === "requester") {
      if (typeof data.value === "string") {
        ticket.value.requester = data.value || "";
        if (!data.value) {
          ticket.value.requester_user = null;
        }
      } else if (typeof data.value === "object" && data.value && "uuid" in data.value) {
        ticket.value.requester = data.value.uuid;
        ticket.value.requester_user = data.value.user_info || ticket.value.requester_user;
      }
    } else if (data.field === "assignee") {
      if (typeof data.value === "string") {
        ticket.value.assignee = data.value || "";
        if (!data.value) {
          ticket.value.assignee_user = null;
        }
      } else if (typeof data.value === "object" && data.value && "uuid" in data.value) {
        ticket.value.assignee = data.value.uuid;
        ticket.value.assignee_user = data.value.user_info || ticket.value.assignee_user;
      }
    }

    recentTicketsStore.updateTicketData(ticket.value.id, {
      title: ticket.value.title,
      status: ticket.value.status,
      requester: ticket.value.requester,
      assignee: ticket.value.assignee,
    });
  }

  // Handle comment added
  function handleCommentAdded(rawData: unknown): void {
    const eventData = unwrapEventData(rawData as CommentAddedEventData);
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) return;

    const commentData = eventData.comment;
    if (!commentData) return;

    // Check for duplicates (will catch optimistic updates)
    if (ticket.value.commentsAndAttachments?.find((c) => c.id === commentData.id)) {
      if (DEBUG_SSE) console.log('[SSE] Skipping duplicate comment', commentData.id);
      return;
    }

    const newComment: CommentWithAttachments = {
      id: commentData.id,
      content: commentData.content,
      user_uuid: commentData.user_uuid || commentData.user_id || "",
      createdAt: commentData.createdAt || commentData.created_at || "",
      created_at: commentData.created_at || commentData.createdAt || "",
      ticket_id: commentData.ticket_id,
      attachments: commentData.attachments || [],
      user: commentData.user,
    };

    // Use direct mutation on the array to preserve object reference
    if (ticket.value.commentsAndAttachments) {
      ticket.value.commentsAndAttachments.unshift(newComment);
    } else {
      ticket.value.commentsAndAttachments = [newComment];
    }

    highlightComment(newComment.id);
  }

  // Handle comment deleted
  function handleCommentDeleted(rawData: unknown): void {
    const eventData = unwrapEventData(rawData as CommentDeletedEventData);
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) return;

    if (ticket.value.commentsAndAttachments) {
      const index = ticket.value.commentsAndAttachments.findIndex(
        (comment) => comment.id === eventData.comment_id
      );
      if (index !== -1) {
        ticket.value.commentsAndAttachments.splice(index, 1);
      }
    }
  }

  // Handle device linked
  async function handleDeviceLinked(rawData: unknown): Promise<void> {
    const eventData = unwrapEventData(rawData as DeviceLinkEventData);
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) return;

    // Skip if already exists (optimistic update already added it)
    if (mutations.hasDevice(eventData.device_id)) {
      if (DEBUG_SSE) console.log('[SSE] device-linked: skipping duplicate', eventData.device_id);
      return;
    }

    try {
      const device = await deviceService.getDeviceById(eventData.device_id);
      if (mutations.addDevice(device)) {
        if (DEBUG_SSE) console.log('[SSE] device-linked:', eventData.device_id);
      }
    } catch (error) {
      console.error("Error fetching linked device:", error);
    }
  }

  // Handle device unlinked
  function handleDeviceUnlinked(rawData: unknown): void {
    const eventData = unwrapEventData(rawData as DeviceLinkEventData);
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) return;

    if (mutations.removeDevice(eventData.device_id)) {
      if (DEBUG_SSE) console.log('[SSE] device-unlinked:', eventData.device_id);
    }
  }

  // Handle device updated
  function handleDeviceUpdated(rawData: unknown): void {
    const eventData = unwrapEventData(rawData as DeviceUpdatedEventData);
    if (!ticket.value?.devices) return;

    if (eventData.field && eventData.value !== undefined) {
      if (mutations.updateDeviceField(eventData.device_id, eventData.field, eventData.value)) {
        if (DEBUG_SSE) console.log('[SSE] device-updated:', eventData.device_id, eventData.field);
      }
    }
  }

  // Handle ticket linked
  function handleTicketLinked(rawData: unknown): void {
    const eventData = unwrapEventData(rawData as TicketLinkEventData);
    if (!ticket.value) return;

    const isSourceTicket = eventData.ticket_id === ticket.value.id;
    const isTargetTicket = eventData.linked_ticket_id === ticket.value.id;

    if (!isSourceTicket && !isTargetTicket) return;

    const linkedTicketId = isSourceTicket
      ? eventData.linked_ticket_id
      : eventData.ticket_id;

    if (mutations.addLinkedTicket(linkedTicketId)) {
      if (DEBUG_SSE) console.log('[SSE] ticket-linked:', linkedTicketId);
    }
  }

  // Handle ticket unlinked
  function handleTicketUnlinked(rawData: unknown): void {
    const eventData = unwrapEventData(rawData as TicketLinkEventData);
    if (!ticket.value) return;

    const isSourceTicket = eventData.ticket_id === ticket.value.id;
    const isTargetTicket = eventData.linked_ticket_id === ticket.value.id;

    if (!isSourceTicket && !isTargetTicket) return;

    const linkedTicketIdToRemove = isSourceTicket
      ? eventData.linked_ticket_id
      : eventData.ticket_id;

    if (mutations.removeLinkedTicket(linkedTicketIdToRemove)) {
      if (DEBUG_SSE) console.log('[SSE] ticket-unlinked:', linkedTicketIdToRemove);
    }
  }

  // Handle project assigned
  function handleProjectAssigned(rawData: unknown): void {
    const eventData = unwrapEventData(rawData as ProjectEventData);
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) return;

    if (mutations.addProject(eventData.project_id)) {
      if (DEBUG_SSE) console.log('[SSE] project-assigned:', eventData.project_id);
    }
  }

  // Handle project unassigned
  function handleProjectUnassigned(rawData: unknown): void {
    const eventData = unwrapEventData(rawData as ProjectEventData);
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) return;

    if (mutations.removeProject(eventData.project_id)) {
      if (DEBUG_SSE) console.log('[SSE] project-unassigned:', eventData.project_id);
    }
  }

  // Handle viewer count changed
  function handleViewerCountChanged(rawData: unknown): void {
    const eventData = unwrapEventData(rawData as ViewerCountEventData);
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) return;

    activeViewerCount.value = eventData.count || 0;
  }

  // SSE event types used by this composable
  type TicketSSEEventType =
    | "ticket-updated"
    | "comment-added"
    | "comment-deleted"
    | "device-linked"
    | "device-unlinked"
    | "device-updated"
    | "ticket-linked"
    | "ticket-unlinked"
    | "project-assigned"
    | "project-unassigned"
    | "viewer-count-changed";

  // Event handler type for SSE events
  type SSEEventHandler = (data: unknown) => void | Promise<void>;

  // Event handler configuration - DRY principle
  const eventHandlers: Record<TicketSSEEventType, SSEEventHandler> = {
    "ticket-updated": handleTicketUpdated,
    "comment-added": handleCommentAdded,
    "comment-deleted": handleCommentDeleted,
    "device-linked": handleDeviceLinked,
    "device-unlinked": handleDeviceUnlinked,
    "device-updated": handleDeviceUpdated,
    "ticket-linked": handleTicketLinked,
    "ticket-unlinked": handleTicketUnlinked,
    "project-assigned": handleProjectAssigned,
    "project-unassigned": handleProjectUnassigned,
    "viewer-count-changed": handleViewerCountChanged,
  };

  // Setup event listeners
  function setupEventListeners(): void {
    if (DEBUG_SSE) console.log('[SSE] Registering event listeners');
    (Object.entries(eventHandlers) as [TicketSSEEventType, SSEEventHandler][]).forEach(
      ([event, handler]) => {
        addEventListener(event, handler);
      }
    );
  }

  // Remove event listeners
  function cleanupEventListeners(): void {
    (Object.entries(eventHandlers) as [TicketSSEEventType, SSEEventHandler][]).forEach(
      ([event, handler]) => {
        removeEventListener(event, handler);
      }
    );
  }

  // Auto-setup on mount - connect immediately for real-time updates
  onMounted(async () => {
    setupEventListeners();
    if (authStore.isAuthenticated && ticketId.value) {
      if (DEBUG_SSE) console.log('[SSE] Connecting for ticket:', ticketId.value);
      await connect(ticketId.value);
    }
  });

  // Auto-cleanup on unmount
  onUnmounted(() => {
    cleanupEventListeners();
    disconnect();
  });

  return {
    isConnected,
    recentlyAddedCommentIds,
    activeViewerCount,
    // Field editing state for SSE update filtering
    startEditing,
    stopEditing,
  };
}
