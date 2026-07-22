// Minimal step-1 sandbox runtime.
//
// Reads the bundle token from the iframe URL, imports the plugin bundle
// cross-origin (token-authorized, no cookies), and mounts it via the
// framework-agnostic contract `export default { mount(rootEl, api, context) }`.
//
// The Comlink host-API bridge + host-side scope enforcement are steps 3-5;
// `api`/`context` are placeholders here so a real bundle can already render in
// the sandbox end to end.
const token = new URLSearchParams(location.search).get('t');
const root = document.getElementById('root');

try {
  if (!token) throw new Error('missing bundle token');
  const mod = await import(`./bundle?t=${encodeURIComponent(token)}`);
  if (!mod.default || typeof mod.default.mount !== 'function') {
    throw new Error('bundle has no default { mount } export');
  }
  mod.default.mount(root, /* api */ {}, /* context */ {});
} catch (e) {
  root.textContent = 'plugin failed to load: ' + e;
}
