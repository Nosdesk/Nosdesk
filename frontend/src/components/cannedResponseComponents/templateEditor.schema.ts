/**
 * Module-singleton schema, input rule, and string round-trip
 * helpers for the canned-response TemplateEditor. Kept out of the
 * .vue file so the Schema instance and NodeType references are
 * shared across every editor mount (per the ProseMirror Guide:
 * `NodeType` belongs to exactly one `Schema`, treat schemas as
 * module-level singletons to avoid cross-schema identity bugs).
 *
 * Deliberately divergent from the larger SimpleEditor / Yjs
 * Collaborative schemas: those carry `ychange` attrs on every
 * node and a heap of marks the admin authoring surface doesn't
 * need. A separate slim schema keeps the chip's surface area
 * small and testable.
 */

import { Schema, type Node as PMNode } from 'prosemirror-model';
import { InputRule } from 'prosemirror-inputrules';
import { CANNED_RESPONSE_VARIABLES } from '@/services/cannedResponsesService';

const ALLOWED: ReadonlyArray<string> = CANNED_RESPONSE_VARIABLES;

/** Literal wire-format token shape. Centralised so the delimiter
 * (currently `{{...}}`) lives in one place. */
export function formatVariableToken(name: string): string {
  return `{{${name}}}`;
}

/**
 * Schema with: doc (block+), paragraph (inline*), text, hard_break,
 * variable_token (inline atom, no marks). `marks: ""` on the chip
 * makes it explicitly opaque to mark application even though the
 * schema declares no marks at all, matching the PM Guide's advice
 * to be explicit about atom marks.
 */
export const templateSchema = new Schema({
  nodes: {
    doc: { content: 'block+' },
    paragraph: {
      group: 'block',
      content: 'inline*',
      parseDOM: [{ tag: 'p' }],
      toDOM() {
        return ['p', 0];
      },
    },
    text: { group: 'inline' },
    hard_break: {
      inline: true,
      group: 'inline',
      selectable: false,
      parseDOM: [{ tag: 'br' }],
      toDOM() {
        return ['br'];
      },
    },
    variable_token: {
      inline: true,
      group: 'inline',
      atom: true,
      selectable: true,
      draggable: false,
      marks: '',
      attrs: { name: { default: '' } },
      parseDOM: [
        {
          tag: 'span.variable-chip[data-variable]',
          getAttrs(dom) {
            return { name: (dom as HTMLElement).getAttribute('data-variable') ?? '' };
          },
        },
      ],
      toDOM(node) {
        return [
          'span',
          {
            class: 'variable-chip',
            'data-variable': node.attrs.name,
            contenteditable: 'false',
          },
          formatVariableToken(node.attrs.name),
        ];
      },
    },
  },
  marks: {},
});

/**
 * Input rule: typing `{{name}}` collapses to a chip when `name` is
 * on the allow-list. Anchors with `$` so it fires on the closing
 * `}}`, not at any midpoint of a longer token.
 *
 * Lives next to the schema rather than in `plugins/prosemirror-*`
 * because it's tightly coupled to the `variable_token` NodeType and
 * isn't reusable across surfaces.
 */
export const variableInputRule = new InputRule(
  /\{\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\}\}$/,
  (state, match, start, end) => {
    const name = match[1];
    if (!ALLOWED.includes(name)) return null;
    return state.tr.replaceWith(
      start,
      end,
      templateSchema.nodes.variable_token.create({ name }),
    );
  },
);

/**
 * Parse the wire-format string (paragraphs separated by blank
 * lines, `{{name}}` tokens, soft line breaks as `\n`) into a
 * ProseMirror Node. The empty-string case produces one empty
 * paragraph, which satisfies `doc: block+`.
 */
export function stringToDoc(value: string): PMNode {
  const paragraphs = value.split(/\n\n+/);
  const paragraphNodes = paragraphs.map((para) => {
    if (para === '') return templateSchema.nodes.paragraph.create();
    const inlines: PMNode[] = [];
    const tokenRe = /\{\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\}\}/g;
    let lastIdx = 0;
    let match: RegExpExecArray | null;
    while ((match = tokenRe.exec(para)) !== null) {
      pushTextRun(inlines, para.slice(lastIdx, match.index));
      if (ALLOWED.includes(match[1])) {
        inlines.push(templateSchema.nodes.variable_token.create({ name: match[1] }));
      } else {
        // Unknown variable: leave as literal text so the warn-banner
        // on the editor view flags it for the admin.
        pushTextRun(inlines, match[0]);
      }
      lastIdx = match.index + match[0].length;
    }
    pushTextRun(inlines, para.slice(lastIdx));
    return templateSchema.nodes.paragraph.create(null, inlines);
  });
  return templateSchema.nodes.doc.create(null, paragraphNodes);
}

function pushTextRun(target: PMNode[], chunk: string): void {
  if (chunk === '') return;
  const parts = chunk.split('\n');
  parts.forEach((part, i) => {
    if (part !== '') target.push(templateSchema.text(part));
    if (i < parts.length - 1) target.push(templateSchema.nodes.hard_break.create());
  });
}

/**
 * Serialise a ProseMirror doc back to the wire format. Uses
 * `textBetween` with a leaf-text callback (the PM-idiomatic
 * primitive for plain-text serialisation, see the reference
 * manual entry on `Node.textBetween`); paragraphs are joined by
 * `"\n\n"` via the `blockSeparator` argument. The leaf callback
 * handles chips and hard breaks; text content is automatic.
 */
export function docToString(doc: PMNode): string {
  return doc.textBetween(0, doc.content.size, '\n\n', (leafNode) => {
    if (leafNode.type === templateSchema.nodes.variable_token) {
      return formatVariableToken(leafNode.attrs.name);
    }
    if (leafNode.type === templateSchema.nodes.hard_break) {
      return '\n';
    }
    return '';
  });
}
