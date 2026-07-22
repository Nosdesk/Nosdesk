// @nosdesk/plugin-sdk — the typed contract + iframe-side bridge for sandboxed
// Nosdesk plugins. A plugin's bundle default-exports a { mount } and, inside it,
// calls connectToHost() to obtain the host API + context.
//
//   import { connectToHost } from '@nosdesk/plugin-sdk';
//   import type { PluginModule } from '@nosdesk/plugin-sdk';
//
//   export default {
//     async mount(root, _api, context) {
//       const { api } = await connectToHost();
//       const t = context.ticket ?? (await api.tickets.list())[0];
//       root.textContent = t ? t.title : 'no ticket';
//     },
//   } satisfies PluginModule;
export { connectToHost, proxy } from './connect';
export type { PluginRuntime } from './connect';

export type {
  HostApi,
  PluginContext,
  PluginModule,
  PluginComment,
  PluginAttachment,
  PluginCollection,
  PluginFetchOptions,
  PluginFetchResponse,
  PluginEventHandler,
  // Domain types, re-exported so authors import only from this package.
  Ticket,
  Asset,
  CollectionRow,
  CollectionListResponse,
  PluginEvent,
} from './types';
