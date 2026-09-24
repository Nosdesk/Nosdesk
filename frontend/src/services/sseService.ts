import { ref, computed } from "vue";
import { logger } from '@nosdesk/core/utils/logger';
import { translate } from '@nosdesk/core/i18n';
import { useAuthStore } from "@/stores/auth";
import { sseStreamUrl } from "@nosdesk/core/transport";
import apiClient from "@nosdesk/core/apiClient";

// Event handler type - uses unknown since SSE events have varying shapes
type EventHandler = (data: unknown) => void;

/**
 * The full set of SSE event names this client knows about. Used both
 * to derive the public `SSEEventType` literal and to register one
 * handler per event name on the underlying EventSource. Defining it
 * once here keeps the type and the runtime registration in sync.
 */
const ALL_SSE_EVENT_TYPES = [
  "ticket-updated",
  "ticket-created",
  "ticket-deleted",
  "ticket-merged",
  "comment-added",
  "comment-deleted",
  "asset-linked",
  "asset-unlinked",
  "asset-created",
  "asset-updated",
  "asset-deleted",
  "asset-usage-recorded",
  "asset-audit-recorded",
  "ticket-linked",
  "ticket-unlinked",
  "project-assigned",
  "project-unassigned",
  "viewers-changed",
  "ticket-field-previewed",
  "user-updated",
  "user-created",
  "user-deleted",
  "sync-actions",
  "heartbeat",
  "reconnect",
] as const;

// SSE Event types - exported for use in composables
export type SSEEventType = (typeof ALL_SSE_EVENT_TYPES)[number];

// SSE Service class optimized for performance
class SSEService {
  private eventSource: EventSource | null = null;
  private isConnected = ref(false);
  private isConnecting = ref(false);
  private lastError = ref<string | null>(null);
  private eventListeners = new Map<SSEEventType, Set<EventHandler>>();
  private reconnectTimeout: ReturnType<typeof setTimeout> | null = null;
  private reconnectAttempts = 0;
  private readonly maxReconnectAttempts = 10;
  private readonly baseReconnectDelay = 1000;
  private sseToken: string | null = null;
  private tokenExpiryTime: number | null = null;
  // Unique client ID assigned by the server on connection (for echo suppression)
  private _clientId: string | null = null;
  // Who wants the stream. The sync runtime holds it for as long as a workspace
  // is open (`start`/`stop`); ticket views add their ticket's presence topic
  // (`watchTicket`), counted so two views of one ticket share it. One
  // connection serves all of them; its topic set is rebuilt on every connect.
  private started = false;
  private watchedTickets = new Map<number, number>();

  private wanted(): boolean {
    return this.started || this.watchedTickets.size > 0;
  }

  /** SSE connection client ID (assigned by server, unique per tab/connection) */
  get clientId(): string | null {
    return this._clientId;
  }

  // Connection status
  get connectionStatus() {
    return computed(() => ({
      isConnected: this.isConnected.value,
      isConnecting: this.isConnecting.value,
      error: this.lastError.value,
      reconnectAttempts: this.reconnectAttempts,
    }));
  }

  // Get SSE token from backend with caching
  private async getSseToken(): Promise<string> {
    // `tokenExpiryTime` already has the refresh buffer subtracted (see below),
    // so this is a plain comparison. The buffer is served by the backend rather
    // than hardcoded here: the token TTL is deliberately short because it
    // travels in the URL, and a buffer larger than the TTL would mean the cache
    // never hits and every connect re-mints.
    if (this.sseToken && this.tokenExpiryTime && Date.now() < this.tokenExpiryTime) {
      return this.sseToken;
    }

    const authStore = useAuthStore();

    if (!authStore.isAuthenticated) {
      throw new Error("No authentication token available");
    }

    try {
      const response = await apiClient.post("/events/token");
      const data = response.data;

      // Cache the token and the moment we should stop using it (expiry minus
      // the backend-served refresh buffer), falling back to half the TTL if an
      // older backend omits the field.
      this.sseToken = data.sse_token;
      const bufferSecs = data.refresh_buffer ?? data.expires_in * 0.5;
      this.tokenExpiryTime =
        Date.now() + Math.max(0, data.expires_in - bufferSecs) * 1000;

      return this.sseToken!;
    } catch (error) {
      const axiosError = error as { response?: { status?: number } };
      throw new Error(
        `Failed to get SSE token: ${axiosError.response?.status || 'Network error'}`
      );
    }
  }

  // Setup event handlers efficiently using a single generic handler
  private setupEventHandlers() {
    if (!this.eventSource) return;

    // Handle the initial "connected" event to capture our client ID
    this.eventSource.addEventListener("connected", (event: MessageEvent) => {
      try {
        const data = JSON.parse(event.data);
        if (data.client_id) {
          this._clientId = data.client_id;
          logger.debug(
            `%c[SSE] Client ID assigned: ${this._clientId}`,
            "color: #22c55e; font-weight: bold"
          );
        }
      } catch (error) {
        logger.error("SSE: Failed to parse connected event:", error);
      }
    });

    // Generic handler for all event types - DRY principle
    const handleEvent = (event: MessageEvent) => {
      const eventType = event.type as SSEEventType;

      logger.debug(
        `[SSE] Raw event from EventSource: ${eventType}`,
        {
          type: eventType,
          rawData: event.data,
          timestamp: new Date().toISOString(),
        }
      );

      // Skip heartbeat events
      if (eventType === "heartbeat") return;

      // Handle server-requested reconnects
      if (eventType === "reconnect") {
        this.handleReconnectRequest();
        return;
      }

      try {
        const data = JSON.parse(event.data);

        // Echo suppression: skip events that originated from this client
        if (this._clientId && data.source_client_id === this._clientId) {
          logger.debug(
            `%c[SSE] Skipping echo from own client: ${eventType}`,
            "color: #f59e0b; font-weight: bold"
          );
          return;
        }

        // Strip source_client_id from data before passing to listeners
        // (consumers don't need to know about it)
        if (data.source_client_id !== undefined) {
          delete data.source_client_id;
        }

        logger.debug(`[SSE] Parsed event data: ${eventType}`, { parsedData: data });
        this.emit(eventType, data);
      } catch (error) {
        logger.error(`SSE: Failed to parse ${eventType}:`, error);
      }
    };

    // Register the generic handler for every known event name. The
    // single source of truth lives in ALL_SSE_EVENT_TYPES at the top
    // of this file, so the runtime registration and the SSEEventType
    // union can never drift apart.
    ALL_SSE_EVENT_TYPES.forEach((eventType) => {
      this.eventSource!.addEventListener(eventType, handleEvent);
    });
  }

  // Connection handlers
  private setupConnectionHandlers() {
    if (!this.eventSource) return;

    this.eventSource.onopen = () => {
      this.isConnected.value = true;
      this.isConnecting.value = false;
      this.lastError.value = null;
      this.reconnectAttempts = 0;
      logger.debug("[SSE Connection] connected successfully", {
        timestamp: new Date().toISOString(),
      });
    };

    this.eventSource.onerror = () => {
      this.handleConnectionError();
    };
  }

  // Handle connection errors
  private handleConnectionError() {
    this.isConnected.value = false;
    this.isConnecting.value = false;
    this.lastError.value = translate('sse-connection-failed', undefined, 'Connection failed');

    this.cleanup();

    // Auto-reconnect
    if (this.reconnectAttempts < this.maxReconnectAttempts) {
      this.scheduleReconnection();
    } else {
      logger.error("SSE: Max reconnection attempts reached");
    }
  }

  // Handle server-requested reconnection
  private handleReconnectRequest() {
    this.cleanup();
    this.reconnectAttempts = 0; // Reset attempts for server-requested reconnects
    this.connect();
  }

  // Schedule reconnection with exponential backoff
  private scheduleReconnection() {
    if (this.reconnectTimeout) {
      clearTimeout(this.reconnectTimeout);
    }

    const delay = Math.min(
      this.baseReconnectDelay * Math.pow(2, this.reconnectAttempts),
      30000, // Max 30 seconds
    );

    this.reconnectTimeout = setTimeout(() => {
      this.reconnect();
    }, delay);
  }

  // Reconnect
  private async reconnect() {
    if (this.isConnecting.value) return;

    this.reconnectAttempts++;

    try {
      await this.connect();
    } catch (error) {
      logger.error("SSE: Reconnection failed:", error);
      if (this.reconnectAttempts < this.maxReconnectAttempts) {
        this.scheduleReconnection();
      }
    }
  }

  /** Open the stream for the sync runtime; held until `stop`. */
  start(): void {
    this.started = true;
    void this.connect();
  }

  /** Close the stream and drop every holder (workspace teardown, logout). */
  stop(): void {
    this.started = false;
    this.watchedTickets.clear();
    this.disconnect();
  }

  /**
   * Receive a ticket's presence events (`viewers-changed`, field previews)
   * on the shared stream. Returns the release function.
   */
  watchTicket(ticketId: number): () => void {
    const count = this.watchedTickets.get(ticketId) ?? 0;
    this.watchedTickets.set(ticketId, count + 1);
    if (count === 0) this.reopen();
    let released = false;
    return () => {
      if (released) return;
      released = true;
      const left = (this.watchedTickets.get(ticketId) ?? 1) - 1;
      if (left > 0) {
        this.watchedTickets.set(ticketId, left);
        return;
      }
      this.watchedTickets.delete(ticketId);
      this.reopen();
    };
  }

  /**
   * Reconnect so the topic set matches the holders, or close if none remain.
   * Coalesced into one microtask, so swapping tickets reopens once.
   */
  private reopenQueued = false;
  private reopen(): void {
    if (this.reopenQueued) return;
    this.reopenQueued = true;
    queueMicrotask(() => {
      this.reopenQueued = false;
      this.applyReopen();
    });
  }

  private applyReopen(): void {
    if (!this.wanted()) {
      this.disconnect();
      return;
    }
    // A connect in flight builds its topics after its token fetch, so it
    // already picks up the change.
    if (this.isConnecting.value) return;
    this.cleanup();
    this.isConnected.value = false;
    this.reconnectAttempts = 0;
    void this.connect();
  }

  // Connect to SSE
  async connect(): Promise<void> {
    // Don't connect if already connected or connecting, or nobody holds it
    if (this.eventSource || this.isConnecting.value || !this.wanted()) {
      return;
    }

    // Check authentication
    const authStore = useAuthStore();
    if (!authStore.isAuthenticated) {
      this.lastError.value = translate('sse-no-auth-token', undefined, 'Not signed in');
      return;
    }

    this.isConnecting.value = true;
    this.lastError.value = null;

    try {
      // Get SSE token
      const sseToken = await this.getSseToken();

      // Build URL. `topics` declares the subscription set the server
      // should attach this connection to: the caller's personal
      // topic for targeted notifications plus the shared global
      // topic for cross-resource events. When a ticket id is
      // supplied, the per-ticket presence topic (`ticket-<id>`) is
      // appended so this connection receives `viewers-changed`
      // events for that ticket only. The server enforces that
      // `user` resolves to the authenticated caller and gates
      // `ticket-<id>` through ticket_visibility::can_view_ticket,
      // so this can't be used to read another user's notifications
      // or learn that a ticket exists.
      const topicTokens = ["user", "global"];
      for (const ticketId of this.watchedTickets.keys()) {
        topicTokens.push(`ticket-${ticketId}`);
      }
      // Released while the token was being fetched.
      if (!this.wanted()) {
        this.isConnecting.value = false;
        return;
      }
      const params = new URLSearchParams({
        sse_token: sseToken,
        topics: topicTokens.join(","),
      });
      const url = sseStreamUrl(params.toString());

      // Create EventSource
      this.eventSource = new EventSource(url);

      // Setup handlers
      this.setupConnectionHandlers();
      this.setupEventHandlers();
    } catch (error) {
      logger.error("SSE: Failed to connect:", error);
      this.isConnecting.value = false;
      this.lastError.value =
        error instanceof Error ? error.message : "Connection failed";

      // Schedule reconnect on connection failure
      if (this.reconnectAttempts < this.maxReconnectAttempts) {
        this.scheduleReconnection();
      }
    }
  }

  // Disconnect. Listeners stay registered: each owner removes its own (the
  // sync bridge outlives every ticket view that opens and closes the stream,
  // and clearing them here left later streams with no `sync-actions` handler).
  disconnect(): void {
    this.cleanup();
    this.sseToken = null;
    this.tokenExpiryTime = null;
    this.isConnected.value = false;
    this.isConnecting.value = false;
    this.lastError.value = null;
    this.reconnectAttempts = 0;
    this._clientId = null;
  }

  // Cleanup resources
  private cleanup(): void {
    if (this.eventSource) {
      this.eventSource.close();
      this.eventSource = null;
    }

    if (this.reconnectTimeout) {
      clearTimeout(this.reconnectTimeout);
      this.reconnectTimeout = null;
    }
  }

  // Emit event to listeners
  private emit(eventType: SSEEventType, data: unknown): void {
    const listeners = this.eventListeners.get(eventType);

    if (import.meta.env.DEV) {
      console.log(`%c[SSE] Emitting event: ${eventType}`, 'color: #10b981; font-weight: bold', {
        data,
        listenerCount: listeners?.size || 0,
      });
    }

    if (listeners && listeners.size > 0) {
      listeners.forEach((listener) => {
        try {
          listener(data);
        } catch (error) {
          console.error(`[SSE] Error in ${eventType} listener:`, error);
        }
      });
    }
  }

  // Add event listener
  addEventListener(eventType: SSEEventType, listener: EventHandler): void {
    if (!this.eventListeners.has(eventType)) {
      this.eventListeners.set(eventType, new Set());
    }
    this.eventListeners.get(eventType)!.add(listener);
  }

  // Remove event listener
  removeEventListener(eventType: SSEEventType, listener: EventHandler): void {
    const listeners = this.eventListeners.get(eventType);
    if (listeners) {
      listeners.delete(listener);
      if (listeners.size === 0) {
        this.eventListeners.delete(eventType);
      }
    }
  }

  // Manual reconnection trigger
  async triggerReconnection(): Promise<void> {
    this.reconnectAttempts = 0;
    await this.reconnect();
  }
}

// Singleton instance
let sseServiceInstance: SSEService | null = null;

const getSSEService = (): SSEService => {
  if (!sseServiceInstance) {
    sseServiceInstance = new SSEService();
  }
  return sseServiceInstance;
};

// Vue 3 composable
export function useSSE() {
  const sseService = getSSEService();

  return {
    // State
    isConnected: computed(() => sseService.connectionStatus.value.isConnected),
    isConnecting: computed(
      () => sseService.connectionStatus.value.isConnecting,
    ),
    error: computed(() => sseService.connectionStatus.value.error),
    reconnectAttempts: computed(
      () => sseService.connectionStatus.value.reconnectAttempts,
    ),

    // Methods
    start: sseService.start.bind(sseService),
    stop: sseService.stop.bind(sseService),
    watchTicket: sseService.watchTicket.bind(sseService),
    addEventListener: sseService.addEventListener.bind(sseService),
    removeEventListener: sseService.removeEventListener.bind(sseService),
    triggerReconnection: sseService.triggerReconnection.bind(sseService),
  };
}

/**
 * Get the current SSE client ID for use in API request headers.
 * Returns null if not connected.
 */
export function getSSEClientId(): string | null {
  return getSSEService().clientId;
}
