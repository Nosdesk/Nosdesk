import { ref, type Ref } from 'vue';
import ticketService from '@/services/ticketService';
import * as deviceService from '@/services/deviceService';
import type { Device } from '@/types/device';

/**
 * Composable for managing ticket devices
 */
export function useTicketDevices(ticket: Ref<any>, refreshTicket: () => Promise<void>) {
  const showDeviceModal = ref(false);

  // Transform backend device to ticket device format
  function transformDevice(backendDevice: Device): any {
    return {
      id: backendDevice.id,
      name: backendDevice.name,
      hostname: backendDevice.hostname,
      serial_number: backendDevice.serial_number,
      model: backendDevice.model,
      warranty_status: backendDevice.warranty_status,
    };
  }

  // Add device to ticket
  async function addDevice(device: Device): Promise<void> {
    if (!ticket.value) return;

    try {
      await ticketService.addDeviceToTicket(ticket.value.id, device.id);

      // Optimistically update local state - use direct mutation to preserve array reference
      const transformedDevice = transformDevice(device);
      if (ticket.value.devices) {
        ticket.value.devices.push(transformedDevice);
      } else {
        ticket.value.devices = [transformedDevice];
      }

      showDeviceModal.value = false;
    } catch (err) {
      console.error('Error adding device to ticket:', err);
      await refreshTicket();
    }
  }

  // Remove device from ticket
  async function removeDevice(deviceId: number): Promise<void> {
    if (!ticket.value) return;

    try {
      await ticketService.removeDeviceFromTicket(ticket.value.id, deviceId);

      // Optimistically update local state - use splice to preserve array reference
      if (ticket.value.devices) {
        const index = ticket.value.devices.findIndex((d: any) => d.id === deviceId);
        if (index !== -1) {
          ticket.value.devices.splice(index, 1);
        }
      }
    } catch (err) {
      console.error('Error removing device from ticket:', err);
      await refreshTicket();
    }
  }

  // Update device field
  async function updateDeviceField(deviceId: number, field: string, newValue: string): Promise<void> {
    if (!ticket.value?.devices) return;

    const deviceIndex = ticket.value.devices.findIndex((d: any) => d.id === deviceId);
    if (deviceIndex === -1) return;

    const oldValue = ticket.value.devices[deviceIndex][field];
    if (oldValue === newValue) return;

    try {
      // Optimistic update - use direct mutation to preserve array and object references
      ticket.value.devices[deviceIndex][field] = newValue;

      // Send to backend
      await deviceService.updateDevice(deviceId, { [field]: newValue });
    } catch (err) {
      console.error('Error updating device field:', err);

      // Revert on error - use direct mutation
      ticket.value.devices[deviceIndex][field] = oldValue;
    }
  }

  return {
    showDeviceModal,
    addDevice,
    removeDevice,
    updateDeviceField,
  };
}
