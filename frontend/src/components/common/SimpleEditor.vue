<script setup lang="ts">
/**
 * SimpleEditor - A lightweight ProseMirror editor without collaboration
 *
 * Features:
 * - Markdown-like formatting (bold, italic, code, lists, etc.)
 * - Twemoji rendering
 * - @mention support with user search
 * - Ticket reference support (paste URLs or drag tickets)
 * - v-model binding
 */
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue';
import { EditorView } from 'prosemirror-view';
import { EditorState } from 'prosemirror-state';
import { schema } from '@/components/editor/schema';
import { keymap } from 'prosemirror-keymap';
import { baseKeymap } from 'prosemirror-commands';
import { history, undo, redo } from 'prosemirror-history';
import { twemojiPlugin } from '@/plugins/prosemirror-twemoji';
import { createPlaceholderPlugin } from '@/plugins/prosemirror-placeholder';
import {
  createMentionPlugins,
  insertMention,
  closeMention,
  type MentionState,
  type MentionUser,
} from '@/plugins/prosemirror-mentions';
import { createMentionViewPlugin } from '@/plugins/prosemirror-mention-view';
import { createTicketLinkPlugin } from '@/components/editor/ticketLinkPlugin';
import { createTicketDropIndicatorPlugin } from '@/components/editor/ticketDropIndicatorPlugin';
import {
  inputRules,
  wrappingInputRule,
  textblockTypeInputRule,
  smartQuotes,
  emDash,
  ellipsis,
} from 'prosemirror-inputrules';
import { DOMSerializer, DOMParser } from 'prosemirror-model';
import { useDataStore } from '@/stores/dataStore';
import UserAvatar from '@/components/UserAvatar.vue';

const props = withDefaults(defineProps<{
  modelValue: string;
  placeholder?: string;
  disabled?: boolean;
  minHeight?: string;
  maxHeight?: string;
}>(), {
  placeholder: 'Type your message... (supports @mentions and **markdown**)',
  disabled: false,
  minHeight: '80px',
  maxHeight: '300px',
});

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
  (e: 'submit'): void;
}>();

const dataStore = useDataStore();

const editorElement = ref<HTMLElement | null>(null);
const editorWrapper = ref<HTMLElement | null>(null);
const dropdownRef = ref<HTMLElement | null>(null);
let view: EditorView | null = null;

// Mention state
const mentionState = ref<MentionState>({
  active: false,
  query: '',
  from: 0,
  to: 0,
  position: null,
});
const users = ref<MentionUser[]>([]);
const selectedIndex = ref(0);
const isSearching = ref(false);
let searchTimer: ReturnType<typeof setTimeout> | null = null;

// Dropdown position using fixed positioning for viewport awareness
const dropdownStyle = computed(() => {
  if (!mentionState.value.active || !mentionState.value.position) {
    return { display: 'none' };
  }

  const { top, left } = mentionState.value.position;
  const dropdownHeight = 280; // Approximate max height of dropdown
  const dropdownWidth = 300;
  const viewportHeight = window.innerHeight;
  const viewportWidth = window.innerWidth;
  const padding = 8;

  // Check if dropdown would overflow bottom of viewport
  const wouldOverflowBottom = top + dropdownHeight + padding > viewportHeight;

  // Calculate left position, ensuring it doesn't overflow right edge
  const adjustedLeft = Math.min(left, viewportWidth - dropdownWidth - padding);

  return {
    display: 'block',
    position: 'fixed',
    top: wouldOverflowBottom ? 'auto' : `${top + padding}px`,
    bottom: wouldOverflowBottom ? `${viewportHeight - top + padding}px` : 'auto',
    left: `${Math.max(padding, adjustedLeft)}px`,
  };
});

// Search users
const searchUsers = async (query: string) => {
  isSearching.value = true;
  try {
    const result = await dataStore.getPaginatedUsers({
      page: 1,
      pageSize: 8,
      search: query || undefined,
    });
    users.value = result.data as MentionUser[];
    selectedIndex.value = 0;
  } catch (error) {
    console.error('Failed to search users:', error);
    users.value = [];
  } finally {
    isSearching.value = false;
  }
};

// Debounced search
const debouncedSearch = (query: string) => {
  if (searchTimer) clearTimeout(searchTimer);
  searchTimer = setTimeout(() => searchUsers(query), 150);
};

// Handle mention state changes from plugin
const handleMentionStateChange = (state: MentionState) => {
  mentionState.value = state;
  if (state.active) {
    debouncedSearch(state.query);
  }
};

// Select a user from the dropdown
const selectUser = (user: MentionUser) => {
  if (!view) return;
  insertMention(view, user, schema.nodes.mention);
};

// Handle keyboard navigation in dropdown (called by ProseMirror plugin)
// Returns true if the key was handled to prevent default ProseMirror behavior
const handleMentionKeyDown = (key: 'ArrowUp' | 'ArrowDown' | 'Enter' | 'Tab' | 'Escape'): boolean => {
  if (!mentionState.value.active || !view) return false;

  switch (key) {
    case 'ArrowDown':
      selectedIndex.value = Math.min(selectedIndex.value + 1, users.value.length - 1);
      scrollToSelected();
      return true;
    case 'ArrowUp':
      selectedIndex.value = Math.max(selectedIndex.value - 1, 0);
      scrollToSelected();
      return true;
    case 'Enter':
    case 'Tab':
      if (users.value.length > 0) {
        selectUser(users.value[selectedIndex.value]);
        return true;
      }
      return false;
    case 'Escape':
      closeMention(view);
      return true;
  }
  return false;
};

const scrollToSelected = () => {
  nextTick(() => {
    const dropdown = dropdownRef.value;
    if (!dropdown) return;
    const selected = dropdown.querySelector('.selected') as HTMLElement;
    if (selected) {
      selected.scrollIntoView({ block: 'nearest' });
    }
  });
};

// Build input rules for markdown-like shortcuts
function buildInputRules() {
  const rules = [
    ...smartQuotes,
    ellipsis,
    emDash,
    // > blockquote
    wrappingInputRule(/^\s*>\s$/, schema.nodes.blockquote),
    // - or * for bullet list
    wrappingInputRule(/^\s*([-*])\s$/, schema.nodes.bullet_list),
    // 1. for ordered list
    wrappingInputRule(
      /^(\d+)\.\s$/,
      schema.nodes.ordered_list,
      match => ({ order: +match[1] }),
      (match, node) => node.childCount + node.attrs.order === +match[1]
    ),
    // ``` for code block
    textblockTypeInputRule(/^```(\w+)?\s$/, schema.nodes.code_block, match => ({
      language: match[1] || null
    })),
    // # ## ### for headings
    textblockTypeInputRule(/^(#{1,6})\s$/, schema.nodes.heading, match => ({
      level: match[1].length
    })),
  ];
  return inputRules({ rules });
}

// Convert HTML to ProseMirror doc
function htmlToDoc(html: string) {
  if (!html) {
    return schema.node('doc', null, [schema.node('paragraph')]);
  }
  const container = document.createElement('div');
  container.innerHTML = html;
  return DOMParser.fromSchema(schema).parse(container);
}

// Convert ProseMirror doc to HTML
function docToHtml(doc: any): string {
  const fragment = DOMSerializer.fromSchema(schema).serializeFragment(doc.content);
  const container = document.createElement('div');
  container.appendChild(fragment);
  return container.innerHTML;
}

// Initialize editor
onMounted(() => {
  if (!editorElement.value) return;

  // Load initial users
  searchUsers('');

  const state = EditorState.create({
    doc: htmlToDoc(props.modelValue),
    plugins: [
      buildInputRules(),
      keymap({
        'Mod-z': undo,
        'Mod-y': redo,
        'Mod-Shift-z': redo,
        'Mod-Enter': () => {
          emit('submit');
          return true;
        },
      }),
      // Mention plugins must come BEFORE baseKeymap to intercept Enter/Tab/Arrow keys
      ...createMentionPlugins({
        onStateChange: handleMentionStateChange,
        onKeyDown: handleMentionKeyDown,
      }),
      keymap(baseKeymap),
      history(),
      createMentionViewPlugin(),
      createTicketLinkPlugin(),
      createTicketDropIndicatorPlugin(),
      createPlaceholderPlugin(props.placeholder),
      twemojiPlugin,
    ],
  });

  view = new EditorView(editorElement.value, {
    state,
    editable: () => !props.disabled,
    dispatchTransaction(transaction) {
      if (!view) return;
      const newState = view.state.apply(transaction);
      view.updateState(newState);

      if (transaction.docChanged) {
        const html = docToHtml(newState.doc);
        emit('update:modelValue', html);
      }
    },
    attributes: {
      class: 'simple-editor-content',
      'data-placeholder': props.placeholder,
    },
  });
});

// Watch for external value changes
watch(() => props.modelValue, (newValue) => {
  if (!view) return;

  const currentHtml = docToHtml(view.state.doc);
  if (currentHtml !== newValue) {
    const newDoc = htmlToDoc(newValue);
    const tr = view.state.tr.replaceWith(0, view.state.doc.content.size, newDoc.content);
    view.dispatch(tr);
  }
});

// Watch disabled state
watch(() => props.disabled, () => {
  if (view) {
    view.setProps({ editable: () => !props.disabled });
  }
});

onUnmounted(() => {
  if (searchTimer) clearTimeout(searchTimer);
  if (view) {
    view.destroy();
    view = null;
  }
});

// Expose focus method
const focus = () => {
  nextTick(() => {
    view?.focus();
  });
};

const clear = () => {
  if (!view) return;
  const emptyDoc = schema.node('doc', null, [schema.node('paragraph')]);
  const tr = view.state.tr.replaceWith(0, view.state.doc.content.size, emptyDoc.content);
  view.dispatch(tr);
  emit('update:modelValue', '');
};

defineExpose({ focus, clear });
</script>

<template>
  <div ref="editorWrapper" class="simple-editor-wrapper">
    <div
      ref="editorElement"
      class="simple-editor"
      :class="{ 'is-disabled': disabled }"
      :style="{
        minHeight: minHeight,
        maxHeight: maxHeight,
      }"
    />

    <!-- Mention Dropdown (teleported to body for proper positioning) -->
    <Teleport to="body">
      <Transition
        enter-active-class="transition ease-out duration-100"
        enter-from-class="transform opacity-0 scale-95"
        enter-to-class="transform opacity-100 scale-100"
        leave-active-class="transition ease-in duration-75"
        leave-from-class="transform opacity-100 scale-100"
        leave-to-class="transform opacity-0 scale-95"
      >
        <div
          v-if="mentionState.active"
          ref="dropdownRef"
          class="mention-dropdown"
          :style="dropdownStyle"
        >
          <!-- Search indicator -->
          <div v-if="mentionState.query" class="px-3 py-2 text-xs text-tertiary border-b border-default bg-surface-alt">
            Searching for "<span class="text-primary font-medium">{{ mentionState.query }}</span>"
          </div>

          <!-- Loading -->
          <div v-if="isSearching" class="px-3 py-4 flex items-center justify-center">
            <div class="animate-spin rounded-full h-4 w-4 border-2 border-accent border-t-transparent"></div>
          </div>

          <!-- User list -->
          <div v-else-if="users.length > 0" class="max-h-48 overflow-y-auto">
            <button
              v-for="(user, index) in users"
              :key="user.uuid"
              type="button"
              @click="selectUser(user)"
              @mouseenter="selectedIndex = index"
              class="w-full px-3 py-2 flex items-center gap-3 text-left hover:bg-surface-alt transition-colors"
              :class="{ 'bg-surface-alt selected': index === selectedIndex }"
            >
              <UserAvatar
                :name="user.name"
                :avatar="user.avatar_thumb || user.avatar_url"
                size="sm"
                :showName="false"
              />
              <div class="flex-1 min-w-0">
                <p class="text-sm font-medium text-primary truncate">{{ user.name }}</p>
                <p v-if="user.email" class="text-xs text-tertiary truncate">{{ user.email }}</p>
              </div>
            </button>
          </div>

          <!-- No results -->
          <div v-else class="px-3 py-4 text-center text-sm text-tertiary">
            No users found
          </div>

          <!-- Hint -->
          <div class="px-3 py-2 text-xs text-tertiary border-t border-default bg-surface-alt flex items-center gap-4">
            <span><kbd class="px-1 py-0.5 bg-surface rounded text-xs">↑↓</kbd> Navigate</span>
            <span><kbd class="px-1 py-0.5 bg-surface rounded text-xs">Enter</kbd> Select</span>
            <span><kbd class="px-1 py-0.5 bg-surface rounded text-xs">Esc</kbd> Close</span>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<style scoped>
.simple-editor-wrapper {
  position: relative;
}

.simple-editor {
  width: 100%;
  font-size: 0.875rem;
  color: var(--color-primary);
  background-color: var(--color-surface);
  border: 1px solid var(--color-default);
  border-radius: 0.5rem;
  overflow-y: auto;
}

.simple-editor :deep(.ProseMirror) {
  padding: 0.5rem 0.75rem;
  outline: none;
  position: relative;
}

.simple-editor :deep(.ProseMirror[data-placeholder]::before) {
  content: attr(data-placeholder);
  color: var(--color-tertiary);
  position: absolute;
  pointer-events: none;
}

.simple-editor:focus-within {
  outline: none;
  border-color: var(--color-accent);
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-accent) 25%, transparent);
}

.simple-editor.is-disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.simple-editor :deep(.simple-editor-content) {
  outline: none;
  min-height: 100%;
}

.simple-editor :deep(.simple-editor-content:empty::before) {
  content: attr(data-placeholder);
  color: var(--color-tertiary);
  pointer-events: none;
  position: absolute;
}

/* Mention typing highlight */
.simple-editor :deep(.mention-typing) {
  background-color: color-mix(in srgb, var(--color-accent) 15%, transparent);
  border-radius: 0.25rem;
}


/* Basic prose styles */
.simple-editor :deep(p) {
  margin: 0 0 0.5rem 0;
}

.simple-editor :deep(p:last-child) {
  margin-bottom: 0;
}

.simple-editor :deep(strong) {
  font-weight: 600;
}

.simple-editor :deep(em) {
  font-style: italic;
}

.simple-editor :deep(code) {
  background-color: var(--color-surface-alt);
  padding: 0.125rem 0.375rem;
  border-radius: 0.25rem;
  font-size: 0.75rem;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.simple-editor :deep(pre) {
  background-color: var(--color-surface-alt);
  padding: 0.75rem;
  border-radius: 0.5rem;
  overflow-x: auto;
  margin: 0.5rem 0;
}

.simple-editor :deep(pre code) {
  background-color: transparent;
  padding: 0;
}

.simple-editor :deep(blockquote) {
  border-left: 3px solid var(--color-default);
  padding-left: 0.75rem;
  margin: 0.5rem 0;
  color: var(--color-secondary);
}

.simple-editor :deep(ul),
.simple-editor :deep(ol) {
  padding-left: 1.25rem;
  margin: 0.5rem 0;
}

.simple-editor :deep(li) {
  margin-bottom: 0.25rem;
}

.simple-editor :deep(a) {
  color: var(--color-accent);
  text-decoration: underline;
}

/* Mention chip styles */
.simple-editor :deep(.mention-chip) {
  display: inline-flex;
  align-items: center;
  gap: 0.1875rem;
  padding: 0.0625em 0.375em 0.0625em 0.1875em;
  margin: 0 0.0625em;
  border-radius: 9999px;
  background-color: color-mix(in srgb, var(--color-accent) 12%, transparent);
  color: var(--color-accent);
  font-weight: 500;
  font-size: inherit;
  line-height: 1;
  vertical-align: text-bottom;
  cursor: pointer;
  user-select: none;
  transition: background-color 0.15s ease;
}

.simple-editor :deep(.mention-chip:hover) {
  background-color: color-mix(in srgb, var(--color-accent) 20%, transparent);
}

.simple-editor :deep(.mention-avatar) {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 1em;
  height: 1em;
  border-radius: 9999px;
  overflow: hidden;
  flex-shrink: 0;
}

.simple-editor :deep(.mention-avatar-img) {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.simple-editor :deep(.mention-avatar-fallback) {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: white;
  font-size: 0.5em;
  font-weight: 600;
}

.simple-editor :deep(.mention-name) {
  white-space: nowrap;
}

/* Ticket Link Card Styles */
.simple-editor :deep(.ticket-link-card) {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px 10px;
  margin: 8px 0;
  background: var(--color-surface-alt);
  border: 1px solid var(--color-border-subtle);
  border-radius: 6px;
  font-size: 12px;
  line-height: 1.4;
  cursor: pointer;
  transition: all 0.15s ease;
  width: 100%;
  max-width: 100%;
  clear: both;
}

@media (min-width: 640px) {
  .simple-editor :deep(.ticket-link-card) {
    padding: 10px 12px;
    font-size: 13px;
  }
}

.simple-editor :deep(.ticket-link-card:hover) {
  border-color: var(--color-accent);
  background: var(--color-surface-hover);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
}

.simple-editor :deep(.ticket-link-loading) {
  opacity: 0.7;
}

.simple-editor :deep(.ticket-link-error) {
  border-color: var(--color-status-error);
}

.simple-editor :deep(.ticket-link-header) {
  display: flex;
  align-items: center;
  gap: 8px;
}

.simple-editor :deep(.ticket-link-id) {
  font-weight: 600;
  color: var(--color-text-secondary, #888);
  font-family: var(--font-mono, monospace);
  font-size: 12px;
  flex-shrink: 0;
}

.simple-editor :deep(.ticket-link-title) {
  color: var(--color-text-primary, #fff);
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}

.simple-editor :deep(.ticket-link-meta) {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
  font-size: 11px;
}

.simple-editor :deep(.ticket-link-person) {
  color: var(--color-text-secondary, #aaa);
}

.simple-editor :deep(.ticket-link-label) {
  color: var(--color-text-tertiary, #666);
}

.simple-editor :deep(.ticket-link-status) {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 4px;
  font-weight: 500;
  text-transform: capitalize;
}

.simple-editor :deep(.ticket-link-status-open) {
  background: var(--color-status-open-muted, rgba(59, 130, 246, 0.15));
  color: var(--color-status-open, #3b82f6);
}

.simple-editor :deep(.ticket-link-status-in-progress) {
  background: var(--color-status-in-progress-muted, rgba(245, 158, 11, 0.15));
  color: var(--color-status-in-progress, #f59e0b);
}

.simple-editor :deep(.ticket-link-status-closed) {
  background: var(--color-status-closed-muted, rgba(34, 197, 94, 0.15));
  color: var(--color-status-closed, #22c55e);
}

.simple-editor :deep(.ticket-link-priority) {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 4px;
  font-weight: 500;
  text-transform: capitalize;
}

.simple-editor :deep(.ticket-link-priority-high) {
  background: var(--color-priority-high-muted, rgba(239, 68, 68, 0.15));
  color: var(--color-priority-high, #ef4444);
}

.simple-editor :deep(.ticket-link-priority-medium) {
  background: var(--color-priority-medium-muted, rgba(245, 158, 11, 0.15));
  color: var(--color-priority-medium, #f59e0b);
}

.simple-editor :deep(.ticket-link-priority-low) {
  background: var(--color-priority-low-muted, rgba(34, 197, 94, 0.15));
  color: var(--color-priority-low, #22c55e);
}

.simple-editor :deep(.ticket-link-status .indicator-icon),
.simple-editor :deep(.ticket-link-priority .indicator-icon) {
  width: 10px;
  height: 10px;
  flex-shrink: 0;
}

.simple-editor :deep(.ticket-link-loader) {
  width: 12px;
  height: 12px;
  border: 2px solid var(--color-border-default, #333);
  border-top-color: var(--color-accent);
  border-radius: 50%;
  animation: ticket-link-spin 0.8s linear infinite;
}

@keyframes ticket-link-spin {
  to { transform: rotate(360deg); }
}

/* Ticket Drop Preview - shows ticket preview when dragging over editor */
.simple-editor :deep(.ticket-drop-preview) {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 12px;
  margin: 8px 0;
  background: var(--color-surface-alt);
  border: 2px dashed var(--color-accent);
  border-radius: 6px;
  font-size: 13px;
  line-height: 1.4;
  width: 100%;
  max-width: 100%;
  pointer-events: none;
  opacity: 0.85;
  animation: ticket-drop-preview-pulse 1.5s ease-in-out infinite;
  box-shadow: 0 0 12px rgba(var(--color-accent-rgb, 59, 130, 246), 0.3);
}

@keyframes ticket-drop-preview-pulse {
  0%, 100% {
    opacity: 0.75;
    box-shadow: 0 0 8px rgba(var(--color-accent-rgb, 59, 130, 246), 0.2);
  }
  50% {
    opacity: 0.95;
    box-shadow: 0 0 16px rgba(var(--color-accent-rgb, 59, 130, 246), 0.4);
  }
}

.simple-editor :deep(.ticket-drop-preview-header) {
  display: flex;
  align-items: center;
  gap: 8px;
}

.simple-editor :deep(.ticket-drop-preview-id) {
  font-weight: 600;
  color: var(--color-accent);
  font-family: var(--font-mono, monospace);
  font-size: 12px;
  flex-shrink: 0;
}

.simple-editor :deep(.ticket-drop-preview-title) {
  color: var(--color-text-primary, #fff);
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}

.simple-editor :deep(.ticket-drop-preview-meta) {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
  font-size: 11px;
}

.simple-editor :deep(.ticket-drop-preview-person) {
  color: var(--color-text-secondary, #aaa);
}

.simple-editor :deep(.ticket-drop-preview-label) {
  color: var(--color-text-tertiary, #666);
}

.simple-editor :deep(.ticket-drop-preview-status) {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 4px;
  font-weight: 500;
  text-transform: capitalize;
}

.simple-editor :deep(.ticket-drop-preview-status-open) {
  background: var(--color-status-open-muted, rgba(59, 130, 246, 0.15));
  color: var(--color-status-open, #3b82f6);
}

.simple-editor :deep(.ticket-drop-preview-status-in-progress) {
  background: var(--color-status-in-progress-muted, rgba(245, 158, 11, 0.15));
  color: var(--color-status-in-progress, #f59e0b);
}

.simple-editor :deep(.ticket-drop-preview-status-closed) {
  background: var(--color-status-closed-muted, rgba(34, 197, 94, 0.15));
  color: var(--color-status-closed, #22c55e);
}

.simple-editor :deep(.ticket-drop-preview-priority) {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 4px;
  font-weight: 500;
}

.simple-editor :deep(.ticket-drop-preview-priority-high) {
  background: var(--color-priority-high-muted, rgba(239, 68, 68, 0.15));
  color: var(--color-priority-high, #ef4444);
}

.simple-editor :deep(.ticket-drop-preview-priority-medium) {
  background: var(--color-priority-medium-muted, rgba(245, 158, 11, 0.15));
  color: var(--color-priority-medium, #f59e0b);
}

.simple-editor :deep(.ticket-drop-preview-priority-low) {
  background: var(--color-priority-low-muted, rgba(34, 197, 94, 0.15));
  color: var(--color-priority-low, #22c55e);
}

.simple-editor :deep(.ticket-drop-preview-skeleton) {
  display: inline-block;
  height: 14px;
  min-width: 80px;
  background: linear-gradient(
    90deg,
    var(--color-surface-hover) 25%,
    var(--color-surface-alt) 50%,
    var(--color-surface-hover) 75%
  );
  background-size: 200% 100%;
  animation: ticket-drop-skeleton-shimmer 1.5s ease-in-out infinite;
  border-radius: 4px;
}

@keyframes ticket-drop-skeleton-shimmer {
  0% { background-position: 200% 0; }
  100% { background-position: -200% 0; }
}
</style>

<!-- Global styles for teleported dropdown -->
<style>
.mention-dropdown {
  z-index: 9999;
  min-width: 250px;
  max-width: 350px;
  background-color: var(--color-surface);
  border: 1px solid var(--color-default);
  border-radius: 0.5rem;
  box-shadow: 0 10px 15px -3px rgba(0, 0, 0, 0.1), 0 4px 6px -2px rgba(0, 0, 0, 0.05);
  overflow: hidden;
}
</style>
