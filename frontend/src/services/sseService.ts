import { ref, computed, type Ref } from 'vue';
import { useAuthStore } from '@/stores/auth';

// Event types that match the backend
export interface TicketEvent {
  type: 'TicketUpdated' | 'CommentAdded' | 'CommentDeleted' | 'AttachmentAdded' | 'AttachmentDeleted' | 
        'DeviceLinked' | 'DeviceUnlinked' | 'DeviceUpdated' | 'ProjectAssigned' | 'ProjectUnassigned' | 
        'TicketLinked' | 'TicketUnlinked' | 'Heartbeat';
  data: any;
}

export interface TicketUpdatedEvent {
  ticket_id: number;
  field: string;
  value: any;
  updated_by: string;
  timestamp: string;
}

export interface CommentAddedEvent {
  ticket_id: number;
  comment: any;
  timestamp: string;
}

export interface CommentDeletedEvent {
  ticket_id: number;
  comment_id: number;
  timestamp: string;
}

export interface DeviceUpdatedEvent {
  device_id: number;
  field: string;
  value: any;
  updated_by: string;
  timestamp: string;
}

// Event handler type
type EventHandler = (data: any) => void;

// SSE Event types for type safety
type SSEEventType = 
  | 'ticket-updated'
  | 'comment-added'
  | 'comment-deleted'
  | 'device-linked'
  | 'device-unlinked'
  | 'device-updated'
  | 'ticket-linked'
  | 'ticket-unlinked'
  | 'project-assigned'
  | 'project-unassigned'
  | 'heartbeat'
  | 'reconnect';

// SSE Service class following Vue 3 composition API patterns
class SSEService {
  private eventSource: Ref<EventSource | null> = ref(null);
  private isConnected: Ref<boolean> = ref(false);
  private isConnecting: Ref<boolean> = ref(false);
  private lastError: Ref<string | null> = ref(null);
  private eventListeners = new Map<SSEEventType, Set<EventHandler>>();
  private reconnectTimeout: ReturnType<typeof setTimeout> | null = null;
  private reconnectAttempts = 0;
  private readonly maxReconnectAttempts = 5;
  private readonly baseReconnectDelay = 1000;

  // Computed properties for reactive state
  get connectionStatus() {
    return computed(() => ({
      isConnected: this.isConnected.value,
      isConnecting: this.isConnecting.value,
      error: this.lastError.value,
      reconnectAttempts: this.reconnectAttempts
    }));
  }

  // Get SSE token from backend
  private async getSseToken(): Promise<string> {
    // Initialize auth store when needed to avoid circular dependencies
    const authStore = useAuthStore();
    
    if (!authStore.token) {
      throw new Error('No authentication token available');
    }
    
    const response = await fetch('/api/events/token', {
      method: 'POST',
      headers: {
        'Authorization': `Bearer ${authStore.token}`,
        'Content-Type': 'application/json'
      }
    });

    if (!response.ok) {
      throw new Error(`Failed to get SSE token: ${response.status}`);
    }

    const data = await response.json();
    return data.sse_token;
  }

  // Generic event handler setup
  private setupEventHandler(eventType: SSEEventType, handler: (data: any) => void) {
    if (!this.eventSource.value) return;

    this.eventSource.value.addEventListener(eventType, (event) => {
      try {
        const data = JSON.parse(event.data);
        console.log(`SSE: Received ${eventType} event:`, data);
        handler(data);
      } catch (error) {
        console.error(`SSE: Failed to parse ${eventType} event:`, error);
        console.log(`SSE: Raw event data:`, event.data);
      }
    });
  }

  // Setup all event handlers
  private setupEventHandlers() {
    if (!this.eventSource.value) return;

    // Generic event handler that emits to listeners
    const createEventHandler = (eventType: SSEEventType) => (data: any) => {
      this.emit(eventType, data);
    };

    // Setup all event types
    const eventTypes: SSEEventType[] = [
      'ticket-updated', 'comment-added', 'comment-deleted',
      'device-linked', 'device-unlinked', 'device-updated',
      'ticket-linked', 'ticket-unlinked', 'project-assigned', 'project-unassigned'
    ];

    eventTypes.forEach(eventType => {
      this.setupEventHandler(eventType, createEventHandler(eventType));
    });

    // Special handlers for system events
    this.setupEventHandler('heartbeat', (data) => {
      console.log('SSE: Heartbeat received:', data?.timestamp);
    });

    this.setupEventHandler('reconnect', (data) => {
      console.log('SSE: Reconnect requested:', data?.reason);
    });
  }

  // Connection event handlers
  private setupConnectionHandlers() {
    if (!this.eventSource.value) return;

    this.eventSource.value.onopen = () => {
      console.log('SSE: Connected');
      this.isConnected.value = true;
      this.isConnecting.value = false;
      this.lastError.value = null;
      this.reconnectAttempts = 0;
    };

    this.eventSource.value.onerror = (error) => {
      console.error('SSE: Connection error', error);
      this.handleConnectionError();
    };

    // Additional error event listener for more detailed error information
    this.eventSource.value.addEventListener('error', (event) => {
      console.error('SSE: EventSource error event:', event);
    });
  }

  // Handle connection errors
  private handleConnectionError() {
    this.isConnected.value = false;
    this.isConnecting.value = false;
    this.lastError.value = 'Connection failed';
    
    this.cleanup();
    
    // Attempt reconnection if we haven't exceeded max attempts
    if (this.reconnectAttempts < this.maxReconnectAttempts) {
      this.scheduleReconnection();
    }
  }

  // Schedule reconnection with exponential backoff
  private scheduleReconnection() {
    if (this.reconnectTimeout) {
      clearTimeout(this.reconnectTimeout);
    }

    const delay = this.baseReconnectDelay * Math.pow(2, this.reconnectAttempts);
    console.log(`SSE: Scheduling reconnection in ${delay}ms (attempt ${this.reconnectAttempts + 1}/${this.maxReconnectAttempts})`);

    this.reconnectTimeout = setTimeout(() => {
      this.reconnect();
    }, delay);
  }

  // Reconnection logic
  private async reconnect() {
    if (this.isConnecting.value) {
      console.log('SSE: Reconnection already in progress, skipping...');
      return;
    }

    this.reconnectAttempts++;
    console.log(`SSE: Attempting reconnection ${this.reconnectAttempts}/${this.maxReconnectAttempts}`);

    try {
      await this.connect();
    } catch (error) {
      console.error('SSE: Reconnection failed:', error);
      if (this.reconnectAttempts < this.maxReconnectAttempts) {
        this.scheduleReconnection();
      }
    }
  }

  // Connect to SSE
  async connect(ticketId?: number): Promise<void> {
    // Don't connect if already connected or connecting
    if (this.eventSource.value || this.isConnecting.value) {
      return;
    }

    // Check for token when connecting, not at module level
    try {
      const authStore = useAuthStore();
      if (!authStore.token) {
        this.lastError.value = 'No authentication token';
        return;
      }
    } catch (error) {
      this.lastError.value = 'Authentication store not available';
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
        params.append('ticket_id', ticketId.toString());
      }
      const url = `/api/events/tickets?${params.toString()}`;

      // Create EventSource
      this.eventSource.value = new EventSource(url);

      // Setup handlers
      this.setupConnectionHandlers();
      this.setupEventHandlers();

    } catch (error) {
      console.error('SSE: Failed to connect', error);
      this.isConnecting.value = false;
      this.lastError.value = error instanceof Error ? error.message : 'Failed to connect';
      throw error;
    }
  }

  // Disconnect from SSE
  disconnect(): void {
    this.cleanup();
    this.isConnected.value = false;
    this.isConnecting.value = false;
    this.lastError.value = null;
  }

  // Cleanup resources
  private cleanup(): void {
    if (this.eventSource.value) {
      this.eventSource.value.close();
      this.eventSource.value = null;
    }

    if (this.reconnectTimeout) {
      clearTimeout(this.reconnectTimeout);
      this.reconnectTimeout = null;
    }
  }

  // Emit event to listeners
  private emit(eventType: SSEEventType, data: any): void {
    const listeners = this.eventListeners.get(eventType);
    if (listeners) {
      listeners.forEach(listener => {
        try {
          listener(data);
        } catch (error) {
          console.error(`SSE: Error in ${eventType} listener`, error);
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

// Create singleton instance lazily to avoid circular dependencies
let sseServiceInstance: SSEService | null = null;

const getSSEService = (): SSEService => {
  if (!sseServiceInstance) {
    sseServiceInstance = new SSEService();
  }
  return sseServiceInstance;
};

// Vue 3 composable that provides the SSE service
export function useSSE() {
  const sseService = getSSEService();
  
  return {
    // State
    isConnected: computed(() => sseService.connectionStatus.value.isConnected),
    isConnecting: computed(() => sseService.connectionStatus.value.isConnecting),
    error: computed(() => sseService.connectionStatus.value.error),
    reconnectAttempts: computed(() => sseService.connectionStatus.value.reconnectAttempts),
    
    // Methods
    connect: sseService.connect.bind(sseService),
    disconnect: sseService.disconnect.bind(sseService),
    addEventListener: sseService.addEventListener.bind(sseService),
    removeEventListener: sseService.removeEventListener.bind(sseService),
    triggerReconnection: sseService.triggerReconnection.bind(sseService)
  };
} 