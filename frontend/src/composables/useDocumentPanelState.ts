/**
 * Cross-component state for which side panel (insights / history)
 * is currently open on the active documentation page. Lets the
 * sidebar's context menu signal "open the insights panel" without
 * round-tripping through the URL — the URL is for what you're
 * looking at, not which transient drawer is on screen.
 *
 * Module-level ref deliberately, rather than a Pinia store: the
 * surface is one piece of state and two mutators, and a store
 * would just add ceremony. The exported composable returns a
 * readonly view + typed mutators so consumers can't accidentally
 * write to the ref directly.
 */
import { readonly, ref } from 'vue'

export type DocumentPanel = 'insights' | 'history'

const activePanel = ref<DocumentPanel | null>(null)

export function useDocumentPanelState() {
  return {
    activePanel: readonly(activePanel),
    open: (panel: DocumentPanel) => {
      activePanel.value = panel
    },
    close: () => {
      activePanel.value = null
    },
  }
}
