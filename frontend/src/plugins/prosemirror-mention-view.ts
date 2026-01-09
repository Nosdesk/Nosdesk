/**
 * ProseMirror Mention NodeView Plugin
 *
 * Renders mention nodes with user avatar and name as inline chips.
 * Also exports utilities for enhancing static HTML with mention rendering.
 */
import { Plugin } from 'prosemirror-state';
import type { Node as PMNode } from 'prosemirror-model';
import type { EditorView, NodeView } from 'prosemirror-view';

// Navigation handler for mention clicks
let mentionNavigationHandler: ((uuid: string) => void) | null = null;

/**
 * Set the navigation handler for mention clicks
 */
export function setMentionNavigationHandler(handler: (uuid: string) => void): void {
  mentionNavigationHandler = handler;
}

/**
 * Get initials from a name for the avatar fallback
 */
function getInitials(name: string): string {
  if (!name) return '?';
  const parts = name.trim().split(/\s+/);
  if (parts.length === 1) {
    return parts[0].charAt(0).toUpperCase();
  }
  return (parts[0].charAt(0) + parts[parts.length - 1].charAt(0)).toUpperCase();
}

/**
 * Generate a consistent color from a string (uuid or name)
 */
function stringToColor(str: string): string {
  if (!str) return '#6366f1';
  let hash = 0;
  for (let i = 0; i < str.length; i++) {
    hash = str.charCodeAt(i) + ((hash << 5) - hash);
  }
  const colors = [
    '#6366f1', // indigo
    '#8b5cf6', // violet
    '#ec4899', // pink
    '#f43f5e', // rose
    '#f97316', // orange
    '#eab308', // yellow
    '#22c55e', // green
    '#14b8a6', // teal
    '#06b6d4', // cyan
    '#3b82f6', // blue
  ];
  return colors[Math.abs(hash) % colors.length];
}

/**
 * Create the inner content for a mention chip (avatar + name)
 */
function createMentionContent(uuid: string, name: string, avatarUrl: string | null): DocumentFragment {
  const fragment = document.createDocumentFragment();

  // Create avatar element
  const avatar = document.createElement('span');
  avatar.className = 'mention-avatar';

  if (avatarUrl) {
    const img = document.createElement('img');
    img.src = avatarUrl;
    img.alt = name;
    img.className = 'mention-avatar-img';
    img.onerror = () => {
      img.style.display = 'none';
      const fallback = document.createElement('span');
      fallback.className = 'mention-avatar-fallback';
      fallback.textContent = getInitials(name);
      fallback.style.backgroundColor = stringToColor(uuid);
      avatar.appendChild(fallback);
    };
    avatar.appendChild(img);
  } else {
    const fallback = document.createElement('span');
    fallback.className = 'mention-avatar-fallback';
    fallback.textContent = getInitials(name);
    fallback.style.backgroundColor = stringToColor(uuid);
    avatar.appendChild(fallback);
  }

  // Create name element
  const nameSpan = document.createElement('span');
  nameSpan.className = 'mention-name';
  nameSpan.textContent = name;

  fragment.appendChild(avatar);
  fragment.appendChild(nameSpan);

  return fragment;
}

/**
 * Enhance all mention elements in a container with avatar rendering.
 * Call this after rendering HTML that contains mention spans.
 */
export function enhanceMentions(container: HTMLElement): void {
  // Find all mention elements (both legacy .mention and new .mention-chip)
  const mentions = container.querySelectorAll('[data-uuid]:not([data-enhanced])');

  mentions.forEach((el) => {
    const element = el as HTMLElement;
    const uuid = element.getAttribute('data-uuid');
    const name = element.getAttribute('data-name') || element.textContent?.replace('@', '') || '';
    const avatarUrl = element.getAttribute('data-avatar-url') || null;

    if (!uuid) return;

    // Mark as enhanced to avoid re-processing
    element.setAttribute('data-enhanced', 'true');

    // Clear existing content and add chip styling
    element.textContent = '';
    element.className = 'mention-chip';

    // Add the avatar and name
    element.appendChild(createMentionContent(uuid, name, avatarUrl));

    // Add click handler
    element.addEventListener('click', (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (mentionNavigationHandler) {
        mentionNavigationHandler(uuid);
      }
    });
  });
}

class MentionNodeView implements NodeView {
  dom: HTMLElement;

  constructor(node: PMNode, _view: EditorView, _getPos: () => number | undefined) {
    const { uuid, name, avatarUrl } = node.attrs;

    // Create the container
    this.dom = document.createElement('span');
    this.dom.className = 'mention-chip';
    this.dom.setAttribute('data-mention', 'true');
    this.dom.setAttribute('data-uuid', uuid);
    this.dom.setAttribute('data-name', name);
    if (avatarUrl) {
      this.dom.setAttribute('data-avatar-url', avatarUrl);
    }
    this.dom.contentEditable = 'false';

    // Add avatar and name using shared function
    this.dom.appendChild(createMentionContent(uuid, name, avatarUrl));

    // Add click handler for navigation
    this.dom.addEventListener('click', (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (mentionNavigationHandler) {
        mentionNavigationHandler(uuid);
      }
    });
  }

  // Prevent editing inside the mention
  stopEvent() {
    return true;
  }

  // Node is immutable, ignore selection changes
  ignoreMutation() {
    return true;
  }
}

/**
 * Create the mention view plugin
 */
export function createMentionViewPlugin(): Plugin {
  return new Plugin({
    props: {
      nodeViews: {
        mention: (node, view, getPos) => new MentionNodeView(node, view, getPos),
      },
    },
  });
}

export default createMentionViewPlugin;
