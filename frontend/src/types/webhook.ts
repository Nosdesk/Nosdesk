/**
 * Webhook Types
 * For external integration webhooks
 */

export interface Webhook {
  uuid: string;
  name: string;
  url: string;
  secret_preview: string;
  events: string[];
  enabled: boolean;
  headers: Record<string, string> | null;
  created_at: string;
  updated_at: string;
  last_triggered_at: string | null;
  failure_count: number;
  disabled_reason: string | null;
}

export interface WebhookCreated {
  uuid: string;
  name: string;
  url: string;
  secret: string; // Full secret - only shown once!
  events: string[];
}

export interface CreateWebhookRequest {
  name: string;
  url: string;
  events: string[];
  headers?: Record<string, string>;
}

export interface UpdateWebhookRequest {
  name?: string;
  url?: string;
  events?: string[];
  enabled?: boolean;
  headers?: Record<string, string>;
  regenerate_secret?: boolean;
}

export interface WebhookDelivery {
  uuid: string;
  event_type: string;
  response_status: number | null;
  duration_ms: number | null;
  error_message: string | null;
  delivered_at: string | null;
  created_at: string;
  attempt_number: number;
}

export interface WebhookEvent {
  value: string;
  /** Fluent key for the event's human label. Consumers resolve
   * this via `t()` or `translate()`. Categories are translated
   * separately (see WebhooksView.vue). */
  labelKey: string;
  /** Fluent key for the category header label. */
  categoryKey: string;
  /** Category slug used for grouping events together in the
   * WEBHOOK_EVENT_CATEGORIES map. Stable across locales. */
  category: string;
}

export const WEBHOOK_EVENTS: WebhookEvent[] = [
  { value: 'ticket.created',        labelKey: 'admin-webhooks-event-ticket-created',        categoryKey: 'admin-webhooks-category-tickets',       category: 'Tickets' },
  { value: 'ticket.updated',        labelKey: 'admin-webhooks-event-ticket-updated',        categoryKey: 'admin-webhooks-category-tickets',       category: 'Tickets' },
  { value: 'ticket.deleted',        labelKey: 'admin-webhooks-event-ticket-deleted',        categoryKey: 'admin-webhooks-category-tickets',       category: 'Tickets' },
  { value: 'ticket.linked',         labelKey: 'admin-webhooks-event-ticket-linked',         categoryKey: 'admin-webhooks-category-tickets',       category: 'Tickets' },
  { value: 'ticket.unlinked',       labelKey: 'admin-webhooks-event-ticket-unlinked',       categoryKey: 'admin-webhooks-category-tickets',       category: 'Tickets' },
  { value: 'comment.added',         labelKey: 'admin-webhooks-event-comment-added',         categoryKey: 'admin-webhooks-category-comments',      category: 'Comments' },
  { value: 'comment.deleted',       labelKey: 'admin-webhooks-event-comment-deleted',       categoryKey: 'admin-webhooks-category-comments',      category: 'Comments' },
  { value: 'attachment.added',      labelKey: 'admin-webhooks-event-attachment-added',      categoryKey: 'admin-webhooks-category-attachments',   category: 'Attachments' },
  { value: 'attachment.deleted',    labelKey: 'admin-webhooks-event-attachment-deleted',    categoryKey: 'admin-webhooks-category-attachments',   category: 'Attachments' },
  { value: 'device.linked',         labelKey: 'admin-webhooks-event-device-linked',         categoryKey: 'admin-webhooks-category-devices',       category: 'Devices' },
  { value: 'device.unlinked',       labelKey: 'admin-webhooks-event-device-unlinked',       categoryKey: 'admin-webhooks-category-devices',       category: 'Devices' },
  { value: 'device.updated',        labelKey: 'admin-webhooks-event-device-updated',        categoryKey: 'admin-webhooks-category-devices',       category: 'Devices' },
  { value: 'project.assigned',      labelKey: 'admin-webhooks-event-project-assigned',      categoryKey: 'admin-webhooks-category-projects',      category: 'Projects' },
  { value: 'project.unassigned',    labelKey: 'admin-webhooks-event-project-unassigned',    categoryKey: 'admin-webhooks-category-projects',      category: 'Projects' },
  { value: 'documentation.updated', labelKey: 'admin-webhooks-event-documentation-updated', categoryKey: 'admin-webhooks-category-documentation', category: 'Documentation' },
  { value: 'user.created',          labelKey: 'admin-webhooks-event-user-created',          categoryKey: 'admin-webhooks-category-users',         category: 'Users' },
  { value: 'user.updated',          labelKey: 'admin-webhooks-event-user-updated',          categoryKey: 'admin-webhooks-category-users',         category: 'Users' },
  { value: 'user.deleted',          labelKey: 'admin-webhooks-event-user-deleted',          categoryKey: 'admin-webhooks-category-users',         category: 'Users' },
];

// Group events by category
export const WEBHOOK_EVENT_CATEGORIES = WEBHOOK_EVENTS.reduce((acc, event) => {
  if (!acc[event.category]) {
    acc[event.category] = [];
  }
  acc[event.category].push(event);
  return acc;
}, {} as Record<string, WebhookEvent[]>);
