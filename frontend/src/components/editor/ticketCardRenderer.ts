/**
 * Shared utilities for rendering ticket card HTML in ProseMirror plugins.
 * Used by both ticketLinkPlugin and ticketDropIndicatorPlugin.
 */

import { escapeHtml } from '@/utils/escape';

export interface TicketCardData {
  id: number
  title: string
  status?: string
  priority?: string
  requester?: string | null
  assignee?: string | null
  loading?: boolean
  error?: boolean
}

export function getStatusClass(status?: string, prefix = 'ticket-link'): string {
  switch (status?.toLowerCase()) {
    case 'open':
      return `${prefix}-status-open`
    case 'in-progress':
      return `${prefix}-status-in-progress`
    case 'closed':
      return `${prefix}-status-closed`
    default:
      return ''
  }
}

export function getPriorityClass(priority?: string, prefix = 'ticket-link'): string {
  switch (priority?.toLowerCase()) {
    case 'high':
      return `${prefix}-priority-high`
    case 'medium':
      return `${prefix}-priority-medium`
    case 'low':
      return `${prefix}-priority-low`
    default:
      return ''
  }
}

/**
 * Render the inner HTML for a compact single-row ticket card.
 * Layout: [#ID] [Title] [status dot + label] [priority dot + label]
 */
export function renderTicketCardHtml(data: TicketCardData, classPrefix = 'ticket-link'): string {
  if (data.loading) {
    return `
      <span class="${classPrefix}-id">#${data.id}</span>
      <span class="${classPrefix}-loader"></span>
    `
  }

  const statusText = data.status ? data.status.replace('-', ' ') : ''
  const priorityText = data.priority
    ? data.priority.charAt(0).toUpperCase() + data.priority.slice(1)
    : ''

  return `
    <span class="${classPrefix}-id">#${data.id}</span>
    <span class="${classPrefix}-title">${escapeHtml(data.title)}</span>
    ${data.status ? `<span class="${classPrefix}-status ${getStatusClass(data.status, classPrefix)}"><span class="${classPrefix}-dot"></span>${statusText}</span>` : ''}
    ${data.priority ? `<span class="${classPrefix}-priority ${getPriorityClass(data.priority, classPrefix)}"><span class="${classPrefix}-dot"></span>${priorityText}</span>` : ''}
  `
}

/**
 * Render skeleton HTML for when ticket data is not available.
 */
export function renderTicketSkeletonHtml(classPrefix = 'ticket-drop-preview'): string {
  return `
    <span class="${classPrefix}-id">#---</span>
    <span class="${classPrefix}-title ${classPrefix}-skeleton"></span>
  `
}
