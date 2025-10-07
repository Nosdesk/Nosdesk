import { ref, computed } from "vue";
import { useAuthStore } from "@/stores/auth";

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
    | "Heartbeat";
  data: any;
}

// Event handler type
type EventHandler = (data: any) => void;

// SSE Event types
type SSEEventType =
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

    if (!authStore.token) {
      throw new Error("No authentication token available");
    }

    const response = await fetch("/api/events/token", {
      method: "POST",
      headers: {
        Authorization: `Bearer ${authStore.token}`,
        "Content-Type": "application/json",
      },
    });

    if (!response.ok) {
      throw new Error(`Failed to get SSE token: ${response.status}`);
    }

    const data = await response.json();

    // Cache token and expiry
    this.sseToken = data.sse_token;
    this.tokenExpiryTime = Date.now() + data.expires_in * 1000;

    console.log("SSE: Got new token, expires in", data.expires_in, "seconds");

    return this.sseToken!;
  }

  // Setup event handlers efficiently
  private setupEventHandlers() {
    if (!this.eventSource) return;

    console.log("SSE: Setting up event handlers");

    // Event types to listen for
    const eventTypes: SSEEventType[] = [
      "ticket-updated",
      "comment-added",
      "comment-deleted",
      "device-linked",
      "device-unlinked",
      "device-updated",
      "ticket-linked",
      "ticket-unlinked",
      "project-assigned",
      "project-unassigned",
    ];

    // Setup listeners for each event type
    eventTypes.forEach((eventType) => {
      this.eventSource!.addEventListener(eventType, (event: MessageEvent) => {
        console.log(`SSE: 📨 Received ${eventType} event`);
        console.log(`SSE: Raw data:`, event.data);
        try {
          const data = JSON.parse(event.data);
          console.log(`SSE: ✅ Parsed ${eventType} data:`, data);
          console.log(
            `SSE: About to emit to ${this.eventListeners.get(eventType)?.size || 0} listeners`,
          );
          this.emit(eventType, data);
        } catch (error) {
          console.error(`SSE: ❌ Failed to parse ${eventType}:`, error);
        }
      });
      console.log(`SSE: Registered EventSource listener for: ${eventType}`);
    });

    // Heartbeat handler
    this.eventSource.addEventListener("heartbeat", () => {
      console.log("SSE: ❤️ Heartbeat");
    });

    // Reconnect handler
    this.eventSource.addEventListener("reconnect", (event: MessageEvent) => {
      console.warn("SSE: Server requested reconnect");
      this.handleReconnectRequest();
    });

    // Add generic message handler to catch any events we might be missing
    this.eventSource.onmessage = (event: MessageEvent) => {
      console.log("SSE: Generic onmessage received:", event.type, event.data);
    };

    // Add a catch-all listener to see ALL events
    this.eventSource.addEventListener("message", (event: MessageEvent) => {
      console.log("SSE: Message event listener:", event.type, event.data);
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
      console.log("SSE: ✅ Connection opened");
      console.log("SSE: ReadyState:", this.eventSource?.readyState);
      console.log("SSE: URL:", this.eventSource?.url);
      console.log(
        "SSE: Registered event listeners count:",
        this.eventListeners.size,
      );
      this.eventListeners.forEach((listeners, eventType) => {
        console.log(`  - ${eventType}: ${listeners.size} listeners`);
      });
    };

    this.eventSource.onerror = (error) => {
      console.error("SSE: ❌ Error event:", error);
      console.log("SSE: ReadyState:", this.eventSource?.readyState);
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
      console.error("SSE: Max reconnection attempts reached");
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

    console.log(
      `SSE: Reconnecting in ${delay}ms (attempt ${this.reconnectAttempts + 1})`,
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
      console.error("SSE: Reconnection failed:", error);
      if (this.reconnectAttempts < this.maxReconnectAttempts) {
        this.scheduleReconnection();
      }
    }
  }

  // Connect to SSE
  async connect(ticketId?: number): Promise<void> {
    // Don't connect if already connected or connecting
    if (this.eventSource || this.isConnecting.value) {
      console.log("SSE: Already connected or connecting");
      return;
    }

    // Check authentication
    try {
      const authStore = useAuthStore();
      if (!authStore.token) {
        this.lastError.value = "No authentication token";
        console.error("SSE: No auth token");
        return;
      }
    } catch (error) {
      this.lastError.value = "Authentication not available";
      console.error("SSE: Auth not available");
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
      const url = `/api/events/tickets?${params.toString()}`;

      console.log("SSE: Connecting to:", url);

      // Create EventSource
      this.eventSource = new EventSource(url);

      // Setup handlers
      this.setupConnectionHandlers();
      this.setupEventHandlers();

      console.log("SSE: EventSource created, waiting for connection...");
    } catch (error) {
      console.error("SSE: Failed to connect:", error);
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
    console.log("SSE: Disconnecting");
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
      console.log("SSE: EventSource closed");
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
  private emit(eventType: SSEEventType, data: any): void {
    const listeners = this.eventListeners.get(eventType);
    console.log(
      `SSE: Emitting ${eventType} to ${listeners?.size || 0} listeners`,
    );

    if (listeners && listeners.size > 0) {
      listeners.forEach((listener) => {
        try {
          listener(data);
        } catch (error) {
          console.error(`SSE: Error in ${eventType} listener:`, error);
        }
      });
    } else {
      console.warn(`SSE: No listeners registered for ${eventType}`);
    }
  }

  // Add event listener
  addEventListener(eventType: SSEEventType, listener: EventHandler): void {
    if (!this.eventListeners.has(eventType)) {
      this.eventListeners.set(eventType, new Set());
    }
    this.eventListeners.get(eventType)!.add(listener);
    console.log(
      `SSE: Added listener for ${eventType} (total: ${this.eventListeners.get(eventType)!.size})`,
    );
  }

  // Remove event listener
  removeEventListener(eventType: SSEEventType, listener: EventHandler): void {
    const listeners = this.eventListeners.get(eventType);
    if (listeners) {
      listeners.delete(listener);
      if (listeners.size === 0) {
        this.eventListeners.delete(eventType);
      }
      console.log(
        `SSE: Removed listener for ${eventType} (remaining: ${listeners.size})`,
      );
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
