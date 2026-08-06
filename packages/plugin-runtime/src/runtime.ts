// The Vue-free iframe runtime for a sandboxed plugin.
//
// Runs inside the opaque-sandbox iframe. It (1) establishes the Comlink bridge
// to the host via connectToHost(), (2) dynamically imports the token-authorized
// plugin bundle, and (3) mounts it against the framework-agnostic contract
// `export default { mount(rootEl, api, context) }`.
//
// Built (bundled with @nosdesk/plugin-sdk + comlink inlined) to dist/runtime.js
// and served by the backend at /__plugin-sandbox/runtime.js. It is NOT loadable
// standalone: connectToHost() resolves only once the host posts the init message
// with the transferred MessageChannel port (the host bridge, sandbox step 4).
import { connectToHost, reportHeight } from '@nosdesk/plugin-sdk';
import type { PluginInstance, PluginModule, PluginTheme } from '@nosdesk/plugin-sdk';
import { PLUGIN_UI_CSS } from './pluginUiCss';

/** Normalize the value a plugin's `mount` returns into a `PluginInstance`. */
function toInstance(result: void | (() => void) | PluginInstance): PluginInstance {
  if (typeof result === 'function') return { unmount: result };
  if (result && typeof result === 'object') return result;
  return {};
}

const token = new URLSearchParams(location.search).get('t');
const root = document.getElementById('root');

// Inject the static base UI kit once. Referencing `var(--nd-*)`, it renders
// against the host tokens (injected below) so a plugin's DOM matches the app.
function injectBaseCss(): void {
  if (document.getElementById('nd-base')) return;
  const style = document.createElement('style');
  style.id = 'nd-base';
  style.textContent = PLUGIN_UI_CSS;
  document.head.appendChild(style);
}

// Inject / replace the dynamic token values pushed by the host. Called on connect
// and on every theme change, so `--nd-*` (and the kit) track light/dark/named
// themes live. Also stamps the scheme/name for a plugin that must branch.
function injectTokens(theme: PluginTheme): void {
  let style = document.getElementById('nd-tokens');
  if (!style) {
    style = document.createElement('style');
    style.id = 'nd-tokens';
    document.head.appendChild(style);
  }
  const vars = Object.entries(theme.tokens)
    .map(([k, v]) => `  --nd-${k}: ${v};`)
    .join('\n');
  style.textContent = `:root {\n${vars}\n}`;
  document.documentElement.setAttribute('data-nd-color-scheme', theme.colorScheme);
  document.documentElement.setAttribute('data-nd-theme', theme.name);
}

/**
 * Sentinel height meaning "this plugin has content, but its height cannot be
 * measured right now". See the recovery path in `observeHeight`.
 *
 * The height wire contract is three-state:
 *   * `> 0` — the content height, in px.
 *   * `0`   — the plugin rendered nothing; the host collapses its chrome.
 *   * `-1`  — has content, height unknown; the host restores layout and waits.
 */
const HAS_CONTENT_UNMEASURED = -1;

/** Re-measure cadence after announcing HAS_CONTENT, and how many attempts
 *  before giving up. ~2s total: long enough to cover the host's un-hide and
 *  reflow, short enough that a genuinely zero-height plugin stops cheaply. */
const CHASE_INTERVAL_MS = 50;
const CHASE_MAX_TRIES = 40;

// Report content height to the host on every change so it can size the iframe
// (a cross-origin sandboxed iframe can't self-size). Deduped to avoid a resize
// feedback loop.
function observeHeight(el: HTMLElement): void {
  let last = -1;
  const report = (): void => {
    // "Drew nothing" is reported as an explicit 0 so the host can collapse the
    // whole contribution (chrome included) instead of leaving an empty card.
    // Deliberately measured from the CONTENT, not the height: a plugin whose
    // root measures 0 because the host collapsed the iframe must not be read as
    // empty, or the two would latch each other at zero and it could never grow
    // back. An empty root cannot feedback-loop, since it does not depend on the
    // iframe's own size.
    const isEmpty = el.children.length === 0 && !el.textContent?.trim();
    if (isEmpty) {
      if (last !== 0) {
        last = 0;
        reportHeight(0);
      }
      return;
    }
    const h = Math.ceil(el.getBoundingClientRect().height);
    if (h > 0) {
      if (h !== last) {
        last = h;
        reportHeight(h);
      }
      return;
    }
    // Non-empty but unmeasurable. This is the recovery path: after an empty
    // report the host hides the frame with `display: none`, which suspends
    // layout here, so a plugin that fills in after a fetch measures 0 and its
    // real height could never be reported. Mutations still fire while hidden,
    // so announce HAS_CONTENT and the host restores layout.
    if (last !== HAS_CONTENT_UNMEASURED) {
      last = HAS_CONTENT_UNMEASURED;
      reportHeight(HAS_CONTENT_UNMEASURED);
      // Then re-measure until it takes. The ResizeObserver above does NOT fire
      // when the host flips `display` back on — observed: the frame un-hid but
      // stayed at the iframe's default 150px forever — so the true height has
      // to be chased here rather than waited for. Bounded, so a plugin that is
      // legitimately zero-height doesn't spin.
      let tries = 0;
      const chase = (): void => {
        if (last !== HAS_CONTENT_UNMEASURED) return; // a real height landed
        const px = Math.ceil(el.getBoundingClientRect().height);
        if (px > 0) {
          last = px;
          reportHeight(px);
          return;
        }
        if (++tries < CHASE_MAX_TRIES) setTimeout(chase, CHASE_INTERVAL_MS);
      };
      // `setTimeout`, NOT `requestAnimationFrame`: rendering is suspended in a
      // `display: none` iframe, so a rAF callback queued here never runs and
      // the chase would silently do nothing (observed: the frame un-hid and
      // stayed at the iframe's default 150px). Timers still fire while hidden.
      setTimeout(chase, CHASE_INTERVAL_MS);
    }
  };
  new ResizeObserver(report).observe(el);
  // The ResizeObserver only fires on a size CHANGE, and a root that starts
  // empty and stays empty never changes size, so the initial 0 would never be
  // sent. A MutationObserver covers the empty <-> non-empty transitions that
  // carry no size change of their own.
  new MutationObserver(report).observe(el, {
    childList: true,
    subtree: true,
    characterData: true,
  });
  report();
}

async function boot(): Promise<void> {
  if (!root) throw new Error('sandbox runtime: no #root element');
  if (!token) throw new Error('sandbox runtime: missing bundle token');

  // Establish the bridge first so the plugin's mount receives a live host API.
  const runtime = await connectToHost();

  // Style the document to match the app before the plugin paints: the static kit
  // once, the host tokens now, and re-inject the tokens whenever the host theme
  // changes (the `--nd-*` variables and the kit update in place, no re-mount).
  injectBaseCss();
  injectTokens(runtime.theme);
  runtime.onThemeChange(injectTokens);

  // Built as a runtime string (not a literal) so the bundler treats it as an
  // external runtime import: the bundle is fetched from the sandbox origin at
  // load time, never bundled here.
  const bundleUrl = `./bundle?t=${encodeURIComponent(token)}`;
  let mod: { default?: PluginModule };
  try {
    mod = (await import(/* @vite-ignore */ bundleUrl)) as { default?: PluginModule };
  } catch (e) {
    // The bundle fetch failed — most often the ~60s bundle token expired before
    // an iframe reload (bfcache eviction / renderer crash). Ask the host to
    // re-mint a fresh token and reload us, rather than dead-ending on a 403.
    window.parent.postMessage({ type: 'nosdesk-plugin-bundle-error' }, '*');
    throw e;
  }
  if (!mod.default || typeof mod.default.mount !== 'function') {
    throw new Error('sandbox runtime: bundle has no default { mount } export');
  }
  const plugin = mod.default;

  const mounted = plugin.mount(root, runtime.api, runtime.context);
  let instance = toInstance(mounted);
  // Start height/emptiness reporting only once the mount has SETTLED. `mount`
  // is commonly async (it awaits host-API calls before appending anything), and
  // observing immediately measured a root that was merely still rendering as
  // "the plugin drew nothing" — every async plugin reported empty within ~200ms
  // of load and the host collapsed its chrome before it ever painted.
  void Promise.resolve(mounted)
    .catch(() => {
      // A failed mount still needs observation: it reports empty, which is the
      // honest signal, and the host collapses rather than framing a blank card.
    })
    .then(() => observeHeight(root));

  // On context change (ticket/device/action), prefer the plugin's in-place
  // `update` (no re-mount, keeps state — needed for action signals); fall back to
  // unmount + re-mount for simple plugins that returned void / a cleanup fn.
  runtime.onContextChange((ctx) => {
    if (instance.update) {
      instance.update(ctx);
      return;
    }
    instance.unmount?.();
    // Drop the prior mount's api.on subscriptions so a simple plugin that
    // subscribes in mount doesn't accumulate handlers (and duplicate deliveries)
    // across re-mounts.
    runtime.resetEvents();
    root.replaceChildren();
    instance = toInstance(plugin.mount(root, runtime.api, ctx));
  });
}

boot().catch((e: unknown) => {
  if (root) root.textContent = `plugin failed to load: ${String(e)}`;
});
