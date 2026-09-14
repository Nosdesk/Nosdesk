/**
 * ProseMirror Mentions Plugin
 *
 * Provides trigger-character pickers (`@` for people, `#` for tickets):
 * - Detects the trigger and tracks query text
 * - Provides position info for dropdown placement
 * - Handles keyboard navigation (must be before baseKeymap)
 * - Replaces the typed trigger + query with the chosen node
 */
import { Plugin, PluginKey } from 'prosemirror-state';
import { Decoration, DecorationSet } from 'prosemirror-view';
import { keymap } from 'prosemirror-keymap';
import type { EditorView } from 'prosemirror-view';
import type { Node as ProseMirrorNode, NodeType } from 'prosemirror-model';
import type { Command } from 'prosemirror-state';

export interface MentionUser {
  uuid: string;
  name: string;
  email?: string;
  avatar_url?: string | null;
  avatar_thumb?: string | null;
}

/** `@` opens the people picker, `#` the ticket picker. */
export type MentionTrigger = '@' | '#';

export interface MentionState {
  active: boolean;
  trigger: MentionTrigger;
  query: string;
  from: number;
  to: number;
  position: { top: number; left: number } | null;
}

const INACTIVE: MentionState = { active: false, trigger: '@', query: '', from: 0, to: 0, position: null };

export type MentionKey = 'ArrowUp' | 'ArrowDown' | 'Enter' | 'Tab' | 'Escape';

export interface MentionPluginOptions {
  /** Called when mention state changes (for showing/hiding dropdown) */
  onStateChange?: (state: MentionState) => void;
  /** Called when a navigation key is pressed. Return true if handled. */
  onKeyDown?: (key: MentionKey) => boolean;
  /** Which triggers open a picker. Defaults to `@` only. */
  triggers?: MentionTrigger[];
}

export const mentionPluginKey = new PluginKey<MentionState>('mentions');

/**
 * Find the nearest trigger before the cursor. A trigger counts only at
 * the start of the block or after whitespace, so `a@b.c` and `/#anchor`
 * never open a picker.
 */
function findMentionTrigger(
  text: string,
  cursorPos: number,
  triggers: MentionTrigger[]
): { start: number; query: string; trigger: MentionTrigger } | null {
  const textBeforeCursor = text.slice(0, cursorPos);
  let start = -1;
  let trigger: MentionTrigger = '@';
  for (const candidate of triggers) {
    const idx = textBeforeCursor.lastIndexOf(candidate);
    if (idx > start) {
      start = idx;
      trigger = candidate;
    }
  }
  if (start === -1) return null;

  const charBefore = start > 0 ? textBeforeCursor[start - 1] : ' ';
  if (charBefore !== ' ' && charBefore !== '\n' && start !== 0) {
    return null;
  }

  const query = textBeforeCursor.slice(start + 1);

  // Query should be a single word (no spaces) and reasonable length
  if (query.includes(' ') || query.length > 50) {
    return null;
  }

  return { start, query, trigger };
}

/**
 * Get cursor position in screen coordinates
 */
function getCursorPosition(view: EditorView, pos: number): { top: number; left: number } | null {
  try {
    const coords = view.coordsAtPos(pos);
    return { top: coords.bottom, left: coords.left };
  } catch {
    return null;
  }
}

/**
 * Create the mention state tracking plugin
 */
function createMentionStatePlugin(options: MentionPluginOptions): Plugin {
  const triggers = options.triggers ?? ['@'];
  return new Plugin<MentionState>({
    key: mentionPluginKey,

    state: {
      init(): MentionState {
        return INACTIVE;
      },

      apply(tr, prev, _oldState, newState): MentionState {
        // Check if this is a mention close action
        if (tr.getMeta(mentionPluginKey)?.type === 'close') {
          return INACTIVE;
        }

        // Only check for mentions if document or selection changed
        if (!tr.docChanged && !tr.selectionSet) {
          return prev;
        }

        const { selection } = newState;
        const { $from } = selection;

        // Only handle cursor (not range) selections in text nodes
        if (!selection.empty || !$from.parent.isTextblock) {
          return prev.active ? INACTIVE : prev;
        }

        // Get text content up to cursor and find the trigger
        const textBefore = $from.parent.textBetween(0, $from.parentOffset, undefined, '\ufffc');
        const trigger = findMentionTrigger(textBefore, $from.parentOffset, triggers);

        if (!trigger) {
          return prev.active ? INACTIVE : prev;
        }

        // Calculate absolute positions
        const blockStart = $from.start();
        return {
          active: true,
          trigger: trigger.trigger,
          query: trigger.query,
          from: blockStart + trigger.start,
          to: $from.pos,
          position: null, // Set in view update
        };
      },
    },

    view(_editorView) {
      return {
        update(view, prevState) {
          const prev = mentionPluginKey.getState(prevState);
          const next = mentionPluginKey.getState(view.state);

          if (!prev || !next) return;

          // Notify on state change
          if (prev.active !== next.active || prev.query !== next.query || prev.trigger !== next.trigger) {
            const position = next.active ? getCursorPosition(view, next.to) : null;
            options.onStateChange?.({ ...next, position });
          }
        },
        destroy() {},
      };
    },

    props: {
      decorations(state) {
        const mentionState = mentionPluginKey.getState(state);
        if (!mentionState?.active) return DecorationSet.empty;

        return DecorationSet.create(state.doc, [
          Decoration.inline(mentionState.from, mentionState.to, {
            class: 'mention-typing',
          }),
        ]);
      },
    },
  });
}

/**
 * Create the mention keymap plugin for navigation keys.
 * Returns false when mention is inactive so other handlers can process.
 */
function createMentionKeymapPlugin(onKeyDown?: (key: MentionKey) => boolean): Plugin {
  const createCommand = (key: MentionKey): Command => (state) => {
    const mentionState = mentionPluginKey.getState(state);
    if (!mentionState?.active) return false;
    return onKeyDown?.(key) ?? false;
  };

  return keymap({
    ArrowUp: createCommand('ArrowUp'),
    ArrowDown: createCommand('ArrowDown'),
    Enter: createCommand('Enter'),
    Tab: createCommand('Tab'),
    Escape: createCommand('Escape'),
  });
}

/**
 * Create all mention plugins. Returns an array of plugins that should be
 * spread into your plugins array BEFORE baseKeymap.
 *
 * @example
 * plugins: [
 *   ...createMentionPlugins({ onStateChange, onKeyDown }),
 *   keymap(baseKeymap),
 *   // ... other plugins
 * ]
 */
export function createMentionPlugins(options: MentionPluginOptions = {}): Plugin[] {
  return [
    // Keymap must come first to intercept keys before baseKeymap
    createMentionKeymapPlugin(options.onKeyDown),
    // State tracking plugin
    createMentionStatePlugin(options),
  ];
}

// Legacy export for backwards compatibility
export const createMentionsPlugin = createMentionStatePlugin;

/**
 * Replace the active trigger + query with `node`, followed by a space,
 * and close the picker.
 */
function replaceActiveTrigger(view: EditorView, node: ProseMirrorNode): void {
  const state = mentionPluginKey.getState(view.state);
  if (!state?.active) return;

  const { from, to } = state;
  let tr = view.state.tr;
  tr = tr.replaceWith(from, to, node);
  tr = tr.insertText(' ', from + 1);
  tr = tr.setMeta(mentionPluginKey, { type: 'close' });

  view.dispatch(tr);
  view.focus();
}

/**
 * Insert a mention into the editor
 */
export function insertMention(
  view: EditorView,
  user: MentionUser,
  mentionNodeType: NodeType
): void {
  replaceActiveTrigger(
    view,
    mentionNodeType.create({
      uuid: user.uuid,
      name: user.name,
      avatarUrl: user.avatar_thumb || user.avatar_url || null,
    })
  );
}

/**
 * Insert a ticket reference (a `ticket_link` node) in place of the `#` query.
 */
export function insertTicketReference(
  view: EditorView,
  ticket: { id: number; href: string },
  ticketLinkNodeType: NodeType
): void {
  replaceActiveTrigger(
    view,
    ticketLinkNodeType.create({ ticketId: String(ticket.id), href: ticket.href })
  );
}

/**
 * Close the mention dropdown without inserting
 */
export function closeMention(view: EditorView): void {
  view.dispatch(view.state.tr.setMeta(mentionPluginKey, { type: 'close' }));
}
