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
export { connectToHost, proxy, reportHeight } from './connect';
export type { PluginRuntime } from './connect';

// Host-side bridge (used by the app + the bridge harness, not by plugins).
export { createRemoteHostApi, postInit, postContext, postTheme, watchPluginHeight } from './host';
export type { HostBridge } from './host';
export { BridgeGovernor, governHostApi, DEFAULT_GOVERNOR_OPTIONS } from './governor';
export type { GovernorOptions } from './governor';
export { PluginApiError, asPluginApiError } from './errors';
export type { PluginApiErrorCode } from './errors';

export type {
  HostApi,
  PluginHostApi,
  PluginContext,
  PluginAddress,
  PluginTheme,
  PluginModule,
  PluginInstance,
  PluginComment,
  PluginAttachment,
  PluginTicketPatch,
  PluginAssetPatch,
  PluginUser,
  PluginUserQuery,
  PluginUserList,
  PluginWorkflowState,
  PluginPriority,
  PluginCollection,
  PluginFetchOptions,
  PluginFetchResponse,
  PluginEventHandler,
  PluginEventPayload,
  // Domain types, re-exported so authors import only from this package.
  Ticket,
  Asset,
  CollectionRow,
  CollectionListResponse,
  PluginEvent,
} from './types';
