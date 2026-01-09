import { Plugin, PluginKey } from 'prosemirror-state'
import { EditorView } from 'prosemirror-view'
import type { NodeView } from 'prosemirror-view'
import { Node as ProseMirrorNode } from 'prosemirror-model'
import { InputRule } from 'prosemirror-inputrules'
import { getTicketById } from '@/services/ticketService'
import {
  isColorBlindMode,
  getStatusIndicatorSvg,
  getPriorityIndicatorSvg
} from '@/utils/indicatorSvg'
import {
  type TicketCardData,
  getStatusClass,
  getPriorityClass,
  escapeHtml,
  renderTicketCardHtml
} from './ticketCardRenderer'

export const ticketLinkPluginKey = new PluginKey('ticketLink')

type TicketStatus = 'open' | 'in-progress' | 'closed'
type TicketPriority = 'low' | 'medium' | 'high'

// Cache for ticket data to avoid repeated API calls
const ticketCache = new Map<number, TicketCardData>()

// Navigation callback - set by the component that creates the plugin
let navigateToTicket: ((ticketId: number) => void) | null = null

export function setTicketNavigationHandler(handler: (ticketId: number) => void) {
  navigateToTicket = handler
}

// Fetch ticket data with caching
async function fetchTicketData(ticketId: number): Promise<TicketCardData> {
  const cached = ticketCache.get(ticketId)
  if (cached && !cached.loading) {
    return cached
  }

  // Set loading state
  const loadingData: TicketCardData = {
    id: ticketId,
    title: 'Loading...',
    status: '',
    priority: '',
    loading: true
  }
  ticketCache.set(ticketId, loadingData)

  try {
    const ticket = await getTicketById(ticketId)
    const data: TicketCardData = {
      id: ticket.id,
      title: ticket.title,
      status: ticket.status,
      priority: ticket.priority,
      requester: ticket.requester_user?.name || ticket.requester || undefined,
      assignee: ticket.assignee_user?.name || ticket.assignee || undefined,
      loading: false
    }
    ticketCache.set(ticketId, data)
    return data
  } catch (err) {
    console.error(`Failed to fetch ticket ${ticketId}:`, err)
    const errorData: TicketCardData = {
      id: ticketId,
      title: `Ticket #${ticketId} not found`,
      status: '',
      priority: '',
      error: true
    }
    ticketCache.set(ticketId, errorData)
    return errorData
  }
}

// Custom NodeView for ticket_link nodes
class TicketLinkView implements NodeView {
  dom: HTMLElement
  private ticketId: number
  private href: string

  constructor(node: ProseMirrorNode, view: EditorView, getPos: () => number | undefined) {
    this.ticketId = parseInt(node.attrs.ticketId, 10)
    this.href = node.attrs.href

    // Create the card element
    this.dom = document.createElement('span')
    this.dom.className = 'ticket-link-card'
    this.dom.contentEditable = 'false'
    this.dom.setAttribute('data-ticket-link', 'true')
    this.dom.setAttribute('data-ticket-id', String(this.ticketId))

    // Initial loading state
    this.render({ id: this.ticketId, title: 'Loading...', status: '', priority: '', loading: true })

    // Fetch ticket data and update
    this.loadTicketData()

    // Add click handler - use navigation callback if set, otherwise fall back to URL
    this.dom.addEventListener('click', (e) => {
      e.preventDefault()
      e.stopPropagation()
      if (navigateToTicket) {
        navigateToTicket(this.ticketId)
      } else {
        // Fallback to URL navigation if no handler is set
        window.location.href = this.href
      }
    })
  }

  private async loadTicketData() {
    const data = await fetchTicketData(this.ticketId)
    this.render(data)
  }

  private render(data: TicketCardData) {
    const colorBlindMode = isColorBlindMode()
    const statusClass = getStatusClass(data.status)
    const priorityClass = getPriorityClass(data.priority)

    // Get colorblind-friendly indicators
    const statusIndicator = colorBlindMode && data.status
      ? getStatusIndicatorSvg(data.status.toLowerCase() as TicketStatus)
      : ''
    const priorityIndicator = colorBlindMode && data.priority
      ? getPriorityIndicatorSvg(data.priority.toLowerCase() as TicketPriority)
      : ''

    this.dom.className = `ticket-link-card ${data.loading ? 'ticket-link-loading' : ''} ${data.error ? 'ticket-link-error' : ''}`

    if (data.loading) {
      this.dom.innerHTML = `
        <div class="ticket-link-header">
          <span class="ticket-link-id">#${data.id}</span>
          <span class="ticket-link-loader"></span>
        </div>
        <div class="ticket-link-title">Loading...</div>
      `
      return
    }

    const statusText = data.status ? data.status.replace('-', ' ') : ''
    const priorityText = data.priority ? data.priority.charAt(0).toUpperCase() + data.priority.slice(1) : ''

    this.dom.innerHTML = `
      <div class="ticket-link-header">
        <span class="ticket-link-id">#${data.id}</span>
        <span class="ticket-link-title">${escapeHtml(data.title)}</span>
      </div>
      <div class="ticket-link-meta">
        ${data.requester ? `<span class="ticket-link-person"><span class="ticket-link-label">From:</span> ${escapeHtml(data.requester)}</span>` : ''}
        ${data.assignee ? `<span class="ticket-link-person"><span class="ticket-link-label">To:</span> ${escapeHtml(data.assignee)}</span>` : ''}
        ${data.status ? `<span class="ticket-link-status ${statusClass}">${statusIndicator}${statusText}</span>` : ''}
        ${data.priority ? `<span class="ticket-link-priority ${priorityClass}">${priorityIndicator}${priorityText}</span>` : ''}
      </div>
    `
  }

  update(node: ProseMirrorNode): boolean {
    if (node.type.name !== 'ticket_link') return false
    const newTicketId = parseInt(node.attrs.ticketId, 10)
    if (newTicketId !== this.ticketId) {
      this.ticketId = newTicketId
      this.href = node.attrs.href
      this.loadTicketData()
    }
    return true
  }

  destroy() {
    // Cleanup if needed
  }

  stopEvent() {
    return true // Prevent editor from handling events on this node
  }

  ignoreMutation() {
    return true // Ignore DOM mutations within this node
  }
}

// Regex to match ticket URLs
const TICKET_URL_REGEX = /https?:\/\/[^\/]+\/tickets\/(\d+)(?:\?[^\s]*)?/

// Parse ticket URL and extract ticket ID
export function parseTicketUrl(url: string): number | null {
  const match = url.match(TICKET_URL_REGEX)
  return match ? parseInt(match[1], 10) : null
}

// Create input rule to convert pasted/typed ticket URLs
export function createTicketLinkInputRule(schema: any): InputRule {
  // Match ticket URL at end of input (when user types or pastes)
  const urlPattern = /https?:\/\/[^\s\/]+\/tickets\/(\d+)(?:\?[^\s]*)?\s$/

  return new InputRule(urlPattern, (state, match, start, end) => {
    const ticketId = match[1]
    const href = match[0].trim()

    const ticketLinkType = schema.nodes.ticket_link
    if (!ticketLinkType) return null

    const node = ticketLinkType.create({ ticketId, href })
    return state.tr.replaceWith(start, end, node)
  })
}

// Create the plugin
export function createTicketLinkPlugin(): Plugin {
  return new Plugin({
    key: ticketLinkPluginKey,
    props: {
      nodeViews: {
        ticket_link: (node, view, getPos) => new TicketLinkView(node, view, getPos)
      },
      // Handle paste events to convert ticket URLs
      handlePaste(view, event, slice) {
        const text = event.clipboardData?.getData('text/plain')
        if (!text) return false

        const ticketId = parseTicketUrl(text.trim())
        if (!ticketId) return false

        // Check if it's just a URL (not part of larger content)
        const lines = text.trim().split('\n')
        if (lines.length > 1) return false

        const { schema, tr, selection } = view.state
        const ticketLinkType = schema.nodes.ticket_link

        if (!ticketLinkType) return false

        const node = ticketLinkType.create({
          ticketId: String(ticketId),
          href: text.trim()
        })

        const transaction = tr.replaceSelectionWith(node)
        view.dispatch(transaction)
        return true
      },
      // Handle drop events
      handleDrop(view, event, slice, moved) {
        if (moved) return false // Let normal move handling work

        // Try to get ticket data from multiple sources
        let ticketId: number | null = null
        let href: string | null = null

        // First try: Parse URL from text/plain
        const text = event.dataTransfer?.getData('text/plain')
        if (text) {
          ticketId = parseTicketUrl(text.trim())
          if (ticketId) {
            href = text.trim()
          }
        }

        // Second try: Get ticket ID from application/json (internal drag from RecentTickets)
        if (!ticketId) {
          const jsonData = event.dataTransfer?.getData('application/json')
          if (jsonData) {
            try {
              const data = JSON.parse(jsonData)
              if (data.ticketId) {
                ticketId = data.ticketId
                href = `${window.location.origin}/tickets/${ticketId}`
              }
            } catch {
              // Invalid JSON, ignore
            }
          }
        }

        if (!ticketId || !href) return false

        const { schema, tr } = view.state
        const ticketLinkType = schema.nodes.ticket_link

        if (!ticketLinkType) return false

        // Get drop position
        const coords = view.posAtCoords({ left: event.clientX, top: event.clientY })
        if (!coords) return false

        // Snap to block boundaries (start or end of line)
        const $pos = view.state.doc.resolve(coords.pos)
        const parentStart = $pos.start($pos.depth)
        const parentEnd = $pos.end($pos.depth)
        const distToStart = coords.pos - parentStart
        const distToEnd = parentEnd - coords.pos
        const insertPos = distToStart <= distToEnd ? parentStart : parentEnd

        const node = ticketLinkType.create({
          ticketId: String(ticketId),
          href
        })

        const transaction = tr.insert(insertPos, node)
        view.dispatch(transaction)
        return true
      }
    }
  })
}

/**
 * Enhance ticket link elements in rendered markdown content.
 * Finds [data-ticket-link] elements and converts them to full ticket cards.
 * Used by MarkdownRenderer to display ticket references in comments.
 */
export function enhanceTicketLinks(container: HTMLElement): void {
  const ticketLinks = container.querySelectorAll('[data-ticket-link]:not([data-enhanced])')

  ticketLinks.forEach(async (el) => {
    const element = el as HTMLElement
    const ticketIdStr = element.getAttribute('data-ticket-id')
    const href = element.getAttribute('data-href')

    if (!ticketIdStr) return

    const ticketId = parseInt(ticketIdStr, 10)
    if (isNaN(ticketId)) return

    // Mark as enhanced to prevent re-processing
    element.setAttribute('data-enhanced', 'true')
    element.className = 'ticket-link-card'

    // Show loading state
    element.innerHTML = renderTicketCardHtml({
      id: ticketId,
      title: 'Loading...',
      loading: true
    })

    // Fetch and render full ticket data
    const data = await fetchTicketData(ticketId)

    // Check for colorblind mode indicators
    const colorBlindMode = isColorBlindMode()
    const statusIndicator = colorBlindMode && data.status
      ? getStatusIndicatorSvg(data.status.toLowerCase() as TicketStatus)
      : ''
    const priorityIndicator = colorBlindMode && data.priority
      ? getPriorityIndicatorSvg(data.priority.toLowerCase() as TicketPriority)
      : ''

    const statusClass = getStatusClass(data.status)
    const priorityClass = getPriorityClass(data.priority)
    const statusText = data.status ? data.status.replace('-', ' ') : ''
    const priorityText = data.priority
      ? data.priority.charAt(0).toUpperCase() + data.priority.slice(1)
      : ''

    element.className = `ticket-link-card ${data.loading ? 'ticket-link-loading' : ''} ${data.error ? 'ticket-link-error' : ''}`
    element.innerHTML = `
      <div class="ticket-link-header">
        <span class="ticket-link-id">#${data.id}</span>
        <span class="ticket-link-title">${escapeHtml(data.title)}</span>
      </div>
      <div class="ticket-link-meta">
        ${data.requester ? `<span class="ticket-link-person"><span class="ticket-link-label">From:</span> ${escapeHtml(data.requester)}</span>` : ''}
        ${data.assignee ? `<span class="ticket-link-person"><span class="ticket-link-label">To:</span> ${escapeHtml(data.assignee)}</span>` : ''}
        ${data.status ? `<span class="ticket-link-status ${statusClass}">${statusIndicator}${statusText}</span>` : ''}
        ${data.priority ? `<span class="ticket-link-priority ${priorityClass}">${priorityIndicator}${priorityText}</span>` : ''}
      </div>
    `

    // Add click handler for navigation
    element.style.cursor = 'pointer'
    element.addEventListener('click', (e) => {
      e.preventDefault()
      e.stopPropagation()
      if (navigateToTicket) {
        navigateToTicket(ticketId)
      } else if (href) {
        window.location.href = href
      }
    })
  })
}
