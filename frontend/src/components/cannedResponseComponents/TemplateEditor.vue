<script setup lang="ts">
/**
 * Chip-aware editor for canned-response bodies. Variables in the
 * allow-list render as styled non-text tokens you can select and
 * delete as a unit; everything else is plain text. Round-trips
 * with the wire format the backend stores: `{{name}}` text and
 * plain paragraphs separated by blank lines.
 *
 * Schema, input rule, and string round-trip helpers live next to
 * this file in `templateEditor.schema.ts` (module-singleton, per
 * the ProseMirror Guide's note that NodeTypes belong to exactly
 * one Schema and schemas should be module-level singletons). The
 * `.vue` file owns only the EditorView lifecycle and the v-model
 * bridge.
 *
 * The reply-composer (SimpleEditor) and the collaborative
 * documentation editor each carry their own, richer schemas; the
 * deliberate divergence is documented in the schema module.
 *
 * v-model contract:
 *  - in: a plain string with `{{name}}` tokens, paragraphs split
 *    by `\n\n`, soft line breaks as `\n`.
 *  - out: same shape on every transaction. Save logic should trim
 *    before persisting.
 */
import { ref, watch, onMounted, onBeforeUnmount, computed } from 'vue';
import { useFluent } from 'fluent-vue';
import { type Node as PMNode } from 'prosemirror-model';
import { EditorState, Plugin } from 'prosemirror-state';
import { EditorView, type NodeView } from 'prosemirror-view';
import { keymap } from 'prosemirror-keymap';
import { baseKeymap } from 'prosemirror-commands';
import { history, undo, redo } from 'prosemirror-history';
import { inputRules } from 'prosemirror-inputrules';
import { CANNED_RESPONSE_VARIABLES } from '@/services/cannedResponsesService';
import {
  templateSchema,
  variableInputRule,
  formatVariableToken,
  stringToDoc,
  docToString,
} from './templateEditor.schema';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const props = withDefaults(
  defineProps<{
    modelValue: string;
    placeholder?: string;
    minHeight?: string;
    maxHeight?: string;
  }>(),
  {
    placeholder: '',
    minHeight: '180px',
    // Conservative default so the preview pane stays reachable on
    // phone viewports where the layout stacks the editor on top of
    // the preview. Larger viewports can push it higher via the prop.
    maxHeight: '320px',
  },
);

const emit = defineEmits<{
  'update:modelValue': [value: string];
}>();

const editorRoot = ref<HTMLElement | null>(null);
let view: EditorView | null = null;

// nodeView reinforces the contenteditable=false guard regardless
// of how the chip entered the doc (paste, input rule, programmatic
// insert), and gives screen readers a human-readable label instead
// of the literal "open curly brace open curly brace ...".
function variableNodeView(node: PMNode): NodeView {
  const dom = document.createElement('span');
  dom.className = 'variable-chip';
  dom.setAttribute('data-variable', node.attrs.name);
  dom.setAttribute('contenteditable', 'false');
  dom.setAttribute('role', 'img');
  dom.setAttribute(
    'aria-label',
    t('admin-canned-responses-editor-variable-aria', { name: node.attrs.name }),
  );
  dom.textContent = formatVariableToken(node.attrs.name);
  return { dom };
}

// Chip-aware delete keymap. Sits before baseKeymap so the chip
// cases win; everything else falls through to default behaviour.
//
// Backspace right after a chip deletes the chip in one stroke
// (rather than PM's default "first stroke selects, second stroke
// deletes" two-step). Same for Delete right before a chip. When a
// NodeSelection of a chip is already active (e.g. the user clicked
// the chip), either key removes it via the standard deleteSelection
// path in baseKeymap, so we only need to handle the cursor-adjacent
// cases here.
const chipDeleteKeymap = keymap({
  Backspace: (state, dispatch) => {
    if (!state.selection.empty) return false;
    const { $from } = state.selection;
    const before = $from.nodeBefore;
    if (!before || before.type !== templateSchema.nodes.variable_token) return false;
    if (dispatch) {
      dispatch(state.tr.delete($from.pos - before.nodeSize, $from.pos));
    }
    return true;
  },
  Delete: (state, dispatch) => {
    if (!state.selection.empty) return false;
    const { $from } = state.selection;
    const after = $from.nodeAfter;
    if (!after || after.type !== templateSchema.nodes.variable_token) return false;
    if (dispatch) {
      dispatch(state.tr.delete($from.pos, $from.pos + after.nodeSize));
    }
    return true;
  },
});

function buildState(value: string): EditorState {
  return EditorState.create({
    doc: stringToDoc(value),
    plugins: [
      history(),
      keymap({ 'Mod-z': undo, 'Mod-y': redo, 'Mod-Shift-z': redo }),
      chipDeleteKeymap,
      keymap(baseKeymap),
      inputRules({ rules: [variableInputRule] }),
      new Plugin({
        props: {
          attributes: { class: 'prose prose-sm max-w-none focus:outline-none' },
        },
      }),
    ],
  });
}

onMounted(() => {
  if (!editorRoot.value) return;
  view = new EditorView(editorRoot.value, {
    state: buildState(props.modelValue),
    nodeViews: { variable_token: variableNodeView },
    dispatchTransaction(tr) {
      if (!view) return;
      const next = view.state.apply(tr);
      view.updateState(next);
      if (tr.docChanged) {
        emit('update:modelValue', docToString(next.doc));
      }
    },
  });
});

onBeforeUnmount(() => {
  view?.destroy();
  view = null;
});

/**
 * Re-hydrate from external prop changes (parent loaded an existing
 * response, starter pre-fill, etc.). The round-trip guard
 * (`current === next`) is the primary defence against loops: if
 * the incoming string matches what the editor would serialise to,
 * we skip. This is more robust than a transient suppression flag,
 * since it doesn't depend on microtask ordering between Vue's
 * watcher and our emit cycle.
 */
watch(
  () => props.modelValue,
  (next) => {
    if (!view) return;
    if (docToString(view.state.doc) === next) return;
    view.updateState(buildState(next));
  },
);

/**
 * Insert a chip at the current selection. Follows the dino example
 * pattern from the PM docs: `canReplaceWith` validates the schema
 * allows the chip in the surrounding context before dispatching,
 * so the toolbar pill becomes a no-op rather than a crash when
 * the cursor sits in a node that forbids inline atoms.
 *
 * After insert, leave the cursor positioned right after the chip
 * (PM's `replaceSelectionWith` default) so the user can keep
 * typing context immediately. Selecting the chip post-insert
 * would create a NodeSelection which most users wouldn't expect.
 */
function insertVariable(name: string): void {
  if (!view) return;
  const { state, dispatch } = view;
  const { $from } = state.selection;
  const index = $from.index();
  const tokenType = templateSchema.nodes.variable_token;
  if (!$from.parent.canReplaceWith(index, index, tokenType)) return;
  dispatch(state.tr.replaceSelectionWith(tokenType.create({ name }), false));
  view.focus();
}

function pillLabel(name: string): string {
  return formatVariableToken(name);
}

const showPlaceholder = computed(() => props.modelValue === '');
</script>

<template>
  <div class="template-editor flex flex-col gap-2">
    <!-- Toolbar pills above the editor. Click any to drop a chip at
         the current selection. Allow-list comes from the service so
         it matches the validator the save handler enforces. The
         label sits in its own gap-2 group so the visual separation
         from the first pill is bigger than the inter-pill gap. -->
    <div class="flex flex-wrap items-center gap-2">
      <span class="text-xs text-secondary">
        {{ t('admin-canned-responses-editor-insert-label') }}
      </span>
      <div class="flex flex-wrap items-center gap-1.5">
        <button
          v-for="name in CANNED_RESPONSE_VARIABLES"
          :key="name"
          type="button"
          class="px-2 py-1 text-xs rounded-md bg-surface-alt text-secondary hover:text-primary hover:bg-surface-hover border border-default font-mono transition-colors"
          :aria-label="t('admin-canned-responses-editor-insert-variable-aria', { name })"
          @click="insertVariable(name)"
        >
          {{ pillLabel(name) }}
        </button>
      </div>
    </div>
    <div class="editor-shell relative border border-default rounded-lg bg-surface">
      <div
        v-if="showPlaceholder"
        class="absolute top-3 left-3 text-tertiary pointer-events-none text-sm"
      >
        {{ placeholder }}
      </div>
      <div
        ref="editorRoot"
        class="editor-root p-3 text-sm text-primary focus:outline-none overflow-auto"
        :style="{ minHeight, maxHeight }"
      ></div>
    </div>
  </div>
</template>

<style scoped>
.editor-shell:focus-within {
  border-color: rgb(var(--color-accent) / 0.6);
  box-shadow: 0 0 0 2px rgb(var(--color-accent) / 0.2);
}
.editor-root :deep(p) {
  margin: 0 0 0.5rem;
}
.editor-root :deep(p:last-child) {
  margin-bottom: 0;
}
/* The chip sits in flowing inline text, not a flex container, so
   the 1px horizontal margin is for inline rhythm against
   surrounding glyphs (gap doesn't apply across inline-flow). */
.editor-root :deep(.variable-chip) {
  display: inline-block;
  padding: 0 6px;
  margin: 0 1px;
  border-radius: 4px;
  background-color: rgb(var(--color-accent) / 0.15);
  color: rgb(var(--color-accent));
  font-family: ui-monospace, SFMono-Regular, monospace;
  font-size: 0.85em;
  vertical-align: baseline;
  white-space: nowrap;
  cursor: pointer;
  user-select: none;
  transition:
    background-color 120ms ease,
    color 120ms ease,
    box-shadow 120ms ease;
}
.editor-root :deep(.variable-chip:hover) {
  background-color: rgb(var(--color-accent) / 0.25);
}
/* Stronger "I am selected, press Backspace to remove me" cue:
   solid accent fill + white text + ring. Less subtle than the
   previous outline-only treatment, which competed with the
   chip's existing tinted background for the reader's attention. */
.editor-root :deep(.variable-chip.ProseMirror-selectednode) {
  background-color: rgb(var(--color-accent));
  color: white;
  box-shadow: 0 0 0 2px rgb(var(--color-accent) / 0.35);
}
</style>
