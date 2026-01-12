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
  label: string;
  category: string;
}

export const WEBHOOK_EVENTS: WebhookEvent[] = [
  { value: 'ticket.created', label: 'Ticket Created', category: 'Tickets' },
  { value: 'ticket.updated', label: 'Ticket Updated', category: 'Tickets' },
  { value: 'ticket.deleted', label: 'Ticket Deleted', category: 'Tickets' },
  { value: 'ticket.linked', label: 'Ticket Linked', category: 'Tickets' },
  { value: 'ticket.unlinked', label: 'Ticket Unlinked', category: 'Tickets' },
  { value: 'comment.added', label: 'Comment Added', category: 'Comments' },
  { value: 'comment.deleted', label: 'Comment Deleted', category: 'Comments' },
  { value: 'attachment.added', label: 'Attachment Added', category: 'Attachments' },
  { value: 'attachment.deleted', label: 'Attachment Deleted', category: 'Attachments' },
  { value: 'device.linked', label: 'Device Linked', category: 'Devices' },
  { value: 'device.unlinked', label: 'Device Unlinked', category: 'Devices' },
  { value: 'device.updated', label: 'Device Updated', category: 'Devices' },
  { value: 'project.assigned', label: 'Project Assigned', category: 'Projects' },
  { value: 'project.unassigned', label: 'Project Unassigned', category: 'Projects' },
  { value: 'documentation.updated', label: 'Documentation Updated', category: 'Documentation' },
  { value: 'user.created', label: 'User Created', category: 'Users' },
  { value: 'user.updated', label: 'User Updated', category: 'Users' },
  { value: 'user.deleted', label: 'User Deleted', category: 'Users' },
];

// Group events by category
export const WEBHOOK_EVENT_CATEGORIES = WEBHOOK_EVENTS.reduce((acc, event) => {
  if (!acc[event.category]) {
    acc[event.category] = [];
  }
  acc[event.category].push(event);
  return acc;
}, {} as Record<string, WebhookEvent[]>);
