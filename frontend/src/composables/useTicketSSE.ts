import { ref, onMounted, onUnmounted, type Ref } from "vue";
import { useSSE } from "@/services/sseService";
import { useAuthStore } from "@/stores/auth";
import { useTitleManager } from "@/composables/useTitleManager";
import { useRecentTicketsStore } from "@/stores/recentTickets";
import * as deviceService from "@/services/deviceService";
import type { TicketStatus, TicketPriority } from "@/constants/ticketOptions";
import type { Ticket, Device, Comment } from "@/types/ticket";
import type { CommentWithAttachments } from "@/types/comment";
import type {
  TicketUpdatedEventData,
  CommentAddedEventData,
  CommentDeletedEventData,
  DeviceLinkEventData,
  DeviceUpdatedEventData,
  TicketLinkEventData,
  ProjectEventData,
  ViewerCountEventData,
} from "@/types/sse";

/**
 * Extended ticket type for detail view with UI-specific fields
 */
interface TicketWithDetails extends Ticket {
  commentsAndAttachments?: CommentWithAttachments[];
}

/**
 * SSE event wrapper - events may come wrapped or direct
 */
interface SSEEventWrapper<T> {
  data?: T;
}

/**
 * Unwrap SSE event data (handles both wrapped and direct formats)
 */
function unwrapEvent<T>(data: T | SSEEventWrapper<T>): T {
  if (data && typeof data === 'object' && 'data' in data && data.data !== undefined) {
    return data.data as T;
  }
  return data as T;
}

/**
 * Composable for handling SSE events for tickets
 * Uses direct mutation to preserve object references and prevent component remounts
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

  const recentlyAddedCommentIds = ref<Set<number>>(new Set());
  const activeViewerCount = ref<number>(0);

  // Highlight comment
  function highlightComment(commentId: number): void {
    recentlyAddedCommentIds.value.add(commentId);
    setTimeout(() => {
      recentlyAddedCommentIds.value.delete(commentId);
    }, 3000);
  }

  // Handle ticket updated
  function handleTicketUpdated(eventData: TicketUpdatedEventData | SSEEventWrapper<TicketUpdatedEventData>): void {
    console.log(
      "%c[SSE Handler] ticket-updated received",
      "color: #f97316; font-weight: bold",
      { rawData: eventData, timestamp: new Date().toISOString() }
    );

    const data = unwrapEvent(eventData);
    if (!ticket.value || data.ticket_id !== ticket.value.id) {
      console.log("[SSE Handler] ticket-updated: Not for this ticket, ignoring", {
        currentTicketId: ticket.value?.id,
        eventTicketId: data.ticket_id,
      });
      return;
    }

    console.log("[SSE Handler] ticket-updated: Processing field update", {
      field: data.field,
      value: data.value,
      updatedBy: data.updated_by,
    });

    // Use direct mutation to preserve object reference - prevents component remounts
    if (data.field === "title" && typeof data.value === "string") {
      ticket.value.title = data.value;
      titleManager.setTicket(ticket.value);
      console.log("%c[SSE Handler] ✅ ticket-updated: Title updated", "color: #22c55e; font-weight: bold");
    } else if (data.field === "status") {
      const statusValue = data.value as TicketStatus;
      ticket.value.status = statusValue;
      selectedStatus.value = statusValue;
      console.log("%c[SSE Handler] ✅ ticket-updated: Status updated", "color: #22c55e; font-weight: bold");
    } else if (data.field === "priority") {
      const priorityValue = data.value as TicketPriority;
      ticket.value.priority = priorityValue;
      selectedPriority.value = priorityValue;
      console.log("%c[SSE Handler] ✅ ticket-updated: Priority updated", "color: #22c55e; font-weight: bold");
    } else if (data.field === "modified" && typeof data.value === "string") {
      ticket.value.modified = data.value;
      console.log("%c[SSE Handler] ✅ ticket-updated: Modified timestamp updated", "color: #22c55e; font-weight: bold");
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
      console.log("%c[SSE Handler] ✅ ticket-updated: Requester updated", "color: #22c55e; font-weight: bold");
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
      console.log("%c[SSE Handler] ✅ ticket-updated: Assignee updated", "color: #22c55e; font-weight: bold");
    }

    recentTicketsStore.updateTicketData(ticket.value.id, {
      title: ticket.value.title,
      status: ticket.value.status,
      requester: ticket.value.requester,
      assignee: ticket.value.assignee,
    });
  }

  // Handle comment added
  function handleCommentAdded(rawData: CommentAddedEventData | SSEEventWrapper<CommentAddedEventData>): void {
    const eventData = unwrapEvent(rawData);
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) return;

    const commentData = eventData.comment;
    if (!commentData) return;

    // Check for duplicates (will catch optimistic updates)
    if (
      ticket.value.commentsAndAttachments?.find(
        (c) => c.id === commentData.id,
      )
    ) {
      console.log('[SSE] Skipping duplicate comment', commentData.id);
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
  function handleCommentDeleted(rawData: CommentDeletedEventData | SSEEventWrapper<CommentDeletedEventData>): void {
    const eventData = unwrapEvent(rawData);
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
  async function handleDeviceLinked(rawData: DeviceLinkEventData | SSEEventWrapper<DeviceLinkEventData>): Promise<void> {
    const eventData = unwrapEvent(rawData);
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) return;

    try {
      const device = await deviceService.getDeviceById(eventData.device_id);

      // Check if device already exists
      const deviceExists = ticket.value.devices?.find(
        (d) => d.id === eventData.device_id,
      );

      if (!deviceExists) {
        if (ticket.value.devices) {
          ticket.value.devices.push(device);
        } else {
          ticket.value.devices = [device];
        }
      }
    } catch (error) {
      console.error("Error fetching linked device:", error);
    }
  }

  // Handle device unlinked
  function handleDeviceUnlinked(rawData: DeviceLinkEventData | SSEEventWrapper<DeviceLinkEventData>): void {
    const eventData = unwrapEvent(rawData);
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) return;

    if (ticket.value.devices) {
      const index = ticket.value.devices.findIndex(
        (d) => d.id === eventData.device_id
      );
      if (index !== -1) {
        ticket.value.devices.splice(index, 1);
      }
    }
  }

  // Handle device updated
  function handleDeviceUpdated(rawData: DeviceUpdatedEventData | SSEEventWrapper<DeviceUpdatedEventData>): void {
    const eventData = unwrapEvent(rawData);
    if (!ticket.value?.devices) return;

    const deviceIndex = ticket.value.devices.findIndex(
      (d) => d.id === eventData.device_id,
    );

    if (
      deviceIndex !== -1 &&
      eventData.field &&
      eventData.value !== undefined
    ) {
      const device = ticket.value.devices[deviceIndex];
      // Use type assertion for dynamic field access
      (device as Record<string, unknown>)[eventData.field] = eventData.value;
    }
  }

  // Handle ticket linked
  function handleTicketLinked(rawData: TicketLinkEventData | SSEEventWrapper<TicketLinkEventData>): void {
    const eventData = unwrapEvent(rawData);
    if (!ticket.value) return;

    const isSourceTicket = eventData.ticket_id === ticket.value.id;
    const isTargetTicket = eventData.linked_ticket_id === ticket.value.id;

    if (!isSourceTicket && !isTargetTicket) return;

    const linkedTicketId = isSourceTicket
      ? eventData.linked_ticket_id
      : eventData.ticket_id;

    // Check if already linked
    const alreadyLinked = ticket.value.linkedTickets?.includes(linkedTicketId);

    if (!alreadyLinked) {
      if (ticket.value.linkedTickets) {
        ticket.value.linkedTickets.push(linkedTicketId);
      } else {
        ticket.value.linkedTickets = [linkedTicketId];
      }
    }
  }

  // Handle ticket unlinked
  function handleTicketUnlinked(rawData: TicketLinkEventData | SSEEventWrapper<TicketLinkEventData>): void {
    console.log(
      "%c[SSE Handler] ticket-unlinked received",
      "color: #f97316; font-weight: bold",
      { rawData, timestamp: new Date().toISOString() }
    );

    const eventData = unwrapEvent(rawData);
    if (!ticket.value) {
      console.log("[SSE Handler] ticket-unlinked: No ticket.value, ignoring");
      return;
    }

    const isSourceTicket = eventData.ticket_id === ticket.value.id;
    const isTargetTicket = eventData.linked_ticket_id === ticket.value.id;

    console.log("[SSE Handler] ticket-unlinked: Checking ticket match", {
      currentTicketId: ticket.value.id,
      eventTicketId: eventData.ticket_id,
      eventLinkedTicketId: eventData.linked_ticket_id,
      isSourceTicket,
      isTargetTicket,
      currentLinkedTickets: ticket.value.linkedTickets,
    });

    if (!isSourceTicket && !isTargetTicket) {
      console.log(
        "[SSE Handler] ticket-unlinked: Not for this ticket, ignoring"
      );
      return;
    }

    const linkedTicketIdToRemove = isSourceTicket
      ? eventData.linked_ticket_id
      : eventData.ticket_id;

    console.log("[SSE Handler] ticket-unlinked: Removing linked ticket", {
      linkedTicketIdToRemove,
      beforeLength: ticket.value.linkedTickets?.length || 0,
    });

    if (ticket.value.linkedTickets) {
      const index = ticket.value.linkedTickets.indexOf(linkedTicketIdToRemove);
      if (index !== -1) {
        ticket.value.linkedTickets.splice(index, 1);
        console.log(
          "%c[SSE Handler] ✅ ticket-unlinked: Successfully removed",
          "color: #22c55e; font-weight: bold",
          {
            removedTicketId: linkedTicketIdToRemove,
            afterLength: ticket.value.linkedTickets.length,
            remainingLinkedTickets: ticket.value.linkedTickets,
          }
        );
      } else {
        console.warn(
          "[SSE Handler] ticket-unlinked: Ticket not found in linkedTickets array",
          { searchedFor: linkedTicketIdToRemove }
        );
      }
    } else {
      console.warn("[SSE Handler] ticket-unlinked: No linkedTickets array");
    }
  }

  // Handle project assigned
  async function handleProjectAssigned(rawData: ProjectEventData | SSEEventWrapper<ProjectEventData>): Promise<void> {
    console.log(
      "%c[SSE Handler] project-assigned received",
      "color: #f97316; font-weight: bold",
      { rawData, timestamp: new Date().toISOString() }
    );

    const eventData = unwrapEvent(rawData);
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) {
      console.log(
        "[SSE Handler] project-assigned: Not for this ticket, ignoring",
        {
          currentTicketId: ticket.value?.id,
          eventTicketId: eventData.ticket_id,
        }
      );
      return;
    }

    const projectId = eventData.project_id;

    console.log("[SSE Handler] project-assigned: Adding to projects array", {
      projectId,
      currentProjects: ticket.value.projects,
    });

    // Check if already in the array (projects array contains Project objects or IDs)
    const projectsArray = ticket.value.projects as unknown[];
    const alreadyAssigned = projectsArray?.some((p) =>
      typeof p === 'string' ? p === projectId : (p as { id?: string })?.id === projectId
    );
    if (!alreadyAssigned) {
      if (ticket.value.projects) {
        (ticket.value.projects as unknown[]).push(projectId);
      } else {
        (ticket.value as Record<string, unknown>).projects = [projectId];
      }
      console.log(
        "%c[SSE Handler] ✅ project-assigned: Successfully assigned",
        "color: #22c55e; font-weight: bold",
        {
          projectId,
          updatedProjects: ticket.value.projects
        }
      );
    } else {
      console.log(
        "[SSE Handler] project-assigned: Already assigned, skipping",
        { projectId }
      );
    }
  }

  // Handle project unassigned
  function handleProjectUnassigned(rawData: ProjectEventData | SSEEventWrapper<ProjectEventData>): void {
    console.log(
      "%c[SSE Handler] project-unassigned received",
      "color: #f97316; font-weight: bold",
      { rawData, timestamp: new Date().toISOString() }
    );

    const eventData = unwrapEvent(rawData);
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) {
      console.log(
        "[SSE Handler] project-unassigned: Not for this ticket, ignoring",
        {
          currentTicketId: ticket.value?.id,
          eventTicketId: eventData.ticket_id,
        }
      );
      return;
    }

    const projectId = eventData.project_id;

    console.log("[SSE Handler] project-unassigned: Removing from projects array", {
      projectId,
      currentProjects: ticket.value.projects,
      beforeLength: ticket.value.projects?.length || 0,
    });

    if (ticket.value.projects) {
      const projectsArray = ticket.value.projects as unknown[];
      const index = projectsArray.findIndex((p) =>
        typeof p === 'string' ? p === projectId : (p as { id?: string })?.id === projectId
      );
      if (index !== -1) {
        projectsArray.splice(index, 1);
        console.log(
          "%c[SSE Handler] ✅ project-unassigned: Successfully unassigned",
          "color: #22c55e; font-weight: bold",
          {
            unassignedProjectId: projectId,
            afterLength: projectsArray.length,
            remainingProjects: ticket.value.projects,
          }
        );
      } else {
        console.warn(
          "[SSE Handler] project-unassigned: Project not found in array",
          { searchedFor: projectId, currentProjects: ticket.value.projects }
        );
      }
    } else {
      console.warn("[SSE Handler] project-unassigned: No projects array");
    }
  }

  // Handle viewer count changed
  function handleViewerCountChanged(rawData: ViewerCountEventData | SSEEventWrapper<ViewerCountEventData>): void {
    const eventData = unwrapEvent(rawData);
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) return;

    activeViewerCount.value = eventData.count || 0;
    console.log(
      "%c[SSE Handler] ✅ viewer-count-changed: Updated",
      "color: #22c55e; font-weight: bold",
      { count: activeViewerCount.value }
    );
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
    console.log(
      "%c[SSE Setup] Registering event listeners",
      "color: #06b6d4; font-weight: bold",
      { eventTypes: Object.keys(eventHandlers) }
    );
    (Object.entries(eventHandlers) as [TicketSSEEventType, SSEEventHandler][]).forEach(
      ([event, handler]) => {
        addEventListener(event, handler);
        console.log(`[SSE Setup] ✓ Registered listener for: ${event}`);
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
    console.log(
      "%c[SSE Setup] Mounting ticket SSE handlers",
      "color: #06b6d4; font-weight: bold; font-size: 14px",
      { ticketId: ticketId.value, isAuthenticated: authStore.isAuthenticated }
    );
    setupEventListeners();
    // Connect immediately - no delay needed
    if (authStore.isAuthenticated && ticketId.value) {
      console.log("[SSE Setup] Connecting to SSE...", { ticketId: ticketId.value });
      await connect(ticketId.value);
      console.log("[SSE Setup] SSE connection initiated");
    } else {
      console.warn("[SSE Setup] Cannot connect - missing auth or ticket ID", {
        isAuthenticated: authStore.isAuthenticated,
        ticketId: ticketId.value,
      });
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
  };
}
