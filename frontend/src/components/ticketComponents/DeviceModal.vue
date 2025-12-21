<!-- components/ticketComponents/DeviceModal.vue -->
<script setup lang="ts">
import { ref } from 'vue';
import type { Device } from '@/types/ticket';
import Modal from '@/components/Modal.vue';

const props = defineProps<{
  show: boolean;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'add-device', device: Device): void;
}>();

// Generate a simple unique ID as number
const generateId = () => {
  return Date.now();
};

const createEmptyDevice = (): Device => ({
  id: generateId(),
  name: '',
  hostname: '',
  serial_number: '',
  model: '',
  warranty_status: '',
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
  is_editable: true
});

const device = ref<Device>(createEmptyDevice());

const handleSubmit = () => {
  emit('add-device', { ...device.value });
  device.value = createEmptyDevice();
  emit('close');
};
</script>

<template>
  <Modal :show="show" title="Add Device" @close="emit('close')">
    <form @submit.prevent="handleSubmit" class="flex flex-col gap-4">
      <!-- Name -->
      <div class="flex flex-col gap-1">
        <label for="name" class="text-sm text-tertiary">Name</label>
        <input
          id="name"
          v-model="device.name"
          type="text"
          required
          class="bg-surface text-secondary rounded-lg p-2 border-none focus:ring-2 focus:ring-accent"
          placeholder="Enter device name"
        />
      </div>

      <!-- Hostname -->
      <div class="flex flex-col gap-1">
        <label for="hostname" class="text-sm text-tertiary">Hostname</label>
        <input
          id="hostname"
          v-model="device.hostname"
          type="text"
          required
          class="bg-surface text-secondary rounded-lg p-2 border-none focus:ring-2 focus:ring-accent"
          placeholder="Enter hostname"
        />
      </div>

      <!-- Serial Number -->
      <div class="flex flex-col gap-1">
        <label for="serial_number" class="text-sm text-tertiary">Serial Number</label>
        <input
          id="serial_number"
          v-model="device.serial_number"
          type="text"
          required
          class="bg-surface text-secondary rounded-lg p-2 border-none focus:ring-2 focus:ring-accent"
          placeholder="Enter serial number"
        />
      </div>

      <!-- Model -->
      <div class="flex flex-col gap-1">
        <label for="model" class="text-sm text-tertiary">Model</label>
        <input
          id="model"
          v-model="device.model"
          type="text"
          required
          class="bg-surface text-secondary rounded-lg p-2 border-none focus:ring-2 focus:ring-accent"
          placeholder="Enter model"
        />
      </div>

      <!-- Warranty Status -->
      <div class="flex flex-col gap-1">
        <label for="warranty_status" class="text-sm text-tertiary">Warranty Status</label>
        <input
          id="warranty_status"
          v-model="device.warranty_status"
          type="text"
          required
          class="bg-surface text-secondary rounded-lg p-2 border-none focus:ring-2 focus:ring-accent"
          placeholder="Enter warranty status"
        />
      </div>

      <!-- Buttons -->
      <div class="flex justify-end gap-3 mt-4">
        <button
          type="button"
          @click="emit('close')"
          class="px-4 py-2 text-sm text-secondary hover:text-primary"
        >
          Cancel
        </button>
        <button
          type="submit"
          class="px-4 py-2 text-sm bg-accent text-white rounded-lg hover:opacity-90"
        >
          Add Device
        </button>
      </div>
    </form>
  </Modal>
</template> 