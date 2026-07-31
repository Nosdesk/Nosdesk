// The host ("app") side. Embeds the opaque-sandbox iframe, and once it loads,
// exposes a stub HostApi over a MessageChannel and transfers the port in with
// the init message (exactly what the app's real PluginSandboxFrame will do).
// The stub records what the plugin sends back (notify) into the host DOM, so the
// round-trip is fully observable on this origin.
import { createRemoteHostApi, postInit } from '@nosdesk/plugin-sdk';
import type { HostApi, PluginContext, Ticket } from '@nosdesk/plugin-sdk';

const sandboxOrigin = document.body.dataset.sandboxOrigin as string;
const TOKEN = 'bridge-token';

function set(id: string, text: string): void {
  const el = document.getElementById(id);
  if (el) el.textContent = text;
}

// The stub only fills what the test plugin exercises; the rest satisfy the type.
const impl: HostApi = {
  version: '1.0.0',
  plugin: { uuid: 'test-uuid', name: 'test', displayName: 'Test', version: '1.0.0' },
  tickets: {
    async get(id) {
      set('host-log', `host: tickets.get(${id})`);
      return { id, title: `ticket ${id} from host` } as unknown as Ticket;
    },
    async list() {
      return [];
    },
    async addComment() {
      return true;
    },
  },
  devices: { async get() { return null; }, async list() { return []; } },
  attachments: { async list() { return []; }, async getBase64() { return null; } },
  async fetch() {
    return null;
  },
  storage: { async get() { return null; }, async set() { return true; }, async delete() { return true; } },
  async collections() {
    throw new Error('collections not stubbed');
  },
  async on() {
    return async () => {};
  },
  async notify(message) {
    set('result', message);
  },
  ui: { async isPrinting() { return false; } },
};

const context: PluginContext = { ticket: null, device: null };

const iframe = document.createElement('iframe');
iframe.setAttribute('sandbox', 'allow-scripts');
iframe.src = `${sandboxOrigin}/runtime.html?t=${TOKEN}`;
iframe.addEventListener('load', () => {
  const bridge = createRemoteHostApi(impl);
  postInit(iframe.contentWindow as Window, bridge, context, {
    tokens: {},
    colorScheme: 'light',
    name: 'light',
  });
});
document.getElementById('frame-slot')?.appendChild(iframe);
