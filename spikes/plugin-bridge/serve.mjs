// Two-origin server for the bridge harness. No deps.
//   host    origin: http://localhost:5320   (the "app")
//   sandbox origin: http://127.0.0.1:5321    (serves runtime + token-gated bundle)
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const DIR = dirname(fileURLToPath(import.meta.url));
const HOST_PORT = 5320;
const SANDBOX_PORT = 5321;
const VALID_TOKEN = 'bridge-token';

async function send(res, status, headers, path) {
  try {
    const body = path ? await readFile(join(DIR, path)) : '';
    res.writeHead(status, headers);
    res.end(body);
  } catch (e) {
    res.writeHead(500);
    res.end(String(e));
  }
}

const jsHeaders = {
  'Content-Type': 'text/javascript',
  'Access-Control-Allow-Origin': '*',
  'Cross-Origin-Resource-Policy': 'cross-origin',
};

function runtimeHeaders(req) {
  const sbHost = `http://${req.headers.host}`;
  return {
    'Content-Type': 'text/html',
    'Content-Security-Policy':
      `default-src 'none'; script-src ${sbHost}; style-src 'unsafe-inline'; connect-src 'none';`,
    'Cross-Origin-Resource-Policy': 'cross-origin',
  };
}

// ---- host ("app") origin -------------------------------------------------
createServer((req, res) => {
  const { pathname } = new URL(req.url, `http://localhost:${HOST_PORT}`);
  if (pathname === '/' || pathname === '/host.html') {
    return send(res, 200, { 'Content-Type': 'text/html' }, 'public/host.html');
  }
  if (pathname === '/host.js') {
    return send(res, 200, { 'Content-Type': 'text/javascript' }, 'dist/host.js');
  }
  res.writeHead(404);
  res.end('not found');
}).listen(HOST_PORT, () => console.log(`host    http://localhost:${HOST_PORT}/host.html`));

// ---- sandbox origin ------------------------------------------------------
createServer((req, res) => {
  const url = new URL(req.url, `http://127.0.0.1:${SANDBOX_PORT}`);
  const p = url.pathname;
  if (p === '/runtime.html') {
    if (url.searchParams.get('t') !== VALID_TOKEN) {
      res.writeHead(403);
      return res.end('bad token');
    }
    return send(res, 200, runtimeHeaders(req), 'public/runtime.html');
  }
  if (p === '/runtime.js') {
    return send(res, 200, jsHeaders, 'dist/runtime.js');
  }
  // The runtime imports `./bundle?t=<token>`; the real backend verifies the
  // token here (covered by backend tests) — the harness just serves the plugin.
  if (p === '/bundle') {
    return send(res, 200, jsHeaders, 'dist/plugin.js');
  }
  res.writeHead(404);
  res.end('not found');
}).listen(SANDBOX_PORT, () => console.log(`sandbox http://127.0.0.1:${SANDBOX_PORT}/runtime.html`));
