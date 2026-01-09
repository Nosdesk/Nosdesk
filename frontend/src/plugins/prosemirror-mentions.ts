/**
 * ProseMirror Mentions Plugin
 *
 * Provides @mention functionality:
 * - Detects @ trigger and tracks query text
 * - Provides position info for dropdown placement
 * - Handles keyboard navigation (must be before baseKeymap)
 * - Inserts mention nodes when user is selected
 */
import { Plugin, PluginKey } from 'prosemirror-state';
import { Decoration, DecorationSet } from 'prosemirror-view';
import { keymap } from 'prosemirror-keymap';
import type { EditorView } from 'prosemirror-view';
import type { NodeType } from 'prosemirror-model';
import type { Command } from 'prosemirror-state';

export interface MentionUser {
  uuid: string;
  name: string;
  email?: string;
  avatar_url?: string | null;
  avatar_thumb?: string | null;
}

export interface MentionState {
  active: boolean;
  query: string;
  from: number;
  to: number;
  position: { top: number; left: number } | null;
}

export type MentionKey = 'ArrowUp' | 'ArrowDown' | 'Enter' | 'Tab' | 'Escape';

export interface MentionPluginOptions {
  /** Called when mention state changes (for showing/hiding dropdown) */
  onStateChange?: (state: MentionState) => void;
  /** Called when a navigation key is pressed. Return true if handled. */
  onKeyDown?: (key: MentionKey) => boolean;
}

export const mentionPluginKey = new PluginKey<MentionState>('mentions');

/**
 * Find the @ trigger position before cursor
 */
function findMentionTrigger(
  text: string,
  cursorPos: number
): { start: number; query: string } | null {
  const textBeforeCursor = text.slice(0, cursorPos);
  const lastAtIndex = textBeforeCursor.lastIndexOf('@');

  if (lastAtIndex === -1) return null;

  // Check if @ is at start or preceded by whitespace
  const charBefore = lastAtIndex > 0 ? textBeforeCursor[lastAtIndex - 1] : ' ';
  if (charBefore !== ' ' && charBefore !== '\n' && lastAtIndex !== 0) {
    return null;
  }

  const query = textBeforeCursor.slice(lastAtIndex + 1);

  // Query should be a single word (no spaces) and reasonable length
  if (query.includes(' ') || query.length > 50) {
    return null;
  }

  return { start: lastAtIndex, query };
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
  return new Plugin<MentionState>({
    key: mentionPluginKey,

    state: {
      init(): MentionState {
        return { active: false, query: '', from: 0, to: 0, position: null };
      },

      apply(tr, prev, _oldState, newState): MentionState {
        // Check if this is a mention close action
        if (tr.getMeta(mentionPluginKey)?.type === 'close') {
          return { active: false, query: '', from: 0, to: 0, position: null };
        }

        // Only check for mentions if document or selection changed
        if (!tr.docChanged && !tr.selectionSet) {
          return prev;
        }

        const { selection } = newState;
        const { $from } = selection;

        // Only handle cursor (not range) selections in text nodes
        if (!selection.empty || !$from.parent.isTextblock) {
          return prev.active
            ? { active: false, query: '', from: 0, to: 0, position: null }
            : prev;
        }

        // Get text content up to cursor and find @ trigger
        const textBefore = $from.parent.textBetween(0, $from.parentOffset, undefined, '\ufffc');
        const trigger = findMentionTrigger(textBefore, $from.parentOffset);

        if (!trigger) {
          return prev.active
            ? { active: false, query: '', from: 0, to: 0, position: null }
            : prev;
        }

        // Calculate absolute positions
        const blockStart = $from.start();
        return {
          active: true,
          query: trigger.query,
          from: blockStart + trigger.start,
          to: $from.pos,
          position: null, // Set in view update
        };
      },
    },

    view(editorView) {
      return {
        update(view, prevState) {
          const prev = mentionPluginKey.getState(prevState);
          const next = mentionPluginKey.getState(view.state);

          if (!prev || !next) return;

          // Notify on state change
          if (prev.active !== next.active || prev.query !== next.query) {
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
 * Insert a mention into the editor
 */
export function insertMention(
  view: EditorView,
  user: MentionUser,
  mentionNodeType: NodeType
): void {
  const state = mentionPluginKey.getState(view.state);
  if (!state?.active) return;

  const { from, to } = state;
  const mentionNode = mentionNodeType.create({
    uuid: user.uuid,
    name: user.name,
    avatarUrl: user.avatar_thumb || user.avatar_url || null,
  });

  let tr = view.state.tr;
  tr = tr.replaceWith(from, to, mentionNode);
  tr = tr.insertText(' ', from + 1);
  tr = tr.setMeta(mentionPluginKey, { type: 'close' });

  view.dispatch(tr);
  view.focus();
}

/**
 * Close the mention dropdown without inserting
 */
export function closeMention(view: EditorView): void {
  view.dispatch(view.state.tr.setMeta(mentionPluginKey, { type: 'close' }));
}
