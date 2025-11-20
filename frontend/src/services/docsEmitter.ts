/**
 * Documentation Event Emitter
 *
 * Provides real-time event broadcasting for documentation changes
 * Used to update sidebar navigation and other components when docs are created/updated/deleted
 */

import { logger } from '@/utils/logger';

type DocumentEvent = {
  id: number | string;
  title?: string;
  parentId?: number | null;
};

type EventCallback = (data: DocumentEvent) => void;

class DocsEventEmitter {
  private events: Map<string, Set<EventCallback>> = new Map();

  /**
   * Subscribe to a documentation event
   */
  on(event: string, callback: EventCallback): () => void {
    if (!this.events.has(event)) {
      this.events.set(event, new Set());
    }

    this.events.get(event)!.add(callback);

    // Return unsubscribe function
    return () => this.off(event, callback);
  }

  /**
   * Unsubscribe from a documentation event
   */
  off(event: string, callback: EventCallback): void {
    const callbacks = this.events.get(event);
    if (callbacks) {
      callbacks.delete(callback);
      if (callbacks.size === 0) {
        this.events.delete(event);
      }
    }
  }

  /**
   * Emit a documentation event
   */
  emit(event: string, data: DocumentEvent): void {
    const callbacks = this.events.get(event);
    if (callbacks) {
      callbacks.forEach(callback => {
        try {
          callback(data);
        } catch (error) {
          logger.error(`Error in docs event handler for ${event}:`, error);
        }
      });
    }
  }

  /**
   * Remove all event listeners
   */
  clear(): void {
    this.events.clear();
  }
}

// Export a singleton instance
export const docsEmitter = new DocsEventEmitter();
export default docsEmitter;
