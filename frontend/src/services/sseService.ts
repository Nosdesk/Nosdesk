import { ref, computed } from "vue";
import { logger } from '@/utils/logger';
import { useAuthStore } from "@/stores/auth";
import apiClient from "./apiConfig";

// Event types that match the backend
export interface TicketEvent {
  type:
    | "TicketUpdated"
    | "CommentAdded"
    | "CommentDeleted"
    | "AttachmentAdded"
    | "AttachmentDeleted"
    | "DeviceLinked"
    | "DeviceUnlinked"
    | "DeviceUpdated"
    | "ProjectAssigned"
    | "ProjectUnassigned"
    | "TicketLinked"
    | "TicketUnlinked"
    | "DocumentationUpdated"
    | "Heartbeat";
  data: unknown;
}

// Event handler type - uses unknown since SSE events have varying shapes
type EventHandler = (data: unknown) => void;

// SSE Event types - exported for use in composables
export type SSEEventType =
  | "ticket-updated"
  | "ticket-created"
  | "ticket-deleted"
  | "comment-added"
  | "comment-deleted"
  | "device-linked"
  | "device-unlinked"
  | "device-updated"
  | "ticket-linked"
  | "ticket-unlinked"
  | "project-assigned"
  | "project-unassigned"
  | "documentation-updated"
  | "user-updated"
  | "user-created"
  | "user-deleted"
  | "notification-received"
  | "heartbeat"
  | "reconnect";

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
    // Return cached token if still valid (with 5 min buffer)
    if (
      this.sseToken &&
      this.tokenExpiryTime &&
      Date.now() < this.tokenExpiryTime - 300000
    ) {
      return this.sseToken;
    }

    const authStore = useAuthStore();

    if (!authStore.isAuthenticated) {
      throw new Error("No authentication token available");
    }

    try {
      const response = await apiClient.post("/events/token");
      const data = response.data;

      // Cache token and expiry
      this.sseToken = data.sse_token;
      this.tokenExpiryTime = Date.now() + data.expires_in * 1000;

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

    // Generic handler for all event types - DRY principle
    const handleEvent = (event: MessageEvent) => {
      const eventType = event.type as SSEEventType;

      logger.debug(
        `%c[SSE] Raw event from EventSource: ${eventType}`,
        "color: #3b82f6; font-weight: bold",
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
        logger.debug(
          `%c[SSE] Parsed event data:`,
          "color: #8b5cf6; font-weight: bold",
          { eventType, parsedData: data }
        );
        this.emit(eventType, data);
      } catch (error) {
        logger.error(`SSE: Failed to parse ${eventType}:`, error);
      }
    };

    // Register the generic handler for all event types
    const eventTypes: SSEEventType[] = [
      "ticket-updated",
      "ticket-created",
      "ticket-deleted",
      "comment-added",
      "comment-deleted",
      "device-linked",
      "device-unlinked",
      "device-updated",
      "ticket-linked",
      "ticket-unlinked",
      "project-assigned",
      "project-unassigned",
      "documentation-updated",
      "user-updated",
      "user-created",
      "user-deleted",
      "notification-received",
      "heartbeat",
      "reconnect",
    ];

    eventTypes.forEach((eventType) => {
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
      logger.debug(
        "%c[SSE Connection] ✅ Connected successfully",
        "color: #22c55e; font-weight: bold; font-size: 14px",
        { timestamp: new Date().toISOString() }
      );
    };

    this.eventSource.onerror = () => {
      this.handleConnectionError();
    };
  }

  // Handle connection errors
  private handleConnectionError() {
    this.isConnected.value = false;
    this.isConnecting.value = false;
    this.lastError.value = "Connection failed";

    this.cleanup(false); // Don't clear listeners

    // Auto-reconnect
    if (this.reconnectAttempts < this.maxReconnectAttempts) {
      this.scheduleReconnection();
    } else {
      logger.error("SSE: Max reconnection attempts reached");
    }
  }

  // Handle server-requested reconnection
  private handleReconnectRequest() {
    this.cleanup(false);
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

  // Connect to SSE
  async connect(ticketId?: number): Promise<void> {
    // Don't connect if already connected or connecting
    if (this.eventSource || this.isConnecting.value) {
      return;
    }

    // Check authentication
    const authStore = useAuthStore();
    if (!authStore.isAuthenticated) {
      this.lastError.value = "No authentication token";
      return;
    }

    this.isConnecting.value = true;
    this.lastError.value = null;

    try {
      // Get SSE token
      const sseToken = await this.getSseToken();

      // Build URL
      const params = new URLSearchParams({ sse_token: sseToken });
      if (ticketId) {
        params.append("ticket_id", ticketId.toString());
      }
      const url = `/api/events/stream?${params.toString()}`;

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

  // Disconnect
  disconnect(): void {
    this.cleanup(true);
    this.isConnected.value = false;
    this.isConnecting.value = false;
    this.lastError.value = null;
    this.reconnectAttempts = 0;
  }

  // Cleanup resources
  private cleanup(clearListeners: boolean = true): void {
    if (this.eventSource) {
      this.eventSource.close();
      this.eventSource = null;
    }

    if (this.reconnectTimeout) {
      clearTimeout(this.reconnectTimeout);
      this.reconnectTimeout = null;
    }

    if (clearListeners) {
      this.eventListeners.clear();
      this.sseToken = null;
      this.tokenExpiryTime = null;
    }
  }

  // Emit event to listeners
  private emit(eventType: SSEEventType, data: unknown): void {
    const listeners = this.eventListeners.get(eventType);

    // Direct console.log for debugging
    console.log(`%c[SSE] Emitting event: ${eventType}`, 'color: #10b981; font-weight: bold', {
      data,
      listenerCount: listeners?.size || 0,
      allEventTypes: Array.from(this.eventListeners.keys()),
    });

    if (listeners && listeners.size > 0) {
      console.log(`[SSE] Calling ${listeners.size} listener(s) for ${eventType}`);
      listeners.forEach((listener) => {
        try {
          listener(data);
        } catch (error) {
          console.error(`[SSE] Error in ${eventType} listener:`, error);
        }
      });
    } else {
      console.warn(`%c[SSE] No listeners for event: ${eventType}`, 'color: #f59e0b; font-weight: bold');
    }
  }

  // Add event listener
  addEventListener(eventType: SSEEventType, listener: EventHandler): void {
    if (!this.eventListeners.has(eventType)) {
      this.eventListeners.set(eventType, new Set());
    }
    this.eventListeners.get(eventType)!.add(listener);
    console.log(`[SSE] Registered listener for: ${eventType}`, {
      listenerCount: this.eventListeners.get(eventType)!.size,
      allEventTypes: Array.from(this.eventListeners.keys()),
    });
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
    connect: sseService.connect.bind(sseService),
    disconnect: sseService.disconnect.bind(sseService),
    addEventListener: sseService.addEventListener.bind(sseService),
    removeEventListener: sseService.removeEventListener.bind(sseService),
    triggerReconnection: sseService.triggerReconnection.bind(sseService),
  };
}
