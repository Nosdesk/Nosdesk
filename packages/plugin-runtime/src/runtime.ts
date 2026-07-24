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
import type { PluginModule } from '@nosdesk/plugin-sdk';

const token = new URLSearchParams(location.search).get('t');
const root = document.getElementById('root');

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

  // Built as a runtime string (not a literal) so the bundler treats it as an
  // external runtime import: the bundle is fetched from the sandbox origin at
  // load time, never bundled here.
  const bundleUrl = `./bundle?t=${encodeURIComponent(token)}`;
  const mod = (await import(/* @vite-ignore */ bundleUrl)) as { default?: PluginModule };
  if (!mod.default || typeof mod.default.mount !== 'function') {
    throw new Error('sandbox runtime: bundle has no default { mount } export');
  }
  const plugin = mod.default;

  let cleanup = plugin.mount(root, runtime.api, runtime.context) ?? undefined;
  observeHeight(root);

  // v1 context updates are coarse: re-mount on change. A plugin that wants
  // fine-grained updates can hold its own state and diff; a richer update
  // channel can come later without changing the mount contract.
  runtime.onContextChange((ctx) => {
    if (cleanup) cleanup();
    root.replaceChildren();
    cleanup = plugin.mount(root, runtime.api, ctx) ?? undefined;
  });
}

boot().catch((e: unknown) => {
  if (root) root.textContent = `plugin failed to load: ${String(e)}`;
});
