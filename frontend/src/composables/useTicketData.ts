import { ref, computed } from "vue";
import { useRouter } from "vue-router";
import { useQueryCache } from "@pinia/colada";
import { useRecentTicketsStore } from "@/stores/recentTickets";
import { useTitleManager } from "@/composables/useTitleManager";
import ticketService from "@/services/ticketService";
import { logger } from "@/utils/logger";
import { formatDateTime, getCurrentUTCDateTime } from "@/utils/dateUtils";
import { ticketDetailKey } from "@/loaders/ticketDetailLoader";
import type { TicketStatus, TicketPriority } from "@/constants/ticketOptions";
import type { Ticket, Device, Project } from '@/types/ticket';
import type { CommentWithAttachments } from '@/types/comment';

// Local type extending the canonical Ticket type with UI-specific
// fields. `projects` flows through as the `string[]` arm of the
// canonical `Project[] | string[]` union because the multi-select
// UI tracks IDs only. The server response is mapped to `string[]`
// via `.map(p => String(p.id))` at fetch time.
interface LocalTicket extends Ticket {
  commentsAndAttachments?: CommentWithAttachments[];
}

/**
 * Composable for managing ticket data and state
 */
export function useTicketData() {
  const router = useRouter();
  const recentTicketsStore = useRecentTicketsStore();
  const titleManager = useTitleManager();
  const queryCache = useQueryCache();

  // State
  const ticket = ref<LocalTicket | null>(null);
  // `loading` starts false so the cached / loader-primed path
  // mounts straight into the real content. The fetch path flips
  // it to true before going to network; the cached path skips
  // that flip entirely so the skeleton never gets a frame.
  const loading = ref(false);
  const error = ref<string | null>(null);
  const selectedStatus = ref<TicketStatus>("open");
  const selectedPriority = ref<TicketPriority>("low");
  const selectedCategory = ref<number | null>(null);
  const selectedWorkflowStateId = ref<number | null>(null);

  // Computed
  const formattedCreatedDate = computed(() =>
    formatDateTime(ticket.value?.created),
  );
  const formattedModifiedDate = computed(() =>
    formatDateTime(ticket.value?.modified),
  );
  const comments = computed(() => ticket.value?.commentsAndAttachments || []);
  const devices = computed(() => ticket.value?.devices || []);

  // Transform comments from API format
  // Uses spread to preserve all fields (including future additions like transcription)
  // Only explicitly maps fields that need transformation
  function transformComments(apiComments: CommentWithAttachments[]): CommentWithAttachments[] {
    return apiComments
      .map((comment) => ({
        ...comment,
        createdAt: comment.created_at, // Add camelCase alias for consistency
        attachments: comment.attachments || [],
      }))
      .sort(
        (a, b) =>
          new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
      );
  }

  // Transform devices from API format
  // Uses spread to preserve all fields automatically
  function transformDevices(apiDevices: Device[]): Device[] {
    return apiDevices.map((device) => ({ ...device }));
  }

  /** Map a raw API ticket onto the LocalTicket shape the view
   * binds to. Pure transform — no side effects so both the
   * cache-hit path and the network path can call it. */
  function applyTicket(fetched: Ticket): void {
    const commentsAndAttachments = transformComments(
      (fetched as { comments?: CommentWithAttachments[] }).comments || [],
    );
    const transformedDevices = transformDevices(fetched.devices || []);
    const fetchedProjects = fetched.projects as Project[] | undefined;
    const projectIds = fetchedProjects?.map((p) => String(p.id)) || [];

    ticket.value = {
      ...fetched,
      projects: projectIds,
      linkedTickets:
        fetched.linked_tickets || fetched.linkedTickets || [],
      devices: transformedDevices,
      commentsAndAttachments,
    } as LocalTicket;

    selectedStatus.value = ticket.value.status;
    selectedPriority.value = ticket.value.priority;
    selectedCategory.value = ticket.value.category_id || null;
    selectedWorkflowStateId.value = ticket.value.workflow_state_id ?? null;

    titleManager.setTicket({
      id: ticket.value.id,
      title: ticket.value.title,
    });
  }

  /** Fetch a ticket, hitting the Pinia Colada cache primed by
   * the route loader before going to network. The cache hit
   * sets the ticket synchronously so the skeleton never renders;
   * the recorded-view ping still fires in the background. */
  async function fetchTicket(
    ticketId: string | string[],
  ): Promise<void> {
    const id = Number(ticketId);
    if (!Number.isFinite(id)) return;
    error.value = null;

    // Cache-first. The route loader (ticketDetailLoader) writes
    // `ticketDetailKey(id)` during navigation; if we're here as
    // a result of that navigation, the cache has the row and we
    // can mount straight into the real content.
    const cached = queryCache.getQueryData(ticketDetailKey(id)) as
      | Ticket
      | undefined;
    if (cached) {
      applyTicket(cached);
      // Record the view in the background. Failure here doesn't
      // need to block the user from seeing the ticket.
      void recentTicketsStore.recordTicketView(id);
      return;
    }

    // Cold path: deep-link / hard-refresh / cache miss. Fall
    // through to a network fetch and surface the loading state
    // so the skeleton renders.
    loading.value = true;
    try {
      const fetchedTicket = await ticketService.getTicketById(id);
      if (!fetchedTicket) {
        router.push("/404");
        return;
      }
      applyTicket(fetchedTicket);
      // Prime the cache for repeat back-nav from elsewhere in the
      // app — keeps return visits flash-free.
      queryCache.setQueryData(ticketDetailKey(id), fetchedTicket);
      await recentTicketsStore.recordTicketView(id);
    } catch (err) {
      logger.error(`Error fetching ticket ${id}`, { error: err });
      error.value = "Failed to load ticket. Please try again later.";
    } finally {
      loading.value = false;
    }
  }

  // Refresh ticket
  async function refreshTicket(): Promise<void> {
    if (ticket.value) {
      await fetchTicket(String(ticket.value.id));
    }
  }

  // Update ticket field
  async function updateTicketField<K extends keyof LocalTicket>(field: K, value: LocalTicket[K]): Promise<void> {
    if (!ticket.value) return;

    const oldValue = ticket.value[field];
    if (oldValue === value) return;

    try {
      const nowDateTime = getCurrentUTCDateTime();
      const updateData = { [field]: value, modified: nowDateTime };

      // Optimistic update - use direct mutation to preserve object reference
      // This prevents component remounts when the ticket object is updated
      ticket.value[field] = value;
      ticket.value.modified = nowDateTime;

      // Clear user objects when clearing requester/assignee
      if (field === "requester" && !value) {
        ticket.value.requester_user = undefined;
      }
      if (field === "assignee" && !value) {
        ticket.value.assignee_user = undefined;
      }

      // Update UI-specific refs. Per-key narrowing is mechanical —
      // TypeScript can't narrow `value: LocalTicket[K]` from a
      // `field === "status"` guard, so we cast at the assignment.
      if (field === "status") selectedStatus.value = value as TicketStatus;
      if (field === "priority") selectedPriority.value = value as TicketPriority;

      // Update stores for consistent state
      if (["title", "status", "requester", "assignee"].includes(field)) {
        recentTicketsStore.updateTicketData(ticket.value.id, {
          [field]: value,
        });
      }

      if (field === "title") {
        titleManager.setTicket({ id: ticket.value.id, title: value as string });
      }

      // Send update to backend - SSE will broadcast to other clients
      const response = await ticketService.updateTicket(ticket.value.id, updateData);

      // Update user objects from backend response to keep UI in sync
      if (response && ticket.value) {
        if (field === "requester" && response.requester_user) {
          ticket.value.requester_user = response.requester_user;
        }
        if (field === "assignee" && response.assignee_user) {
          ticket.value.assignee_user = response.assignee_user;
        }
      }
    } catch (err) {
      logger.error(`Error updating ticket field: ${field}`, { error: err, field });
      // Revert optimistic update on error - also use direct mutation
      ticket.value[field] = oldValue;
      if (field === "status") selectedStatus.value = oldValue as TicketStatus;
      if (field === "priority") selectedPriority.value = oldValue as TicketPriority;
      throw err;
    }
  }

  // Update status
  async function updateStatus(newStatus: TicketStatus): Promise<void> {
    await updateTicketField("status", newStatus);
  }

  // Update workflow state by id. The backend recomputes the legacy
  // status bucket from the new state's category, so the optimistic
  // mutation here is best-effort: we set the id locally, then rely on
  // the API response's `status` field to re-sync the legacy ref. We
  // don't try to predict the bucket on the client.
  async function updateWorkflowState(newId: number): Promise<void> {
    if (!ticket.value) return;
    const oldId = ticket.value.workflow_state_id ?? null;
    if (oldId === newId) return;

    try {
      const nowDateTime = getCurrentUTCDateTime();
      ticket.value.workflow_state_id = newId;
      ticket.value.modified = nowDateTime;
      selectedWorkflowStateId.value = newId;

      const response = await ticketService.updateTicket(ticket.value.id, {
        workflow_state_id: newId,
        modified: nowDateTime,
      });

      if (response && ticket.value) {
        if (response.status) {
          ticket.value.status = response.status;
          selectedStatus.value = response.status;
        }
        if (response.workflow_state) {
          ticket.value.workflow_state = response.workflow_state;
        }
        recentTicketsStore.updateTicketData(ticket.value.id, {
          status: ticket.value.status,
        });
      }
    } catch (err) {
      logger.error('Error updating workflow state', { error: err });
      if (ticket.value) {
        ticket.value.workflow_state_id = oldId ?? undefined;
        selectedWorkflowStateId.value = oldId;
      }
      throw err;
    }
  }

  // Update priority
  async function updatePriority(newPriority: TicketPriority): Promise<void> {
    await updateTicketField("priority", newPriority);
  }

  // Update requester
  async function updateRequester(newRequester: string): Promise<void> {
    await updateTicketField("requester", newRequester);
  }

  // Update assignee
  async function updateAssignee(newAssignee: string): Promise<void> {
    await updateTicketField("assignee", newAssignee);
  }

  // Update title
  async function updateTitle(newTitle: string): Promise<void> {
    await updateTicketField("title", newTitle);
  }

  // Update due_date. Pass-through; the input component already
  // serialised to RFC3339 (or null to clear).
  async function updateDueDate(newDueDate: string | null): Promise<void> {
    await updateTicketField("due_date", newDueDate);
  }

  // Update recurrence_rule. Pass-through to the API; the backend
  // validates the rule lazily on close.
  async function updateRecurrenceRule(newRule: string | null): Promise<void> {
    await updateTicketField("recurrence_rule", newRule);
  }

  // Update category
  async function updateCategory(newCategory: string): Promise<void> {
    const categoryId = newCategory ? parseInt(newCategory, 10) : null;
    selectedCategory.value = categoryId;
    await updateTicketField("category_id", categoryId);
  }

  // Delete ticket
  async function deleteTicket(): Promise<void> {
    if (!ticket.value) return;

    const ticketId = ticket.value.id;
    await ticketService.deleteTicket(ticketId);
    // Recent tickets will be automatically updated when the list is refreshed
    router.push("/tickets");
  }

  return {
    // State
    ticket,
    loading,
    error,
    selectedStatus,
    selectedPriority,
    selectedCategory,
    selectedWorkflowStateId,

    // Computed
    formattedCreatedDate,
    formattedModifiedDate,
    comments,
    devices,

    // Methods
    fetchTicket,
    refreshTicket,
    updateStatus,
    updateWorkflowState,
    updatePriority,
    updateCategory,
    updateRequester,
    updateAssignee,
    updateTitle,
    updateDueDate,
    updateRecurrenceRule,
    deleteTicket,
  };
}
