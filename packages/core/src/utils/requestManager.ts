/**
 * Request Manager
 *
 * Centralized request cancellation management using AbortController.
 * Allows services to cancel pending requests when new requests are made
 * or when components unmount.
 *
 * This replaces the duplicated RequestManager class in:
 * - ticketService.ts
 * - deviceService.ts
 * - userService.ts
 */

export class RequestManager {
  private activeRequests = new Map<string, AbortController>();

  /**
   * Create a new request with automatic cancellation of previous requests
   * @param key - Unique identifier for the request
   * @returns AbortController for the request
   */
  createRequest(key: string): AbortController {
    // Cancel any existing request with the same key
    this.cancelRequest(key);

    // Create new abort controller
    const controller = new AbortController();
    this.activeRequests.set(key, controller);

    return controller;
  }

  /**
   * Cancel a specific request by key
   * @param key - Request identifier
   */
  cancelRequest(key: string): void {
    const controller = this.activeRequests.get(key);
    if (controller) {
      controller.abort();
      this.activeRequests.delete(key);
    }
  }

  /**
   * Cancel all active requests
   */
  cancelAllRequests(): void {
    this.activeRequests.forEach(controller => controller.abort());
    this.activeRequests.clear();
  }

  /**
   * Get the number of active requests
   */
  get activeRequestCount(): number {
    return this.activeRequests.size;
  }

  /**
   * Check if a request is active
   * @param key - Request identifier
   */
  hasActiveRequest(key: string): boolean {
    return this.activeRequests.has(key);
  }
}
