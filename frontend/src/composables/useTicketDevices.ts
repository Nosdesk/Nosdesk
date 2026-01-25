import { ref, type Ref } from 'vue';
import ticketService from '@/services/ticketService';
import * as deviceService from '@/services/deviceService';
import type { Device } from '@/types/device';
import type { Ticket } from '@/types/ticket';
import { useTicketMutations } from './useTicketMutations';

/**
 * Composable for managing ticket devices
 *
 * Uses centralized mutations from useTicketMutations for consistent
 * array operations with built-in deduplication.
 */
export function useTicketDevices(ticket: Ref<Ticket | null>) {
  const mutations = useTicketMutations(ticket);
  const showDeviceModal = ref(false);

  // Add device to ticket with optimistic update
  async function addDevice(device: Device): Promise<void> {
    if (!ticket.value || mutations.hasDevice(device.id)) return;

    // Optimistic update
    mutations.addDevice(device);
    showDeviceModal.value = false;

    try {
      await ticketService.addDeviceToTicket(ticket.value.id, device.id);
    } catch (err) {
      console.error('Error adding device to ticket:', err);
      mutations.removeDevice(device.id);
    }
  }

  // Remove device from ticket with optimistic update
  async function removeDevice(deviceId: number): Promise<void> {
    if (!ticket.value) return;

    // Store device for potential rollback
    const device = ticket.value.devices?.find(d => d.id === deviceId);
    if (!device) return;

    // Optimistic update
    mutations.removeDevice(deviceId);

    try {
      await ticketService.removeDeviceFromTicket(ticket.value.id, deviceId);
    } catch (err) {
      console.error('Error removing device from ticket:', err);
      mutations.addDevice(device);
    }
  }

  // Update device field with optimistic update
  async function updateDeviceField(deviceId: number, field: string, newValue: string): Promise<void> {
    if (!ticket.value?.devices) return;

    const device = ticket.value.devices.find(d => d.id === deviceId);
    if (!device) return;

    const oldValue = (device as Record<string, unknown>)[field];
    if (oldValue === newValue) return;

    // Optimistic update
    mutations.updateDeviceField(deviceId, field, newValue);

    try {
      await deviceService.updateDevice(deviceId, { [field]: newValue });
    } catch (err) {
      console.error('Error updating device field:', err);
      mutations.updateDeviceField(deviceId, field, oldValue);
    }
  }

  return {
    showDeviceModal,
    addDevice,
    removeDevice,
    updateDeviceField,
  };
}
