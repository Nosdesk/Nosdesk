// GENERATED from @nosdesk/plugin-runtime src/runtime.ts by `pnpm --filter @nosdesk/plugin-runtime build`. Do not edit by hand; CI drift-checks it.

// ../../node_modules/.pnpm/comlink@4.4.2/node_modules/comlink/dist/esm/comlink.mjs
var proxyMarker = Symbol("Comlink.proxy");
var createEndpoint = Symbol("Comlink.endpoint");
var releaseProxy = Symbol("Comlink.releaseProxy");
var finalizer = Symbol("Comlink.finalizer");
var throwMarker = Symbol("Comlink.thrown");
var isObject = (val) => typeof val === "object" && val !== null || typeof val === "function";
var proxyTransferHandler = {
  canHandle: (val) => isObject(val) && val[proxyMarker],
  serialize(obj) {
    const { port1, port2 } = new MessageChannel();
    expose(obj, port1);
    return [port2, [port2]];
  },
  deserialize(port) {
    port.start();
    return wrap(port);
  }
};
var throwTransferHandler = {
  canHandle: (value) => isObject(value) && throwMarker in value,
  serialize({ value }) {
    let serialized;
    if (value instanceof Error) {
      serialized = {
        isError: true,
        value: {
          message: value.message,
          name: value.name,
          stack: value.stack
        }
      };
    } else {
      serialized = { isError: false, value };
    }
    return [serialized, []];
  },
  deserialize(serialized) {
    if (serialized.isError) {
      throw Object.assign(new Error(serialized.value.message), serialized.value);
    }
    throw serialized.value;
  }
};
var transferHandlers = /* @__PURE__ */ new Map([
  ["proxy", proxyTransferHandler],
  ["throw", throwTransferHandler]
]);
function isAllowedOrigin(allowedOrigins, origin) {
  for (const allowedOrigin of allowedOrigins) {
    if (origin === allowedOrigin || allowedOrigin === "*") {
      return true;
    }
    if (allowedOrigin instanceof RegExp && allowedOrigin.test(origin)) {
      return true;
    }
  }
  return false;
}
function expose(obj, ep = globalThis, allowedOrigins = ["*"]) {
  ep.addEventListener("message", function callback(ev) {
    if (!ev || !ev.data) {
      return;
    }
    if (!isAllowedOrigin(allowedOrigins, ev.origin)) {
      console.warn(`Invalid origin '${ev.origin}' for comlink proxy`);
      return;
    }
    const { id, type, path } = Object.assign({ path: [] }, ev.data);
    const argumentList = (ev.data.argumentList || []).map(fromWireValue);
    let returnValue;
    try {
      const parent = path.slice(0, -1).reduce((obj2, prop) => obj2[prop], obj);
      const rawValue = path.reduce((obj2, prop) => obj2[prop], obj);
      switch (type) {
        case "GET":
          {
            returnValue = rawValue;
          }
          break;
        case "SET":
          {
            parent[path.slice(-1)[0]] = fromWireValue(ev.data.value);
            returnValue = true;
          }
          break;
        case "APPLY":
          {
            returnValue = rawValue.apply(parent, argumentList);
          }
          break;
        case "CONSTRUCT":
          {
            const value = new rawValue(...argumentList);
            returnValue = proxy(value);
          }
          break;
        case "ENDPOINT":
          {
            const { port1, port2 } = new MessageChannel();
            expose(obj, port2);
            returnValue = transfer(port1, [port1]);
          }
          break;
        case "RELEASE":
          {
            returnValue = void 0;
          }
          break;
        default:
          return;
      }
    } catch (value) {
      returnValue = { value, [throwMarker]: 0 };
    }
    Promise.resolve(returnValue).catch((value) => {
      return { value, [throwMarker]: 0 };
    }).then((returnValue2) => {
      const [wireValue, transferables] = toWireValue(returnValue2);
      ep.postMessage(Object.assign(Object.assign({}, wireValue), { id }), transferables);
      if (type === "RELEASE") {
        ep.removeEventListener("message", callback);
        closeEndPoint(ep);
        if (finalizer in obj && typeof obj[finalizer] === "function") {
          obj[finalizer]();
        }
      }
    }).catch((error) => {
      const [wireValue, transferables] = toWireValue({
        value: new TypeError("Unserializable return value"),
        [throwMarker]: 0
      });
      ep.postMessage(Object.assign(Object.assign({}, wireValue), { id }), transferables);
    });
  });
  if (ep.start) {
    ep.start();
  }
}
function isMessagePort(endpoint) {
  return endpoint.constructor.name === "MessagePort";
}
function closeEndPoint(endpoint) {
  if (isMessagePort(endpoint))
    endpoint.close();
}
function wrap(ep, target) {
  const pendingListeners = /* @__PURE__ */ new Map();
  ep.addEventListener("message", function handleMessage(ev) {
    const { data } = ev;
    if (!data || !data.id) {
      return;
    }
    const resolver = pendingListeners.get(data.id);
    if (!resolver) {
      return;
    }
    try {
      resolver(data);
    } finally {
      pendingListeners.delete(data.id);
    }
  });
  return createProxy(ep, pendingListeners, [], target);
}
function throwIfProxyReleased(isReleased) {
  if (isReleased) {
    throw new Error("Proxy has been released and is not useable");
  }
}
function releaseEndpoint(ep) {
  return requestResponseMessage(ep, /* @__PURE__ */ new Map(), {
    type: "RELEASE"
  }).then(() => {
    closeEndPoint(ep);
  });
}
var proxyCounter = /* @__PURE__ */ new WeakMap();
var proxyFinalizers = "FinalizationRegistry" in globalThis && new FinalizationRegistry((ep) => {
  const newCount = (proxyCounter.get(ep) || 0) - 1;
  proxyCounter.set(ep, newCount);
  if (newCount === 0) {
    releaseEndpoint(ep);
  }
});
function registerProxy(proxy3, ep) {
  const newCount = (proxyCounter.get(ep) || 0) + 1;
  proxyCounter.set(ep, newCount);
  if (proxyFinalizers) {
    proxyFinalizers.register(proxy3, ep, proxy3);
  }
}
function unregisterProxy(proxy3) {
  if (proxyFinalizers) {
    proxyFinalizers.unregister(proxy3);
  }
}
function createProxy(ep, pendingListeners, path = [], target = function() {
}) {
  let isProxyReleased = false;
  const proxy3 = new Proxy(target, {
    get(_target, prop) {
      throwIfProxyReleased(isProxyReleased);
      if (prop === releaseProxy) {
        return () => {
          unregisterProxy(proxy3);
          releaseEndpoint(ep);
          pendingListeners.clear();
          isProxyReleased = true;
        };
      }
      if (prop === "then") {
        if (path.length === 0) {
          return { then: () => proxy3 };
        }
        const r = requestResponseMessage(ep, pendingListeners, {
          type: "GET",
          path: path.map((p) => p.toString())
        }).then(fromWireValue);
        return r.then.bind(r);
      }
      return createProxy(ep, pendingListeners, [...path, prop]);
    },
    set(_target, prop, rawValue) {
      throwIfProxyReleased(isProxyReleased);
      const [value, transferables] = toWireValue(rawValue);
      return requestResponseMessage(ep, pendingListeners, {
        type: "SET",
        path: [...path, prop].map((p) => p.toString()),
        value
      }, transferables).then(fromWireValue);
    },
    apply(_target, _thisArg, rawArgumentList) {
      throwIfProxyReleased(isProxyReleased);
      const last = path[path.length - 1];
      if (last === createEndpoint) {
        return requestResponseMessage(ep, pendingListeners, {
          type: "ENDPOINT"
        }).then(fromWireValue);
      }
      if (last === "bind") {
        return createProxy(ep, pendingListeners, path.slice(0, -1));
      }
      const [argumentList, transferables] = processArguments(rawArgumentList);
      return requestResponseMessage(ep, pendingListeners, {
        type: "APPLY",
        path: path.map((p) => p.toString()),
        argumentList
      }, transferables).then(fromWireValue);
    },
    construct(_target, rawArgumentList) {
      throwIfProxyReleased(isProxyReleased);
      const [argumentList, transferables] = processArguments(rawArgumentList);
      return requestResponseMessage(ep, pendingListeners, {
        type: "CONSTRUCT",
        path: path.map((p) => p.toString()),
        argumentList
      }, transferables).then(fromWireValue);
    }
  });
  registerProxy(proxy3, ep);
  return proxy3;
}
function myFlat(arr) {
  return Array.prototype.concat.apply([], arr);
}
function processArguments(argumentList) {
  const processed = argumentList.map(toWireValue);
  return [processed.map((v) => v[0]), myFlat(processed.map((v) => v[1]))];
}
var transferCache = /* @__PURE__ */ new WeakMap();
function transfer(obj, transfers) {
  transferCache.set(obj, transfers);
  return obj;
}
function proxy(obj) {
  return Object.assign(obj, { [proxyMarker]: true });
}
function toWireValue(value) {
  for (const [name, handler] of transferHandlers) {
    if (handler.canHandle(value)) {
      const [serializedValue, transferables] = handler.serialize(value);
      return [
        {
          type: "HANDLER",
          name,
          value: serializedValue
        },
        transferables
      ];
    }
  }
  return [
    {
      type: "RAW",
      value
    },
    transferCache.get(value) || []
  ];
}
function fromWireValue(value) {
  switch (value.type) {
    case "HANDLER":
      return transferHandlers.get(value.name).deserialize(value.value);
    case "RAW":
      return value.value;
  }
}
function requestResponseMessage(ep, pendingListeners, msg, transfers) {
  return new Promise((resolve) => {
    const id = generateUUID();
    pendingListeners.set(id, resolve);
    if (ep.start) {
      ep.start();
    }
    ep.postMessage(Object.assign({ id }, msg), transfers);
  });
}
function generateUUID() {
  return new Array(4).fill(0).map(() => Math.floor(Math.random() * Number.MAX_SAFE_INTEGER).toString(16)).join("-");
}

// ../plugin-sdk/src/connect.ts
var CONNECT_TIMEOUT_MS = 1e4;
function wrapEvents(remote) {
  const local = /* @__PURE__ */ new Map();
  const remoteUnsub = /* @__PURE__ */ new Map();
  const dispatch = (event, payload) => {
    const set = local.get(event);
    if (!set) return;
    for (const h of [...set]) {
      try {
        void h(payload);
      } catch {
      }
    }
  };
  const on = async (event, handler) => {
    let set = local.get(event);
    if (!set) {
      set = /* @__PURE__ */ new Set();
      local.set(event, set);
      remoteUnsub.set(
        event,
        remote.on(event, proxy((d) => dispatch(event, d)))
      );
    }
    set.add(handler);
    return () => {
      const s = local.get(event);
      if (!s) return;
      s.delete(handler);
      if (s.size === 0) {
        local.delete(event);
        const unsubP = remoteUnsub.get(event);
        remoteUnsub.delete(event);
        void unsubP?.then((fn) => fn()).catch(() => {
        });
      }
    };
  };
  const reset = () => {
    for (const unsubP of remoteUnsub.values()) {
      void unsubP.then((fn) => fn()).catch(() => {
      });
    }
    remoteUnsub.clear();
    local.clear();
  };
  const api = new Proxy(remote, {
    get(target, prop, receiver) {
      if (prop === "on") return on;
      return Reflect.get(target, prop, receiver);
    }
  });
  return { api, reset };
}
function connectToHost() {
  return new Promise((resolve, reject) => {
    const listeners = /* @__PURE__ */ new Set();
    const themeListeners = /* @__PURE__ */ new Set();
    let context = {
      ticket: null,
      asset: null,
      user: null,
      component: { name: "", slot: "" }
    };
    let theme = { tokens: {}, colorScheme: "light", name: "light" };
    let connected = false;
    const timer = setTimeout(() => {
      if (!connected) {
        window.removeEventListener("message", onMessage);
        reject(new Error("sandbox runtime: host did not connect in time"));
      }
    }, CONNECT_TIMEOUT_MS);
    function onMessage(event) {
      const data = event.data;
      if (!data || typeof data !== "object") return;
      if (!connected && data.type === "nosdesk-plugin-init" && event.ports[0]) {
        connected = true;
        clearTimeout(timer);
        context = data.context;
        theme = data.theme;
        const { api, reset } = wrapEvents(wrap(event.ports[0]));
        resolve({
          api,
          context,
          theme,
          onContextChange(cb) {
            listeners.add(cb);
            return () => {
              listeners.delete(cb);
            };
          },
          onThemeChange(cb) {
            themeListeners.add(cb);
            return () => {
              themeListeners.delete(cb);
            };
          },
          resetEvents: reset
        });
      } else if (data.type === "nosdesk-plugin-context") {
        context = data.context;
        for (const cb of listeners) cb(context);
      } else if (data.type === "nosdesk-plugin-theme") {
        theme = data.theme;
        for (const cb of themeListeners) cb(theme);
      }
    }
    window.addEventListener("message", onMessage);
  });
}
function reportHeight(height) {
  window.parent.postMessage({ type: "nosdesk-plugin-height", height }, "*");
}

// src/pluginUiCss.ts
var PLUGIN_UI_CSS = `
:root { --nd-radius: 8px; color-scheme: light dark; }

html, body { margin: 0; padding: 0; }
body {
  font-family: var(--nd-font-sans, system-ui, -apple-system, sans-serif);
  font-size: 13px;
  line-height: 1.5;
  color: var(--nd-text, #1f2937);
  background: transparent;
  -webkit-font-smoothing: antialiased;
}

/* Scrollbars matched to the app's subtle style. */
* { scrollbar-width: thin; scrollbar-color: var(--nd-border-strong, #cbd5e1) transparent; }
*::-webkit-scrollbar { width: 8px; height: 8px; }
*::-webkit-scrollbar-thumb { background: var(--nd-border-strong, #cbd5e1); border-radius: 4px; }
*::-webkit-scrollbar-thumb:hover { background: var(--nd-text-tertiary, #9ca3af); }
*::-webkit-scrollbar-track { background: transparent; }

a { color: var(--nd-accent, #FF6B1A); }

.nd-btn {
  font: inherit;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 5px 12px;
  border-radius: var(--nd-radius, 8px);
  border: 1px solid var(--nd-border, #e5e7eb);
  background: var(--nd-surface, #ffffff);
  color: var(--nd-text, #1f2937);
  font-weight: 500;
  cursor: pointer;
  transition: background 0.12s ease, border-color 0.12s ease;
}
.nd-btn:hover { background: var(--nd-surface-hover, #f3f4f6); }
.nd-btn:disabled { opacity: 0.5; cursor: not-allowed; }
.nd-btn--primary {
  background: var(--nd-accent, #FF6B1A);
  border-color: var(--nd-accent, #FF6B1A);
  color: var(--nd-on-accent, #000000);
}
.nd-btn--primary:hover {
  background: var(--nd-accent-hover, #EB5808);
  border-color: var(--nd-accent-hover, #EB5808);
}

.nd-input, .nd-textarea {
  font: inherit;
  width: 100%;
  box-sizing: border-box;
  padding: 6px 8px;
  border: 1px solid var(--nd-border, #e5e7eb);
  border-radius: var(--nd-radius, 8px);
  background: var(--nd-surface, #ffffff);
  color: var(--nd-text, #1f2937);
}
.nd-input::placeholder, .nd-textarea::placeholder { color: var(--nd-text-tertiary, #9ca3af); }
.nd-input:focus, .nd-textarea:focus {
  outline: none;
  border-color: var(--nd-accent, #FF6B1A);
}
.nd-textarea { resize: vertical; }

.nd-card {
  border: 1px solid var(--nd-border, #e5e7eb);
  border-radius: var(--nd-radius, 8px);
  background: var(--nd-surface-alt, #f9fafb);
  padding: 12px;
}

.nd-label { font-weight: 600; color: var(--nd-text, #1f2937); }
.nd-muted { color: var(--nd-text-tertiary, #6b7280); }
`;

// src/runtime.ts
function toInstance(result) {
  if (typeof result === "function") return { unmount: result };
  if (result && typeof result === "object") return result;
  return {};
}
var token = new URLSearchParams(location.search).get("t");
var root = document.getElementById("root");
function injectBaseCss() {
  if (document.getElementById("nd-base")) return;
  const style = document.createElement("style");
  style.id = "nd-base";
  style.textContent = PLUGIN_UI_CSS;
  document.head.appendChild(style);
}
function injectTokens(theme) {
  let style = document.getElementById("nd-tokens");
  if (!style) {
    style = document.createElement("style");
    style.id = "nd-tokens";
    document.head.appendChild(style);
  }
  const vars = Object.entries(theme.tokens).map(([k, v]) => `  --nd-${k}: ${v};`).join("\n");
  style.textContent = `:root {
${vars}
}`;
  document.documentElement.setAttribute("data-nd-color-scheme", theme.colorScheme);
  document.documentElement.setAttribute("data-nd-theme", theme.name);
}
function observeHeight(el) {
  let last = -1;
  const report = () => {
    const h = Math.ceil(el.getBoundingClientRect().height);
    if (h > 0 && h !== last) {
      last = h;
      reportHeight(h);
    }
  };
  new ResizeObserver(report).observe(el);
  report();
}
async function boot() {
  if (!root) throw new Error("sandbox runtime: no #root element");
  if (!token) throw new Error("sandbox runtime: missing bundle token");
  const runtime = await connectToHost();
  injectBaseCss();
  injectTokens(runtime.theme);
  runtime.onThemeChange(injectTokens);
  const bundleUrl = `./bundle?t=${encodeURIComponent(token)}`;
  let mod;
  try {
    mod = await import(
      /* @vite-ignore */
      bundleUrl
    );
  } catch (e) {
    window.parent.postMessage({ type: "nosdesk-plugin-bundle-error" }, "*");
    throw e;
  }
  if (!mod.default || typeof mod.default.mount !== "function") {
    throw new Error("sandbox runtime: bundle has no default { mount } export");
  }
  const plugin = mod.default;
  let instance = toInstance(plugin.mount(root, runtime.api, runtime.context));
  observeHeight(root);
  runtime.onContextChange((ctx) => {
    if (instance.update) {
      instance.update(ctx);
      return;
    }
    instance.unmount?.();
    runtime.resetEvents();
    root.replaceChildren();
    instance = toInstance(plugin.mount(root, runtime.api, ctx));
  });
}
boot().catch((e) => {
  if (root) root.textContent = `plugin failed to load: ${String(e)}`;
});
/*! Bundled license information:

comlink/dist/esm/comlink.mjs:
  (**
   * @license
   * Copyright 2019 Google LLC
   * SPDX-License-Identifier: Apache-2.0
   *)
*/
