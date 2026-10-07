import { Plugin, PluginKey } from 'prosemirror-state'
import { EditorView } from 'prosemirror-view'
import type { NodeView } from 'prosemirror-view'
import { Node as ProseMirrorNode } from 'prosemirror-model'
import { InputRule } from 'prosemirror-inputrules'
import { getTicketById } from '@nosdesk/core/services/ticketService'
import { parseInertHtml } from '@nosdesk/core/utils/inertHtml'
import { translate } from '@/i18n'
import { shareableTicketUrl } from '@/utils/shareUrl'
import {
  knownTicketNumber,
  pooledTicketIdForNumber,
  pooledTicketNumber,
  ticketIdFromUrl,
  ticketNumber,
  ticketPathForId,
} from '@/utils/ticketNumbers'
import {
  type TicketCardData,
  renderTicketCardHtml
} from './ticketCardRenderer'

export const ticketLinkPluginKey = new PluginKey('ticketLink')

// Cache for ticket data to avoid repeated API calls
const ticketCache = new Map<number, TicketCardData>()

// Navigation callback - set by the component that creates the plugin
let navigateToTicket: ((ticketId: number) => void) | null = null

// A link's node stores the ticket's id; its href may predate ticket numbers
// (and carry the id), so navigation always goes by the id.
function openTicket(ticketId: number) {
  if (navigateToTicket) {
    navigateToTicket(ticketId)
  } else {
    import('@/router').then(({ default: router }) => router.push(ticketPathForId(ticketId)))
  }
}

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
    number: knownTicketNumber({ id: ticketId }),
    title: translate('editor-loading', undefined, 'Loading...'),
    priority: '',
    loading: true
  }
  ticketCache.set(ticketId, loadingData)

  try {
    const ticket = await getTicketById(ticketId)
    const data: TicketCardData = {
      id: ticket.id,
      number: ticketNumber(ticket),
      title: ticket.title,
      category: ticket.workflow_state?.category,
      priority: ticket.priority,
      requester: ticket.requester_user?.name || ticket.requester || undefined,
      assignee: ticket.assignee_user?.name || ticket.assignee || undefined,
      loading: false
    }
    ticketCache.set(ticketId, data)
    return data
  } catch (err) {
    console.error(`Failed to fetch ticket ${ticketId}:`, err)
    const number = pooledTicketNumber(ticketId)
    const errorData: TicketCardData = {
      id: ticketId,
      number,
      title:
        number !== undefined
          ? translate('editor-ticket-link-not-found', { id: number }, `Ticket #${number} not found`)
          : translate('editor-ticket-link-unavailable', undefined, 'Ticket not found'),
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

  constructor(node: ProseMirrorNode, _view: EditorView, _getPos: () => number | undefined) {
    this.ticketId = parseInt(node.attrs.ticketId, 10)

    // Create the card element
    this.dom = document.createElement('span')
    this.dom.className = 'ticket-link-card'
    this.dom.contentEditable = 'false'
    this.dom.setAttribute('data-ticket-link', 'true')
    this.dom.setAttribute('data-ticket-id', String(this.ticketId))

    // Initial loading state
    this.render({
      id: this.ticketId,
      number: knownTicketNumber({ id: this.ticketId }),
      title: translate('editor-loading', undefined, 'Loading...'),
      priority: '',
      loading: true
    })

    // Fetch ticket data and update
    this.loadTicketData()

    this.dom.addEventListener('click', (e) => {
      e.preventDefault()
      e.stopPropagation()
      openTicket(this.ticketId)
    })
  }

  private async loadTicketData() {
    const data = await fetchTicketData(this.ticketId)
    this.render(data)
  }

  private render(data: TicketCardData) {
    this.dom.className = `ticket-link-card ${data.loading ? 'ticket-link-loading' : ''} ${data.error ? 'ticket-link-error' : ''}`
    this.dom.innerHTML = renderTicketCardHtml(data)
  }

  update(node: ProseMirrorNode): boolean {
    if (node.type.name !== 'ticket_link') return false
    const newTicketId = parseInt(node.attrs.ticketId, 10)
    if (newTicketId !== this.ticketId) {
      this.ticketId = newTicketId
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

/** The id of the ticket a pasted or dropped URL names, if it's this workspace's. */
export function parseTicketUrl(url: string): number | null {
  return ticketIdFromUrl(url)
}

// Create input rule to convert pasted/typed ticket URLs
export function createTicketLinkInputRule(schema: any): InputRule {
  // Match ticket URL at end of input (when user types or pastes)
  const urlPattern = /https?:\/\/[^\s\/]+\/[^\s]*tickets\/[^\s]+\s$/

  return new InputRule(urlPattern, (state, match, start, end) => {
    const href = match[0].trim()
    const ticketId = ticketIdFromUrl(href)
    if (ticketId === null) return null

    const ticketLinkType = schema.nodes.ticket_link
    if (!ticketLinkType) return null

    const node = ticketLinkType.create({ ticketId: String(ticketId), href })
    return state.tr.replaceWith(start, end, node)
  })
}

/**
 * `#123` followed by a space becomes a ticket link when the workspace pool
 * knows ticket number 123. Unknown numbers ("PO #4521") stay text. Only fires
 * after whitespace or at the start of the block, like the `#` picker.
 */
export function createTicketNumberInputRule(schema: any): InputRule {
  return new InputRule(/(^|\s)#(\d{1,9})\s$/, (state, match, start, end) => {
    const ticketLinkType = schema.nodes.ticket_link
    if (!ticketLinkType) return null
    const number = parseInt(match[2], 10)
    const ticketId = pooledTicketIdForNumber(number)
    if (ticketId === undefined) return null
    const from = start + match[1].length
    const node = ticketLinkType.create({
      ticketId: String(ticketId),
      href: shareableTicketUrl({ id: ticketId, number })
    })
    return state.tr.replaceWith(from, end, node).insertText(' ', from + 1)
  })
}

/**
 * Ticket ids referenced by `ticket_link` nodes in composer HTML, deduplicated.
 * Mirrors the backend's `parse_ticket_references`; the suggestion strip uses
 * it to know what a just-posted comment mentioned.
 */
export function referencedTicketIds(html: string): number[] {
  const container = parseInertHtml(html)
  const ids = new Set<number>()
  container.querySelectorAll('[data-ticket-link][data-ticket-id]').forEach((el) => {
    const id = parseInt(el.getAttribute('data-ticket-id') ?? '', 10)
    if (Number.isInteger(id) && id > 0) ids.add(id)
  })
  return [...ids]
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
      handlePaste(view, event, _slice) {
        const text = event.clipboardData?.getData('text/plain')
        if (!text) return false

        const ticketId = parseTicketUrl(text.trim())
        if (!ticketId) return false

        // Check if it's just a URL (not part of larger content)
        const lines = text.trim().split('\n')
        if (lines.length > 1) return false

        const { schema, tr } = view.state
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
                href = shareableTicketUrl({ id: data.ticketId, number: pooledTicketNumber(data.ticketId) })
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

    if (!ticketIdStr) return

    const ticketId = parseInt(ticketIdStr, 10)
    if (isNaN(ticketId)) return

    // Mark as enhanced to prevent re-processing
    element.setAttribute('data-enhanced', 'true')
    element.className = 'ticket-link-card'

    // Show loading state
    element.innerHTML = renderTicketCardHtml({
      id: ticketId,
      number: knownTicketNumber({ id: ticketId }),
      title: translate('editor-loading', undefined, 'Loading...'),
      loading: true
    })

    // Fetch and render full ticket data
    const data = await fetchTicketData(ticketId)

    element.className = `ticket-link-card ${data.loading ? 'ticket-link-loading' : ''} ${data.error ? 'ticket-link-error' : ''}`
    element.innerHTML = renderTicketCardHtml(data)

    // Add click handler for navigation
    element.style.cursor = 'pointer'
    element.addEventListener('click', (e) => {
      e.preventDefault()
      e.stopPropagation()
      openTicket(ticketId)
    })
  })
}
