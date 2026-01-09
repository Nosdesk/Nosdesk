/**
 * ProseMirror Placeholder Plugin
 *
 * Displays placeholder text when the editor is empty.
 * Uses a data-attribute approach with CSS ::before pseudo-element.
 *
 * Based on: https://gist.github.com/amk221/1f9657e92e003a3725aaa4cf86a07cc0
 */
import { Plugin } from 'prosemirror-state';
import type { EditorView } from 'prosemirror-view';

function isDocEmpty(view: EditorView): boolean {
  const { doc } = view.state;
  // Check if document has no text content
  if (doc.textContent) {
    return false;
  }
  // Also check if first child has content (for block nodes)
  if (doc.firstChild && doc.firstChild.content.size > 0) {
    return false;
  }
  return true;
}

export function createPlaceholderPlugin(text: string): Plugin {
  const update = (view: EditorView) => {
    if (isDocEmpty(view)) {
      view.dom.setAttribute('data-placeholder', text);
    } else {
      view.dom.removeAttribute('data-placeholder');
    }
  };

  return new Plugin({
    view(view) {
      update(view);
      return { update };
    },
  });
}

export default createPlaceholderPlugin;
