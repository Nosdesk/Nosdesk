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
import type {
  PluginContext,
  PluginInstance,
  PluginModule,
  PluginTheme,
} from '@nosdesk/plugin-sdk';
import { PLUGIN_UI_CSS } from './pluginUiCss';
import {
  HAS_CONTENT_UNMEASURED,
  containerSize,
  decideHeightReport,
} from './heightProtocol';

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

// --- Responsive signals ------------------------------------------------------
//
// Two independent axes, and conflating them is the trap:
//
//   * `data-nd-container` — how wide THIS PANEL is. Measured here, because the
//     iframe's viewport IS the panel's container. This is what a plugin should
//     lay itself out against, and it is also why a plugin's own media queries
//     behave like container queries: `@media (min-width: 768px)` is false in a
//     336px sidebar panel no matter how wide the display is. That is correct,
//     and usually what you want, but it surprises authors who expect "desktop".
//
//   * `data-nd-app-breakpoint` — what the APP around the panel is doing. Cannot
//     be measured from in here (a sidebar panel is the same width on a phone as
//     on a 4K display), so the host pushes it on the context channel. Use it
//     only to match an app-level decision.
//
//   * `data-nd-pointer` — touch or mouse. Resolved HERE, not pushed: pointer is
//     a device capability, so `matchMedia` answers correctly inside the iframe.
//     Mirroring it from the host would add a second source of truth that can
//     only go stale.
//
// All three are stamped on `<html>` so plugin CSS can select on them, and the
// app breakpoint is also on `context.layout` for JS.

// Container bucketing and the height-report rules are pure, and subtle enough
// to have caused two bugs, so they live in ./heightProtocol where they can be
// unit-tested directly. This module keeps only the DOM wiring.

/** Stamp the panel's own width onto `<html>`, live. `--nd-container-width` is
 *  the exact px for plugins that need to compute; `data-nd-container` is the
 *  bucket for CSS selectors. */
function observeContainer(): void {
  const el = document.documentElement;
  let lastWidth = -1;
  let lastBucket = '';
  const apply = (): void => {
    const width = el.clientWidth;
    // Guarded: a ResizeObserver fires per frame through a host drag, and every
    // write here invalidates style for anything reading the variable. Writing
    // only on change also keeps a plugin that SIZES itself from
    // `--nd-container-width` from feeding its own observer.
    if (width === lastWidth) return;
    lastWidth = width;
    el.style.setProperty('--nd-container-width', `${width}px`);
    const bucket = containerSize(width);
    if (bucket !== lastBucket) {
      lastBucket = bucket;
      el.setAttribute('data-nd-container', bucket);
    }
  };
  new ResizeObserver(apply).observe(el);
  apply();
}

/** Stamp the pointer type, live. A device can gain or lose a fine pointer mid
 *  session (tablet docked to a trackpad), and the listener costs nothing. */
function observePointer(): void {
  if (!window.matchMedia) return;
  const mq = window.matchMedia('(pointer: coarse)');
  const apply = (): void =>
    document.documentElement.setAttribute('data-nd-pointer', mq.matches ? 'coarse' : 'fine');
  mq.addEventListener('change', apply);
  apply();
}

/** Stamp the host's app breakpoint. Called on every context push; the value is
 *  bucketed host-side, so this is a handful of writes, not a resize firehose. */
function applyLayout(context: PluginContext): void {
  if (!context.layout) return;
  document.documentElement.setAttribute('data-nd-app-breakpoint', context.layout.breakpoint);
}

/** Re-measure cadence after announcing HAS_CONTENT, and how many attempts
 *  before giving up. ~2s total: long enough to cover the host's un-hide and
 *  reflow, short enough that a genuinely zero-height plugin stops cheaply. */
const CHASE_INTERVAL_MS = 50;
const CHASE_MAX_TRIES = 40;

/** How long to wait for `mount` to settle before observing height anyway. */
const MOUNT_SETTLE_TIMEOUT_MS = 3000;

// Report content height to the host on every change so it can size the iframe
// (a cross-origin sandboxed iframe can't self-size). Deduped to avoid a resize
// feedback loop.
function observeHeight(el: HTMLElement): void {
  // `null`, not -1: -1 IS the HAS_CONTENT_UNMEASURED sentinel, so seeding with
  // it would make the "already reported unmeasured" guard below true on the
  // very first measurement and swallow the report. A panel whose first measure
  // is non-empty but unmeasurable (mounted inside an already-hidden container)
  // would then never announce itself and would sit at the default height.
  let last: number | null = null;
  const measure = (): number => Math.ceil(el.getBoundingClientRect().height);
  // Emptiness is read from CONTENT, never from height: a root that measures 0
  // only because the host collapsed the frame must not read as empty, or the
  // two latch each other at zero and it can never grow back.
  const isEmpty = (): boolean => el.children.length === 0 && !el.textContent?.trim();

  const report = (): void => {
    const decision = decideHeightReport({
      isEmpty: isEmpty(),
      measuredPx: measure(),
      last,
    });
    last = decision.last;
    if (decision.report !== null) reportHeight(decision.report);
    if (!decision.chase) return;

    // Re-measure until it takes. The ResizeObserver does NOT fire when the host
    // flips `display` back on — observed: the frame un-hid but stayed at the
    // iframe's default 150px forever — so the true height has to be chased
    // rather than waited for. Bounded, so a genuinely zero-height plugin stops.
    let tries = 0;
    const chase = (): void => {
      if (last !== HAS_CONTENT_UNMEASURED) return; // a real height landed
      const px = measure();
      if (px > 0) {
        last = px;
        reportHeight(px);
        return;
      }
      if (++tries < CHASE_MAX_TRIES) setTimeout(chase, CHASE_INTERVAL_MS);
    };
    // `setTimeout`, NOT `requestAnimationFrame`: rendering is suspended in a
    // `display: none` iframe, so a rAF callback queued here never runs and the
    // chase would silently do nothing. Timers still fire while hidden.
    setTimeout(chase, CHASE_INTERVAL_MS);
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

  // Responsive signals, both stamped before the plugin paints so its first
  // render already sees the right container bucket and app breakpoint rather
  // than laying out once and reflowing.
  observeContainer();
  observePointer();
  applyLayout(runtime.context);

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
  void Promise.race([
    Promise.resolve(mounted).catch(() => {
      // A failed mount still needs observation: it reports empty, which is the
      // honest signal, and the host collapses rather than framing a blank card.
    }),
    // A mount that never settles must not disable height reporting outright.
    // Awaiting it unconditionally means one hung await inside a plugin (a host
    // call that never resolves) leaves the frame stuck at the iframe's default
    // 150px forever, chrome and all. Observed with a real bundle, so this is a
    // failure mode plugins hit in practice, not a theoretical one.
    new Promise<void>((resolve) => setTimeout(resolve, MOUNT_SETTLE_TIMEOUT_MS)),
  ]).then(() => observeHeight(root));

  // On context change (ticket/device/action), prefer the plugin's in-place
  // `update` (no re-mount, keeps state — needed for action signals); fall back to
  // unmount + re-mount for simple plugins that returned void / a cleanup fn.
  runtime.onContextChange((ctx) => {
    applyLayout(ctx);
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
