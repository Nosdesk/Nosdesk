import { Schema } from 'prosemirror-model';
import type { NodeSpec, MarkSpec, DOMOutputSpec } from 'prosemirror-model';

const brDOM: DOMOutputSpec = ['br'];

const calcYchangeDomAttrs = (attrs: any, domAttrs: any = {}) => {
  domAttrs = Object.assign({}, domAttrs);
  if (attrs.ychange !== null) {
    domAttrs.ychange_user = attrs.ychange.user;
    domAttrs.ychange_state = attrs.ychange.state;
  }
  return domAttrs;
};

// Specs for the nodes defined in this schema
export const nodes: {[key: string]: NodeSpec} = {
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
      const codeAttrs: any = {};
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
  
  // An inline image (<img>) node
  image: {
    inline: true,
    attrs: {
      ychange: { default: null },
      src: {},
      alt: { default: null },
      title: { default: null }
    },
    group: 'inline',
    draggable: true,
    parseDOM: [{
      tag: 'img[src]',
      getAttrs(dom: HTMLElement) {
        return {
          src: dom.getAttribute('src'),
          title: dom.getAttribute('title'),
          alt: dom.getAttribute('alt')
        };
      }
    }],
    toDOM(node) {
      const domAttrs = {
        src: node.attrs.src,
        title: node.attrs.title,
        alt: node.attrs.alt
      };
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

// This schema roughly corresponds to the document schema used by
// CommonMark, plus the list elements from prosemirror-schema-list
export const schema = new Schema({ nodes, marks }); 