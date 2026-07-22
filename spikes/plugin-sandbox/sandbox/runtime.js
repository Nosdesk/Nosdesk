// The iframe-side runtime: receives the bridge port, imports the plugin bundle,
// calls its mount(), and answers isolation self-checks over the port.
let port;
let hostOrigin;

window.addEventListener('message', async (e) => {
  const m = e.data;
  if (!m || m.type !== 'init' || !e.ports || !e.ports[0]) return;
  port = e.ports[0];
  hostOrigin = m.hostOrigin;
  port.onmessage = onPortMessage;

  try {
    // M1: cross-origin ES-module import under the opaque origin + explicit-host
    // CSP + CORS. This is the make-or-break load mechanic.
    const mod = await import('./bundle.js');
    const api = {}; // real bridge Remote<PluginAPI> goes here later
    const context = {};
    const mountResult = mod.default.mount(document.getElementById('root'), api, context);
    port.postMessage({ type: 'mounted', ok: true, info: JSON.stringify(mountResult ?? 'ok') });
  } catch (err) {
    port.postMessage({ type: 'mounted', ok: false, info: 'import/mount failed: ' + String(err).slice(0, 120) });
  }
});

function probe(fn) {
  try {
    fn();
    return { blocked: false, info: 'ACCESSIBLE (unexpected)' };
  } catch (err) {
    return { blocked: true, info: String(err.name || err).slice(0, 50) };
  }
}

async function onPortMessage(e) {
  if (e.data.type !== 'selfcheck') return;
  const cookie = probe(() => void document.cookie);
  const storage = probe(() => void window.localStorage.length);
  let hostFetch;
  try {
    await fetch(hostOrigin + '/host.html', { mode: 'no-cors' });
    hostFetch = { blocked: false, info: 'fetch SUCCEEDED (unexpected — connect-src none should block)' };
  } catch (err) {
    hostFetch = { blocked: true, info: String(err.name || err).slice(0, 50) };
  }
  port.postMessage({ type: 'selfcheck', cookie, storage, hostFetch });
}
