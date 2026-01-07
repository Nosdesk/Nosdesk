/**
 * Shared utilities for rendering ticket card HTML in ProseMirror plugins.
 * Used by both ticketLinkPlugin and ticketDropIndicatorPlugin.
 */

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

export function escapeHtml(text: string): string {
  const div = document.createElement('div')
  div.textContent = text
  return div.innerHTML
}

/**
 * Render the inner HTML for a ticket card.
 * @param data - Ticket data to render
 * @param classPrefix - CSS class prefix ('ticket-link' or 'ticket-drop-preview')
 */
export function renderTicketCardHtml(data: TicketCardData, classPrefix = 'ticket-link'): string {
  if (data.loading) {
    return `
      <div class="${classPrefix}-header">
        <span class="${classPrefix}-id">#${data.id}</span>
        <span class="${classPrefix}-loader"></span>
      </div>
      <div class="${classPrefix}-title">Loading...</div>
    `
  }

  const statusText = data.status ? data.status.replace('-', ' ') : ''
  const priorityText = data.priority
    ? data.priority.charAt(0).toUpperCase() + data.priority.slice(1)
    : ''

  return `
    <div class="${classPrefix}-header">
      <span class="${classPrefix}-id">#${data.id}</span>
      <span class="${classPrefix}-title">${escapeHtml(data.title)}</span>
    </div>
    <div class="${classPrefix}-meta">
      ${data.requester ? `<span class="${classPrefix}-person"><span class="${classPrefix}-label">From:</span> ${escapeHtml(data.requester)}</span>` : ''}
      ${data.assignee ? `<span class="${classPrefix}-person"><span class="${classPrefix}-label">To:</span> ${escapeHtml(data.assignee)}</span>` : ''}
      ${data.status ? `<span class="${classPrefix}-status ${getStatusClass(data.status, classPrefix)}">${statusText}</span>` : ''}
      ${data.priority ? `<span class="${classPrefix}-priority ${getPriorityClass(data.priority, classPrefix)}">${priorityText}</span>` : ''}
    </div>
  `
}

/**
 * Render skeleton HTML for when ticket data is not available.
 */
export function renderTicketSkeletonHtml(classPrefix = 'ticket-drop-preview'): string {
  return `
    <div class="${classPrefix}-header">
      <span class="${classPrefix}-id">#---</span>
      <span class="${classPrefix}-title ${classPrefix}-skeleton"></span>
    </div>
    <div class="${classPrefix}-meta">
      <span class="${classPrefix}-skeleton" style="width: 60px;"></span>
      <span class="${classPrefix}-skeleton" style="width: 50px;"></span>
    </div>
  `
}
