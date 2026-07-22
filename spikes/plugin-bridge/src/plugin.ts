// A trivial sandboxed plugin. The runtime calls `mount` with a Comlink-proxied
// HostApi; every call is a round-trip over the transferred port. Proves both
// directions: reads a value the host returns (tickets.get), then sends a value
// back to the host (notify) that the host records on its own origin.
import type { PluginModule } from '@nosdesk/plugin-sdk';

export default {
  async mount(root, api) {
    const ticket = await api.tickets.get(42);
    const title = ticket ? ticket.title : 'null';
    root.textContent = `plugin got: ${title}`;
    await api.notify(title);
  },
} satisfies PluginModule;
