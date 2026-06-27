import { ref, computed, onMounted, onUnmounted, type Ref } from "vue";
import { useSSE } from "@/services/sseService";
import { useAuthStore } from "@/stores/auth";
import * as pool from "@nosdesk/core/sync/pool";
import {
  unwrapEventData,
  type ViewerInfo,
  type ViewersChangedEventData,
  type TicketFieldPreviewedEventData,
} from "@nosdesk/core/types/sse";

// Enable debug logging only in development
const DEBUG_SSE = import.meta.env.DEV && import.meta.env.VITE_DEBUG_SSE === 'true';

/**
 * Presence + live-preview channel for the ticket detail view.
 *
 * Ticket *data* (status, comments, links, assets, merge state, ...) now
 * flows through the sync object pool (see `useTicketDetail`), so this
 * composable is reduced to the two things the pool can't express:
 *
 *  - **Presence**: who else is viewing the ticket (`viewers-changed`),
 *    surfaced as `otherViewers` for the PresenceStack, plus the live
 *    connection status (`isConnected`).
 *  - **Field preview**: another viewer's in-progress, not-yet-committed
 *    keystrokes (`ticket-field-previewed`). The committed value still
 *    arrives via the pool; this just mirrors the typing in real time.
 *
 * Both ride the per-ticket SSE topic opened by `connect(ticketId)`.
 * The local field-edit guard (`startEditing` / `stopEditing`) protects
 * a field the current user is actively editing from being overwritten
 * by an incoming remote preview.
 */
export function useTicketSSE(ticketId: Ref<number | undefined>) {
  const { addEventListener, removeEventListener, isConnected, connect, disconnect } = useSSE();

  const authStore = useAuthStore();

  // Raw viewer list from the backend (already deduped per user and
  // sorted recency-first). Includes the current user; the UI filters
  // self out via `otherViewers` so it never shows you back to yourself.
  const activeViewers = ref<ViewerInfo[]>([]);
  const otherViewers = computed<ViewerInfo[]>(() => {
    const selfUuid = authStore.user?.uuid;
    if (!selfUuid) return activeViewers.value;
    return activeViewers.value.filter((v) => v.user_uuid !== selfUuid);
  });

  // Fields the current user is actively editing locally (e.g. the
  // title input is focused). A remote preview for such a field is
  // skipped so it can't clobber in-progress text.
  const editingFields = ref<Set<string>>(new Set());
  function startEditing(field: string): void {
    editingFields.value.add(field);
  }
  function stopEditing(field: string): void {
    editingFields.value.delete(field);
  }
  function shouldApplyUpdate(field: string): boolean {
    return !editingFields.value.has(field);
  }

  // viewers-changed: backend sends the full viewer set on every change,
  // so swap the ref wholesale. Defensive ticket_id filter even though
  // the per-ticket topic already scopes us.
  function handleViewersChanged(rawData: unknown): void {
    const eventData = unwrapEventData(rawData as ViewersChangedEventData);
    if (ticketId.value == null || eventData.ticket_id !== ticketId.value) return;
    activeViewers.value = eventData.viewers ?? [];
  }

  // ticket-field-previewed: another viewer is typing. Mirror it so the
  // UI reflects their keystrokes before they commit. No persistence;
  // the eventual committed value arrives via the pool. Echo suppression
  // for our own previews is handled upstream by sseService
  // (source_client_id match); `shouldApplyUpdate` additionally protects
  // a field WE are editing locally. Title previews route through the
  // title manager (the header owns the editable title); resolution
  // notes mirror onto the pool row the detail view reads.
  function handleTicketFieldPreviewed(rawData: unknown): void {
    const eventData = unwrapEventData(rawData as TicketFieldPreviewedEventData);
    if (ticketId.value == null || eventData.ticket_id !== ticketId.value) return;
    if (!shouldApplyUpdate(eventData.field)) return;

    // Mirror the in-flight value onto the pool row the detail view + header
    // read. Non-destructive (preserves every other field, unlike the old
    // setTicket stub) and consistent across preview fields; the committed
    // value still arrives via the pool and supersedes this.
    if (eventData.field === "title" || eventData.field === "resolution_notes") {
      pool.patch("ticket", ticketId.value, { [eventData.field]: eventData.value });
    }
  }

  function setupEventListeners(): void {
    addEventListener("viewers-changed", handleViewersChanged);
    addEventListener("ticket-field-previewed", handleTicketFieldPreviewed);
  }
  function cleanupEventListeners(): void {
    removeEventListener("viewers-changed", handleViewersChanged);
    removeEventListener("ticket-field-previewed", handleTicketFieldPreviewed);
  }

  onMounted(async () => {
    setupEventListeners();
    if (authStore.isAuthenticated && ticketId.value) {
      if (DEBUG_SSE) console.log('[SSE] Connecting for ticket:', ticketId.value);
      await connect(ticketId.value);
    }
  });

  onUnmounted(() => {
    cleanupEventListeners();
    disconnect();
  });

  return {
    isConnected,
    activeViewers,
    otherViewers,
    // Field-edit guard: protects actively-edited fields (e.g. title)
    // from remote preview overwrites.
    startEditing,
    stopEditing,
  };
}
