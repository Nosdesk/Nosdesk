import { Plugin, PluginKey, EditorState } from "prosemirror-state";
import { EditorView } from "prosemirror-view";
import type { Mark } from "prosemirror-model";

export const linkTooltipPluginKey = new PluginKey("linkTooltip");

export interface LinkTooltipState {
  visible: boolean;
  url: string;
  x: number;
  y: number;
  isEditing: boolean;
  from: number;
  to: number;
}

// Helper to get link mark at current selection
export function getLinkMarkAtSelection(state: EditorState): { mark: Mark | null; from: number; to: number } {
  const { selection, doc, schema } = state;
  const { $from, $to } = selection;

  // Check if cursor is inside a link mark
  const linkMark = $from.marks().find((mark) => mark.type === schema.marks.link);

  if (linkMark) {
    // Find the extent of the link
    let from = $from.pos;
    let to = $to.pos;

    // Search backward for link start
    doc.nodesBetween(Math.max(0, from - 50), from, (node, pos) => {
      if (node.isText) {
        const marks = node.marks.filter((m) => m.type === schema.marks.link && m.attrs.href === linkMark.attrs.href);
        if (marks.length > 0) {
          from = Math.min(from, pos);
        }
      }
    });

    // Search forward for link end
    doc.nodesBetween(to, Math.min(doc.content.size, to + 50), (node, pos) => {
      if (node.isText) {
        const marks = node.marks.filter((m) => m.type === schema.marks.link && m.attrs.href === linkMark.attrs.href);
        if (marks.length > 0) {
          to = Math.max(to, pos + node.nodeSize);
        }
      }
    });

    return { mark: linkMark, from, to };
  }

  return { mark: null, from: selection.from, to: selection.to };
}

// Create the link tooltip plugin
export function createLinkTooltipPlugin(callbacks: {
  onStateChange: (state: LinkTooltipState) => void;
}): Plugin {
  return new Plugin({
    key: linkTooltipPluginKey,
    state: {
      init(): LinkTooltipState {
        return {
          visible: false,
          url: "",
          x: 0,
          y: 0,
          isEditing: false,
          from: 0,
          to: 0,
        };
      },
      apply(tr, oldState): LinkTooltipState {
        // Get meta command from transaction
        const meta = tr.getMeta(linkTooltipPluginKey);

        if (meta?.command === "show") {
          return {
            ...oldState,
            ...meta.state,
            visible: true,
          };
        }

        if (meta?.command === "hide") {
          return {
            ...oldState,
            visible: false,
          };
        }

        return oldState;
      },
    },
    view(_editorView: EditorView) {
      return {
        update(view: EditorView, _prevState: EditorState) {
          // Get plugin state
          const pluginState = linkTooltipPluginKey.getState(view.state);

          // If tooltip is explicitly shown via command, just pass through
          if (pluginState?.visible) {
            callbacks.onStateChange(pluginState);
            return;
          }

          // Check if cursor is in a link
          const { mark, from, to } = getLinkMarkAtSelection(view.state);

          if (mark) {
            // Cursor is in a link - show tooltip
            const start = view.coordsAtPos(from);
            const end = view.coordsAtPos(to);

            const x = (start.left + end.left) / 2;
            const y = end.bottom + 8;

            callbacks.onStateChange({
              visible: true,
              url: mark.attrs.href || "",
              x,
              y,
              isEditing: false,
              from,
              to,
            });
          } else {
            // No link - hide tooltip
            callbacks.onStateChange({
              visible: false,
              url: "",
              x: 0,
              y: 0,
              isEditing: false,
              from: 0,
              to: 0,
            });
          }
        },
        destroy() {
          callbacks.onStateChange({
            visible: false,
            url: "",
            x: 0,
            y: 0,
            isEditing: false,
            from: 0,
            to: 0,
          });
        },
      };
    },
  });
}

// Commands to control the tooltip
export function showLinkTooltip(isEditing: boolean = false) {
  return (state: EditorState, dispatch?: (tr: any) => void, view?: EditorView) => {
    if (!dispatch || !view) return false;

    const { from, to } = state.selection;
    const { mark } = getLinkMarkAtSelection(state);

    // Get position for tooltip
    const coords = view.coordsAtPos(from);

    const tr = state.tr.setMeta(linkTooltipPluginKey, {
      command: "show",
      state: {
        url: mark?.attrs.href || "",
        x: coords.left,
        y: coords.bottom + 8,
        isEditing,
        from,
        to,
      },
    });

    dispatch(tr);
    return true;
  };
}

export function hideLinkTooltip() {
  return (state: EditorState, dispatch?: (tr: any) => void) => {
    if (!dispatch) return false;
    dispatch(state.tr.setMeta(linkTooltipPluginKey, { command: "hide" }));
    return true;
  };
}

// Command to apply a link
export function applyLink(href: string) {
  return (state: EditorState, dispatch?: (tr: any) => void) => {
    if (!dispatch) return false;

    const { from, to } = state.selection;
    const linkType = state.schema.marks.link;

    if (!linkType) return false;

    const tr = state.tr;

    // Remove any existing link marks in the selection
    tr.removeMark(from, to, linkType);

    // Add the new link mark
    tr.addMark(from, to, linkType.create({ href }));

    // Hide the tooltip
    tr.setMeta(linkTooltipPluginKey, { command: "hide" });

    dispatch(tr);
    return true;
  };
}

// Command to remove a link
export function removeLink() {
  return (state: EditorState, dispatch?: (tr: any) => void) => {
    if (!dispatch) return false;

    const { mark, from, to } = getLinkMarkAtSelection(state);

    if (!mark) return false;

    const tr = state.tr;
    tr.removeMark(from, to, state.schema.marks.link);
    tr.setMeta(linkTooltipPluginKey, { command: "hide" });

    dispatch(tr);
    return true;
  };
}

/**
 * Paste-to-link: pasting a URL over a text selection turns the selection into
 * a link instead of replacing it (the Notion/Slack/Figma convention). With no
 * selection the paste falls through untouched — the ticket-link plugin and
 * plain text insertion behave as before. Register BEFORE plugins that also
 * handle URL pastes.
 */
export function createPasteLinkPlugin() {
  return new Plugin({
    props: {
      handlePaste(view, event) {
        const text = event.clipboardData?.getData("text/plain")?.trim();
        if (!text || !/^https?:\/\/\S+$/.test(text)) return false;
        const { from, to, empty } = view.state.selection;
        if (empty) return false;
        const linkType = view.state.schema.marks.link;
        if (!linkType) return false;
        const tr = view.state.tr;
        tr.removeMark(from, to, linkType);
        tr.addMark(from, to, linkType.create({ href: text }));
        view.dispatch(tr);
        return true;
      },
    },
  });
}
