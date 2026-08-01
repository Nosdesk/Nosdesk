import type { Ticket } from '@nosdesk/core/types/ticket';
import type { Asset } from '@nosdesk/core/types/asset';
import type { User } from '@nosdesk/core/types/user';
import type { UserAddress } from '@nosdesk/core/services/userContactService';

/**
 * The host-provided context bag handed to a plugin slot.
 *
 * Every field is optional; a mount fills the one matching its slot's declared
 * context type (see `SlotDef.context`). This replaces the old per-entity prop
 * drilling (`ticket` + `device` threaded through PluginSlot -> PluginSlotItem
 * -> PluginSandboxFrame): adding a new context type is now a field here plus one
 * line in the sandbox snapshot, not an edit across every layer.
 */
export interface PluginSlotContext {
  ticket?: Ticket;
  asset?: Asset;
  /** The profile being viewed (`user.sidebar.panel`). */
  user?: User;
  /** A single address row to enrich (`user.address.panel`). Projected to the
   * wire `PluginAddress` in the sandbox snapshot. */
  address?: UserAddress;
  /** Selected ticket ids for a bulk action (`ticket.bulk.action`). */
  ticketIds?: number[];
  // Future context types (e.g. `documentationPage`) land here alongside a
  // matching line in PluginSandboxFrame's snapshot() and the SDK wire type.
}
