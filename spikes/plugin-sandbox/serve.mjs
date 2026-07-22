// Throwaway two-origin harness for the plugin-sandbox spike. No deps.
//   host    origin: http://localhost:5310  (the "app")
//   sandbox origin: http://127.0.0.1:5311  (serves the plugin runtime + bundle)
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const DIR = dirname(fileURLToPath(import.meta.url));
const HOST_PORT = 5310;
const SANDBOX_PORT = 5311;
const HOST_ORIGIN = `http://localhost:${HOST_PORT}`;
const SANDBOX_ORIGIN = `http://127.0.0.1:${SANDBOX_PORT}`;
const VALID_TOKEN = 'spike-token';

async function send(res, status, headers, bodyPath) {
  try {
    const body = bodyPath ? await readFile(join(DIR, bodyPath)) : '';
    res.writeHead(status, headers);
    res.end(body);
  } catch (e) {
    res.writeHead(500);
    res.end(String(e));
  }
}

// Runtime-page CSP built from the host the client used (works on localhost or a
// LAN IP). Shared by the separate-origin and same-origin variants.
function runtimeHeaders(req) {
  const sbHost = `http://${req.headers.host}`;
  return {
    'Content-Type': 'text/html',
    'Content-Security-Policy':
      `default-src 'none'; script-src ${sbHost}; style-src 'unsafe-inline'; connect-src 'none';`,
    'Cross-Origin-Resource-Policy': 'cross-origin',
  };
}
const jsHeaders = {
  'Content-Type': 'text/javascript',
  'Access-Control-Allow-Origin': '*',
  'Cross-Origin-Resource-Policy': 'cross-origin',
};

// ---- host ("app") origin -------------------------------------------------
createServer((req, res) => {
  const url = new URL(req.url, HOST_ORIGIN);
  const p = url.pathname;
  if (p === '/' || p === '/host.html') {
    return send(res, 200, { 'Content-Type': 'text/html' }, 'host.html');
  }
  // Same-origin variant (the zero-config self-host path): serve the runtime +
  // bundle from THIS host origin under /so/. Proves a same-origin `src` still
  // opaque-ifies under sandbox="allow-scripts" (document.cookie must still throw).
  if (p === '/so/runtime.html') {
    if (url.searchParams.get('token') !== VALID_TOKEN) {
      res.writeHead(403);
      return res.end('bad token');
    }
    return send(res, 200, runtimeHeaders(req), 'sandbox/runtime.html');
  }
  if (p === '/so/runtime.js' || p === '/so/bundle.js') {
    return send(res, 200, jsHeaders, `sandbox/${p.slice(4)}`);
  }
  res.writeHead(404);
  res.end('not found');
}).listen(HOST_PORT, () => console.log(`host    ${HOST_ORIGIN}/host.html`));

// ---- sandbox origin ------------------------------------------------------
createServer((req, res) => {
  const url = new URL(req.url, SANDBOX_ORIGIN);
  const p = url.pathname;

  if (p === '/runtime.html') {
    // Token gate (M4): the host mints a short-lived token; the sandbox serves
    // the runtime only for a valid one (no cookies involved).
    if (url.searchParams.get('token') !== VALID_TOKEN) {
      res.writeHead(403);
      return res.end('bad token');
    }
    return send(res, 200, runtimeHeaders(req), 'sandbox/runtime.html');
  }

  if (p === '/runtime.js' || p === '/bundle.js') {
    return send(res, 200, jsHeaders, `sandbox/${p.slice(1)}`);
  }

  res.writeHead(404);
  res.end('not found');
}).listen(SANDBOX_PORT, () => console.log(`sandbox ${SANDBOX_ORIGIN}/runtime.html`));
