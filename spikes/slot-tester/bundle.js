// Slot Tester — a dev-only plugin bundle that renders a distinct, obviously
// working panel on every UI slot. One `mount` switches on the slot the host is
// rendering (context.component.slot) and exercises the host API + the context
// each surface provides.
//
// Framework-free on purpose: the sandbox runtime calls `default.mount(root, api,
// context)` with a Comlink-proxied host API (every call is an async round-trip).

function el(tag, props, children) {
  const node = document.createElement(tag);
  if (props) {
    for (const [k, v] of Object.entries(props)) {
      if (k === 'style') node.setAttribute('style', v);
      else if (k === 'text') node.textContent = v;
      else node.setAttribute(k, v);
    }
  }
  for (const c of children || []) node.appendChild(typeof c === 'string' ? document.createTextNode(c) : c);
  return node;
}

// No FONT constant any more: the runtime's injected kit already sets the host
// font, size and line-height on `body`, so restating them here only risked
// drifting from the app.

// The panel body. Deliberately draws NO outer frame: on a `card`-chrome slot
// the host wraps this document in the app's own SectionCard (border, radius,
// surface, header pill, 12px body padding), so a border here would render a
// card inside a card. Content starts flush at the top-left and inherits the
// kit's typography.
//
// `fullBleedPanel` below is the counter-example: it declares `chrome: "none"`
// and therefore does draw its own frame.
function panel(accent, title) {
  const root = el('div', { style: 'color: inherit;' });
  // A slot label, kept because this is a test vehicle and the point is to see
  // WHICH slot rendered. A real plugin would let the host header carry it.
  root.appendChild(
    el('div', {
      style: `font-weight: 600; color: ${accent}; margin-bottom: var(--nd-space-xs, 4px);`,
      text: title,
    }),
  );
  return root;
}

function line(label, value) {
  return el('div', { style: 'margin: 2px 0;' }, [
    el('span', { style: 'opacity: 0.6;', text: `${label}: ` }),
    el('span', { style: 'font-weight: 500;', text: String(value) }),
  ]);
}

// Run an async host call and render its result / error into `slot`.
async function probe(slot, label, fn) {
  const row = el('div', { style: 'margin-top: 6px; font-family: ui-monospace, monospace; font-size: 12px;' });
  row.textContent = `${label}: …`;
  slot.appendChild(row);
  try {
    row.textContent = `${label}: ${await fn()}`;
  } catch (e) {
    row.textContent = `${label}: ERROR ${e && e.code ? `(${e.code})` : ''} ${e && e.message ? e.message : e}`;
    row.setAttribute('style', row.getAttribute('style') + '; color: #dc2626;');
  }
}

export default {
  async mount(root, api, context) {
    const slot = context.component.slot;
    const name = context.component.name;

    // --- Host-chrome test vehicles -----------------------------------------
    // These three exist to exercise the host chrome, not the plugin API.

    // Renders nothing, ever. The host should collapse the whole contribution,
    // chrome included, leaving no empty card and no stray flex gap.
    if (name === 'emptyPanel') return;

    // Renders nothing at first, then fills in. This is the recovery path: the
    // host hides an empty contribution with `display: none`, which suspends
    // layout in here, so the runtime has to signal "I have content now" for the
    // host to restore layout and pick up the real height. If this one never
    // appears, that path is broken.
    if (name === 'latePanel') {
      setTimeout(() => {
        const late = panel('#0d9488', 'latePanel (filled in after 1.5s)');
        late.appendChild(line('info', 'If you can read this, empty -> filled recovery works.'));
        root.appendChild(late);
      }, 1500);
      return;
    }

    // Never settles. A plugin that awaits something that never resolves must
    // not disable height reporting: the runtime observes anyway after its
    // settle timeout, so this should collapse like any other empty panel
    // rather than sit at the iframe's default height forever.
    if (name === 'hangPanel') {
      await new Promise(() => {});
      return;
    }

    // Declares `chrome: "none"`, so the host mounts it bare and it owns its
    // frame. Edge-to-edge on purpose: nothing should inset it.
    if (name === 'fullBleedPanel') {
      const bleed = el('div', {
        style:
          'background: linear-gradient(90deg, #0891b2, #7c3aed); color: #fff; padding: 12px; border-radius: var(--nd-radius-lg, 12px); font-weight: 600;',
      });
      bleed.textContent = 'fullBleedPanel — chrome: "none", draws its own frame';
      root.appendChild(bleed);
      return;
    }

    const ACCENTS = {
      'ticket.sidebar.panel': '#2563eb',
      'asset.info.panel': '#0891b2',
      'settings.integrations.page': '#7c3aed',
      'dashboard.widget': '#059669',
      'ticket.header.action': '#d97706',
      'ticket.bulk.action': '#db2777',
      'nav.item': '#4f46e5',
    };
    const accent = ACCENTS[slot] || '#64748b';
    const box = panel(accent, `${slot}`);

    // Responsive signals, echoed so a human (and the verification script) can
    // see the panel's own bucket and the app's breakpoint side by side.
    const de = document.documentElement;
    const showLayout = () => {
      layoutRow.textContent =
        `container=${de.getAttribute('data-nd-container')} ` +
        `(${getComputedStyle(de).getPropertyValue('--nd-container-width').trim()}) ` +
        `app=${de.getAttribute('data-nd-app-breakpoint')} ` +
        `pointer=${de.getAttribute('data-nd-pointer')}`;
    };
    const layoutRow = el('div', {
      class: 'nd-layout-probe',
      style: 'margin: 2px 0; font-family: ui-monospace, monospace; font-size: 12px;',
    });
    box.appendChild(layoutRow);
    showLayout();
    // The container bucket changes without a context push, so watch for it.
    new ResizeObserver(showLayout).observe(de);

    // `api` is a Comlink proxy: even plain data properties resolve as promises,
    // so these cannot be concatenated directly (that threw a TypeError out of
    // `mount`). Filled in asynchronously rather than awaited, because `mount`
    // must settle promptly: the runtime defers height/emptiness observation
    // until it does, so a mount that blocks here reports no height at all and
    // the panel sits at the iframe's default size.
    const apiRow = line('runtime api', '...');
    box.appendChild(apiRow);
    Promise.all([api.version, api.plugin])
      .then(([v, meta]) => {
        apiRow.textContent = `api v${v} — ${meta && meta.name} v${meta && meta.version}`;
      })
      .catch((e) => {
        apiRow.textContent = `api: ERROR ${e && e.message ? e.message : e}`;
      });

    if (slot === 'ticket.sidebar.panel' || slot === 'ticket.header.action') {
      const t = context.ticket;
      box.appendChild(line('ticket (from context)', t ? `#${t.id} — ${t.title}` : 'none'));
      if (t) {
        await probe(box, 'api.tickets.get(id).title', async () => {
          const fetched = await api.tickets.get(t.id);
          return fetched ? fetched.title : 'null';
        });
      }
      // storage round-trip
      await probe(box, 'storage round-trip', async () => {
        await api.storage.set('probe', { at: 'ticket', n: (context.ticket && context.ticket.id) || 0 });
        const got = await api.storage.get('probe');
        return JSON.stringify(got);
      });
    }

    if (slot === 'asset.info.panel') {
      const a = context.asset;
      box.appendChild(line('asset (from context)', a ? `#${a.id} — ${a.name}` : 'none'));
      if (a) {
        await probe(box, 'api.assets.get(id).name', async () => {
          const fetched = await api.assets.get(a.id);
          return fetched ? fetched.name : 'null';
        });
      }
    }

    if (slot === 'ticket.bulk.action') {
      const ids = context.ticketIds || [];
      box.appendChild(line('selected ticket ids', ids.length ? ids.join(', ') : '(none)'));
      box.appendChild(line('selection size', ids.length));
    }

    if (slot === 'dashboard.widget') {
      await probe(box, 'api.tickets.list().length', async () => (await api.tickets.list()).length);
      await probe(box, 'api.users.me().name', async () => {
        const me = await api.users.me();
        return me ? me.name : 'null';
      });
    }

    if (slot === 'settings.integrations.page') {
      box.appendChild(line('info', 'This is the plugin-rendered config page.'));
      await probe(box, 'api.settings.get(greeting)', async () => {
        const v = await api.settings.get('greeting');
        return JSON.stringify(v);
      });
    }

    if (slot === 'nav.item') {
      box.appendChild(line('info', 'Full-page plugin surface (a nav.item route).'));
      await probe(box, 'api.tickets.list().length', async () => (await api.tickets.list()).length);
    }

    // A live control to prove the bundle is interactive, not a static render.
    // Uses the kit's `.nd-btn` rather than a hand-rolled button, so it also
    // proves the injected UI kit matches the host's button styling.
    let clicks = 0;
    const btn = el('button', {
      class: 'nd-btn',
      style: 'margin-top: var(--nd-space-sm, 8px);',
      text: 'Click me (0)',
    });
    btn.addEventListener('click', () => {
      clicks += 1;
      btn.textContent = `Click me (${clicks})`;
    });
    box.appendChild(btn);

    root.appendChild(box);

    // React to context updates without a full re-mount (e.g. the ticket the
    // sidebar shows changing, or the action counter bumping).
    return {
      update(next) {
        if (next.actionActivated !== undefined && next.actionActivated !== context.actionActivated) {
          const bump = el('div', {
            style: `margin-top: 6px; color: ${accent}; font-weight: 600;`,
            text: `actionActivated -> ${next.actionActivated}`,
          });
          box.appendChild(bump);
        }
        context = next;
      },
    };
  },
};
