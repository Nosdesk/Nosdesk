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

// Report content height to the host on every change so it can size the iframe
// (a cross-origin sandboxed iframe can't self-size). Deduped to avoid a resize
// feedback loop.
function observeHeight(el: HTMLElement): void {
  let last = -1;
  const report = (): void => {
    const h = Math.ceil(el.getBoundingClientRect().height);
    if (h > 0 && h !== last) {
      last = h;
      reportHeight(h);
    }
  };
  new ResizeObserver(report).observe(el);
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

  let instance = toInstance(plugin.mount(root, runtime.api, runtime.context));
  observeHeight(root);

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
