import type { Ref } from 'vue';
import type { Ticket, Device } from '@/types/ticket';

/**
 * Centralized ticket mutation helpers with built-in deduplication.
 * Used by both user actions (optimistic updates) and SSE handlers.
 *
 * All mutations use direct array mutation (splice/push) to preserve
 * Vue reactivity without replacing object references.
 */
export function useTicketMutations(ticket: Ref<Ticket | null>) {

  // ─────────────────────────────────────────────────────────────
  // Linked Tickets
  // ─────────────────────────────────────────────────────────────

  function addLinkedTicket(ticketId: number): boolean {
    if (!ticket.value) return false;

    if (!ticket.value.linkedTickets) {
      ticket.value.linkedTickets = [];
    }

    if (ticket.value.linkedTickets.includes(ticketId)) {
      return false; // Already exists
    }

    ticket.value.linkedTickets.push(ticketId);
    return true;
  }

  function removeLinkedTicket(ticketId: number): boolean {
    if (!ticket.value?.linkedTickets) return false;

    const index = ticket.value.linkedTickets.indexOf(ticketId);
    if (index === -1) return false; // Not found

    ticket.value.linkedTickets.splice(index, 1);
    return true;
  }

  function hasLinkedTicket(ticketId: number): boolean {
    return ticket.value?.linkedTickets?.includes(ticketId) ?? false;
  }

  // ─────────────────────────────────────────────────────────────
  // Projects
  // ─────────────────────────────────────────────────────────────

  function addProject(projectId: string | number): boolean {
    if (!ticket.value) return false;

    const id = String(projectId);

    if (!ticket.value.projects) {
      ticket.value.projects = [];
    }

    if (ticket.value.projects.includes(id)) {
      return false; // Already exists
    }

    ticket.value.projects.push(id);
    return true;
  }

  function removeProject(projectId: string | number): boolean {
    if (!ticket.value?.projects) return false;

    const id = String(projectId);
    const index = ticket.value.projects.indexOf(id);
    if (index === -1) return false; // Not found

    ticket.value.projects.splice(index, 1);
    return true;
  }

  function hasProject(projectId: string | number): boolean {
    return ticket.value?.projects?.includes(String(projectId)) ?? false;
  }

  // ─────────────────────────────────────────────────────────────
  // Devices
  // ─────────────────────────────────────────────────────────────

  function addDevice(device: Device): boolean {
    if (!ticket.value) return false;

    if (!ticket.value.devices) {
      ticket.value.devices = [];
    }

    if (ticket.value.devices.some(d => d.id === device.id)) {
      return false; // Already exists
    }

    ticket.value.devices.push(device);
    return true;
  }

  function removeDevice(deviceId: number): boolean {
    if (!ticket.value?.devices) return false;

    const index = ticket.value.devices.findIndex(d => d.id === deviceId);
    if (index === -1) return false; // Not found

    ticket.value.devices.splice(index, 1);
    return true;
  }

  function updateDeviceField(deviceId: number, field: string, value: unknown): boolean {
    if (!ticket.value?.devices) return false;

    const device = ticket.value.devices.find(d => d.id === deviceId);
    if (!device) return false;

    (device as Record<string, unknown>)[field] = value;
    return true;
  }

  function hasDevice(deviceId: number): boolean {
    return ticket.value?.devices?.some(d => d.id === deviceId) ?? false;
  }

  return {
    // Linked tickets
    addLinkedTicket,
    removeLinkedTicket,
    hasLinkedTicket,
    // Projects
    addProject,
    removeProject,
    hasProject,
    // Devices
    addDevice,
    removeDevice,
    updateDeviceField,
    hasDevice,
  };
}
