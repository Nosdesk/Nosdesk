import { Plugin, PluginKey } from 'prosemirror-state'
import { Decoration, DecorationSet } from 'prosemirror-view'
import { useTicketDrag } from '@/composables/useTicketDrag'
import { renderTicketCardHtml, renderTicketSkeletonHtml } from './ticketCardRenderer'

export const ticketDropIndicatorKey = new PluginKey('ticketDropIndicator')

interface DropIndicatorState {
  active: boolean
  pos: number | null
}

/**
 * Create the drop preview element showing actual ticket data
 */
function createDropPreviewElement(): HTMLElement {
  const { dragState } = useTicketDrag()
  const ticket = dragState.value.ticket

  const el = document.createElement('div')
  el.className = 'ticket-drop-preview'

  if (ticket) {
    el.innerHTML = renderTicketCardHtml(ticket, 'ticket-drop-preview')
  } else {
    el.innerHTML = renderTicketSkeletonHtml('ticket-drop-preview')
  }

  return el
}

/**
 * ProseMirror plugin that shows a visual drop indicator when dragging
 * ticket cards over the editor. Shows a preview of the actual ticket
 * that will be inserted.
 */
export function createTicketDropIndicatorPlugin(): Plugin<DropIndicatorState> {
  return new Plugin<DropIndicatorState>({
    key: ticketDropIndicatorKey,

    state: {
      init(): DropIndicatorState {
        return { active: false, pos: null }
      },

      apply(tr, state): DropIndicatorState {
        const meta = tr.getMeta(ticketDropIndicatorKey)
        if (meta !== undefined) {
          return meta
        }
        return state
      }
    },

    props: {
      decorations(state) {
        const pluginState = ticketDropIndicatorKey.getState(state)

        if (!pluginState?.active || pluginState.pos === null) {
          return DecorationSet.empty
        }

        const indicator = Decoration.widget(
          pluginState.pos,
          createDropPreviewElement,
          { side: 0, key: 'ticket-drop-indicator' }
        )

        return DecorationSet.create(state.doc, [indicator])
      },

      handleDOMEvents: {
        dragover(view, event) {
          const types = event.dataTransfer?.types || []
          const hasTicketData = types.includes('application/json') || types.includes('text/uri-list')

          if (!hasTicketData) {
            return false
          }

          event.preventDefault()
          event.dataTransfer!.dropEffect = 'copy'

          const coords = view.posAtCoords({ left: event.clientX, top: event.clientY })
          if (!coords) return true

          // Snap to block boundaries (start or end of line)
          const $pos = view.state.doc.resolve(coords.pos)
          const parentStart = $pos.start($pos.depth)
          const parentEnd = $pos.end($pos.depth)

          // Snap to whichever boundary is closer
          const distToStart = coords.pos - parentStart
          const distToEnd = parentEnd - coords.pos
          const snappedPos = distToStart <= distToEnd ? parentStart : parentEnd

          const currentState = ticketDropIndicatorKey.getState(view.state)

          if (!currentState?.active || currentState.pos !== snappedPos) {
            const tr = view.state.tr.setMeta(ticketDropIndicatorKey, {
              active: true,
              pos: snappedPos
            })
            view.dispatch(tr)
          }

          return false
        },

        dragleave(view, event) {
          const relatedTarget = event.relatedTarget as Node | null
          const editorDom = view.dom as HTMLElement

          if (relatedTarget && editorDom.contains(relatedTarget)) {
            return false
          }

          const currentState = ticketDropIndicatorKey.getState(view.state)
          if (currentState?.active) {
            const tr = view.state.tr.setMeta(ticketDropIndicatorKey, {
              active: false,
              pos: null
            })
            view.dispatch(tr)
          }

          return false
        },

        drop(view) {
          const currentState = ticketDropIndicatorKey.getState(view.state)
          if (currentState?.active) {
            const tr = view.state.tr.setMeta(ticketDropIndicatorKey, {
              active: false,
              pos: null
            })
            view.dispatch(tr)
          }
          return false
        },

        dragend(view) {
          const currentState = ticketDropIndicatorKey.getState(view.state)
          if (currentState?.active) {
            const tr = view.state.tr.setMeta(ticketDropIndicatorKey, {
              active: false,
              pos: null
            })
            view.dispatch(tr)
          }
          return false
        }
      }
    }
  })
}
