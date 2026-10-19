import { Schema } from 'prosemirror-model';
import { tableNodes } from 'prosemirror-tables';
import { assetUrl, assetPath } from '@nosdesk/core/transport';
import type { Node as ProseMirrorNode, NodeSpec, MarkSpec, DOMOutputSpec } from 'prosemirror-model';

/** The document schema's version; see `schemaVersion.ts`. */
export { EDITOR_SCHEMA_VERSION } from './schemaVersion';

const brDOM: DOMOutputSpec = ['br'];

const calcYchangeDomAttrs = (
  attrs: Record<string, unknown>,
  domAttrs: Record<string, string> = {}
): Record<string, string> => {
  const result = { ...domAttrs };
  if (attrs.ychange !== null && attrs.ychange !== undefined) {
    const ychange = attrs.ychange as { user: string; state: string };
    result.ychange_user = ychange.user;
    result.ychange_state = ychange.state;
  }
  return result;
};

// Specs for the nodes every editor shares: the comment composer's whole
// schema, and the base of the documentation schema below.
const commentNodes: {[key: string]: NodeSpec} = {
  // The top level document node
  doc: {
    content: 'block+'
  },
  
  // A plain paragraph textblock. Represented in the DOM as a <p> element
  paragraph: {
    attrs: { ychange: { default: null } },
    content: 'inline*',
    group: 'block',
    parseDOM: [{ tag: 'p' }],
    toDOM(node) { return ['p', calcYchangeDomAttrs(node.attrs), 0]; }
  },
  
  // A blockquote (<blockquote>) wrapping one or more blocks
  blockquote: {
    attrs: { ychange: { default: null } },
    content: 'block+',
    group: 'block',
    defining: true,
    parseDOM: [{ tag: 'blockquote' }],
    toDOM(node) { return ['blockquote', calcYchangeDomAttrs(node.attrs), 0]; }
  },
  
  // A horizontal rule (<hr>)
  horizontal_rule: {
    attrs: { ychange: { default: null } },
    group: 'block',
    parseDOM: [{ tag: 'hr' }],
    toDOM(node) {
      return ['hr', calcYchangeDomAttrs(node.attrs)];
    }
  },
  
  // A heading textblock, with a `level` attribute that should hold the number 1-6
  heading: {
    attrs: {
      level: { default: 1 },
      ychange: { default: null }
    },
    content: 'inline*',
    group: 'block',
    defining: true,
    parseDOM: [
      { tag: 'h1', attrs: { level: 1 } },
      { tag: 'h2', attrs: { level: 2 } },
      { tag: 'h3', attrs: { level: 3 } },
      { tag: 'h4', attrs: { level: 4 } },
      { tag: 'h5', attrs: { level: 5 } },
      { tag: 'h6', attrs: { level: 6 } }
    ],
    toDOM(node) { return ['h' + node.attrs.level, calcYchangeDomAttrs(node.attrs), 0]; }
  },
  
  // A code listing. Disallows marks or non-text inline nodes by default
  code_block: {
    attrs: { 
      ychange: { default: null },
      language: { default: null }
    },
    content: 'text*',
    marks: '',
    group: 'block',
    code: true,
    defining: true,
    parseDOM: [{ 
      tag: 'pre', 
      preserveWhitespace: 'full',
      getAttrs(node: HTMLElement) {
        const codeEl = node.querySelector('code');
        let language = null;
        if (codeEl) {
          const className = codeEl.className;
          const match = /language-(\S+)/.exec(className);
          if (match) {
            language = match[1];
          }
        }
        return { language };
      }
    }],
    toDOM(node) { 
      const attrs = calcYchangeDomAttrs(node.attrs);
      if (node.attrs.language) {
        attrs['data-language'] = node.attrs.language;
      }
      const codeAttrs: Record<string, string> = {};
      if (node.attrs.language) {
        codeAttrs.class = `language-${node.attrs.language}`;
      }
      return ['pre', attrs, ['code', codeAttrs, 0]]; 
    }
  },
  
  // The text node
  text: {
    group: 'inline'
  },
  
  // An inline image (<img>) node.
  //
  // `width` (px) and `align` are the resize/adjust state, persisted as
  // defaulted attrs so documents authored before they existed load
  // unchanged, and remote peers receive them through the normal Yjs attr
  // sync. Height is never stored: the natural aspect ratio derives it,
  // and a `max-width: 100%` style guard keeps any stored width inside
  // the container. The node deliberately stays `inline`/`group: inline`:
  // changing those against documents already stored in Yjs is a breaking
  // schema change with no migration path.
  image: {
    inline: true,
    attrs: {
      ychange: { default: null },
      src: {},
      alt: { default: null },
      title: { default: null },
      /** Rendered width in px; null = natural size. */
      width: { default: null },
      /** 'left' | 'center' | 'right'; null = inline flow. */
      align: { default: null }
    },
    group: 'inline',
    draggable: true,
    parseDOM: [{
      tag: 'img[src]',
      getAttrs(dom: HTMLElement) {
        const styleWidth = /(?:^|;)\s*width:\s*(\d+(?:\.\d+)?)px/.exec(
          dom.getAttribute('style') ?? ''
        );
        const attrWidth = dom.getAttribute('width');
        const width = styleWidth
          ? Math.round(Number(styleWidth[1]))
          : attrWidth && /^\d+$/.test(attrWidth)
            ? Number(attrWidth)
            : null;
        const align = dom.getAttribute('data-align');
        return {
          // Store the portable path, never this platform's rendered URL: a
          // paste inside the app re-enters here, and a `nosdesk-asset://` src
          // written into the shared document would break that image for every
          // other client.
          src: assetPath(dom.getAttribute('src') ?? ''),
          title: dom.getAttribute('title'),
          alt: dom.getAttribute('alt'),
          width,
          align: align === 'left' || align === 'center' || align === 'right' ? align : null
        };
      }
    }],
    toDOM(node) {
      const domAttrs: Record<string, string> = {
        src: assetUrl(node.attrs.src),
        title: node.attrs.title,
        alt: node.attrs.alt
      };
      if (node.attrs.width != null) {
        domAttrs.style = `width: ${node.attrs.width}px`;
      }
      if (node.attrs.align) {
        domAttrs['data-align'] = node.attrs.align;
      }
      return ['img', calcYchangeDomAttrs(node.attrs, domAttrs)];
    }
  },
  
  // A hard line break, represented in the DOM as <br>
  hard_break: {
    inline: true,
    group: 'inline',
    selectable: false,
    parseDOM: [{ tag: 'br' }],
    toDOM() { return brDOM; }
  },

  // A ticket link card - renders as an inline preview card
  ticket_link: {
    inline: true,
    attrs: {
      ychange: { default: null },
      ticketId: {},
      href: {}
    },
    group: 'inline',
    draggable: true,
    atom: true, // Treated as a single unit, not editable
    parseDOM: [{
      tag: 'span[data-ticket-link]',
      getAttrs(dom: HTMLElement) {
        return {
          ticketId: dom.getAttribute('data-ticket-id'),
          href: dom.getAttribute('data-href')
        };
      }
    }],
    toDOM(node) {
      const domAttrs = calcYchangeDomAttrs(node.attrs, {
        'data-ticket-link': 'true',
        'data-ticket-id': node.attrs.ticketId,
        'data-href': node.attrs.href,
        'class': 'ticket-link-card',
        'contenteditable': 'false'
      });
      return ['span', domAttrs];
    }
  },
  
  // A mention node - renders as an inline @user chip with avatar
  mention: {
    inline: true,
    attrs: {
      ychange: { default: null },
      uuid: {},
      name: {},
      avatarUrl: { default: null }
    },
    group: 'inline',
    draggable: true,
    atom: true, // Treated as a single unit, not editable
    parseDOM: [{
      tag: 'span[data-mention]',
      getAttrs(dom: HTMLElement) {
        return {
          uuid: dom.getAttribute('data-uuid'),
          name: dom.getAttribute('data-name'),
          avatarUrl: dom.getAttribute('data-avatar-url')
        };
      }
    }],
    toDOM(node) {
      const domAttrs = calcYchangeDomAttrs(node.attrs, {
        'data-mention': 'true',
        'data-uuid': node.attrs.uuid,
        'data-name': node.attrs.name,
        'data-avatar-url': node.attrs.avatarUrl || '',
        'class': 'mention-chip',
        'contenteditable': 'false'
      });
      return ['span', domAttrs, `@${node.attrs.name}`];
    }
  },

  // For lists
  bullet_list: {
    attrs: { ychange: { default: null } },
    content: 'list_item+',
    group: 'block',
    parseDOM: [{ tag: 'ul' }],
    toDOM(node) { return ['ul', calcYchangeDomAttrs(node.attrs), 0]; }
  },
  
  ordered_list: {
    attrs: { 
      ychange: { default: null },
      order: { default: 1 }
    },
    content: 'list_item+',
    group: 'block',
    parseDOM: [{ 
      tag: 'ol',
      getAttrs(dom: HTMLElement) {
        return { 
          order: dom.hasAttribute('start') ? parseInt(dom.getAttribute('start') || '1', 10) : 1
        };
      }
    }],
    toDOM(node) {
      const attrs = calcYchangeDomAttrs(node.attrs);
      if (node.attrs.order !== 1) {
        attrs.start = node.attrs.order;
      }
      return ['ol', attrs, 0];
    }
  },
  
  list_item: {
    attrs: { ychange: { default: null } },
    content: 'paragraph block*',
    defining: true,
    parseDOM: [{ tag: 'li' }],
    toDOM(node) { return ['li', calcYchangeDomAttrs(node.attrs), 0]; }
  },

  // An embedded document: a read-only preview of another page's saved content
  embedded_document: {
    attrs: {
      ychange: { default: null },
      documentUuid: {},
      documentTitle: { default: 'Untitled' }
    },
    group: 'block',
    atom: true,
    draggable: true,
    selectable: true,
    parseDOM: [{
      tag: 'div[data-embedded-document]',
      getAttrs(dom: HTMLElement) {
        return {
          documentUuid: dom.getAttribute('data-document-uuid'),
          documentTitle: dom.getAttribute('data-document-title') || 'Untitled'
        };
      }
    }],
    toDOM(node) {
      const domAttrs = calcYchangeDomAttrs(node.attrs, {
        'data-embedded-document': 'true',
        'data-document-uuid': node.attrs.documentUuid,
        'data-document-title': node.attrs.documentTitle,
        'class': 'embedded-document-block',
        'contenteditable': 'false'
      });
      return ['div', domAttrs, node.attrs.documentTitle];
    }
  }
};

const emDOM: DOMOutputSpec = ['em', 0];
const strongDOM: DOMOutputSpec = ['strong', 0];
const codeDOM: DOMOutputSpec = ['code', 0];

// Specs for the marks in the schema
export const marks: {[key: string]: MarkSpec} = {
  // A link. Has `href` and `title` attributes
  link: {
    attrs: {
      href: {},
      title: { default: null }
    },
    inclusive: false,
    parseDOM: [{
      tag: 'a[href]',
      getAttrs(dom: HTMLElement) {
        return { href: dom.getAttribute('href'), title: dom.getAttribute('title') };
      }
    }],
    toDOM(mark) { return ['a', mark.attrs, 0]; }
  },
  
  // An emphasis mark. Rendered as an <em> element
  em: {
    parseDOM: [
      { tag: 'i' },
      { tag: 'em' },
      { style: 'font-style=italic' }
    ],
    toDOM() { return emDOM; }
  },
  
  // A strong mark. Rendered as <strong>
  strong: {
    parseDOM: [
      { tag: 'strong' },
      // This works around a Google Docs misbehavior where
      // pasted content will be inexplicably wrapped in <b>
      // tags with a font-weight normal.
      { tag: 'b', getAttrs: (node: HTMLElement) => node.style.fontWeight !== 'normal' && null },
      { style: 'font-weight', getAttrs: (value: string) => /^(bold(er)?|[5-9]\d{2,})$/.test(value) && null }
    ],
    toDOM() { return strongDOM; }
  },
  
  // Code font mark. Represented as a <code> element
  code: {
    parseDOM: [{ tag: 'code' }],
    toDOM() { return codeDOM; }
  },
  
  // The ychange mark is used to track changes
  ychange: {
    attrs: {
      user: { default: null },
      state: { default: null }
    },
    inclusive: false,
    parseDOM: [{ tag: 'ychange' }],
    toDOM(mark) {
      return ['ychange', { ychange_user: mark.attrs.user, ychange_state: mark.attrs.state }, 0];
    }
  }
};

const CELL_ALIGNMENTS = ['left', 'center', 'right'] as const;
type CellAlign = (typeof CELL_ALIGNMENTS)[number];

function cellAlign(value: unknown): CellAlign | null {
  return CELL_ALIGNMENTS.includes(value as CellAlign) ? (value as CellAlign) : null;
}

// `align` is validated as a string only: a value a later version adds must not
// make this client throw on the cell, because y-prosemirror deletes what it
// can't build. Values it doesn't know render unaligned.
const baseTableNodes = tableNodes({
  cellContent: 'cell_block+',
  cellAttributes: {
    align: {
      default: null,
      validate: 'string|null',
      getFromDOM(dom: HTMLElement) {
        const fromClass = /(?:^|\s)align-(left|center|right)(?:\s|$)/.exec(dom.className);
        return cellAlign(fromClass?.[1])
          ?? cellAlign(dom.style.textAlign)
          ?? cellAlign(dom.getAttribute('align'));
      },
    },
  },
});

/** The first row's column widths, one entry per column (null = unset). */
function columnWidths(table: ProseMirrorNode): (number | null)[] {
  const widths: (number | null)[] = [];
  let columns = 0;
  table.forEach((row, _offset, index) => {
    let rowColumns = 0;
    row.forEach((cell) => {
      const span = Number(cell.attrs.colspan) || 1;
      const colwidth = cell.attrs.colwidth as number[] | null;
      for (let i = 0; i < span; i++) {
        if (index === 0) widths.push(colwidth?.[i] ?? null);
        rowColumns++;
      }
    });
    columns = Math.max(columns, rowColumns);
  });
  while (widths.length < columns) widths.push(null);
  return widths;
}

function cellSpec(base: NodeSpec): NodeSpec {
  return {
    ...base,
    attrs: { ...base.attrs, ychange: { default: null } },
    toDOM(node) {
      const [tag, attrs] = base.toDOM!(node) as [string, Record<string, string>, 0];
      const align = cellAlign(node.attrs.align);
      const domAttrs = { ...attrs };
      if (align) domAttrs.class = `align-${align}`;
      return [tag, calcYchangeDomAttrs(node.attrs, domAttrs), 0];
    },
  };
}

// Tables live at the top level only: `table` is outside the `block` group, so
// no list, quote or cell can hold one. Cells hold `cell_block`s.
const tableSpecs: {[key: string]: NodeSpec} = {
  table: {
    ...baseTableNodes.table,
    attrs: { ychange: { default: null } },
    toDOM(node) {
      const cols = columnWidths(node).map((width): DOMOutputSpec =>
        width != null && width > 0 ? ['col', { width: String(width) }] : ['col']
      );
      return [
        'div',
        { class: 'table-scroll' },
        ['table', calcYchangeDomAttrs(node.attrs), ['colgroup', ...cols], ['tbody', 0]],
      ];
    },
  },
  table_row: {
    ...baseTableNodes.table_row,
    attrs: { ychange: { default: null } },
    toDOM(node) { return ['tr', calcYchangeDomAttrs(node.attrs), 0]; },
  },
  table_cell: cellSpec(baseTableNodes.table_cell),
  table_header: cellSpec(baseTableNodes.table_header),
};

const CELL_BLOCK_NODES = ['paragraph', 'bullet_list', 'ordered_list', 'code_block'];

// Specs for the documentation editor: the shared nodes, the `cell_block`
// group, and tables. Paragraph stays first so it is what `createAndFill`
// picks for a new block or cell.
export const nodes: {[key: string]: NodeSpec} = {
  ...Object.fromEntries(
    Object.entries(commentNodes).map(([name, spec]) => [
      name,
      CELL_BLOCK_NODES.includes(name) ? { ...spec, group: `${spec.group} cell_block` } : spec,
    ])
  ),
  doc: { content: '(block | table)+' },
  ...tableSpecs,
};

// The documentation and ticket-note editor (CollaborativeEditor), and every
// reader of saved pages. Roughly CommonMark, plus lists, mentions, ticket
// links, embeds and tables.
export const schema = new Schema({ nodes, marks });

// The comment composer (SimpleEditor). No tables: comments go out by email,
// where tables render unevenly, so a pasted table arrives as text.
export const commentSchema = new Schema({ nodes: commentNodes, marks }); 