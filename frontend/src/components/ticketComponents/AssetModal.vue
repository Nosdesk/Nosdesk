<!-- components/ticketComponents/DeviceModal.vue -->
<script setup lang="ts">
import { computed, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import type { Asset } from '@/types/ticket';
import Modal from '@/components/Modal.vue';
import BaseDropdown from '@/components/common/BaseDropdown.vue';

const { $t } = useFluent();

const warrantyOptions = computed(() => [
  { value: 'Active', label: $t('ticket-picker-device-warranty-active') },
  { value: 'Warning', label: $t('ticket-picker-device-warranty-warning') },
  { value: 'Expired', label: $t('ticket-picker-device-warranty-expired') },
  { value: 'Unknown', label: $t('ticket-picker-device-warranty-unknown') },
]);

defineProps<{
  show: boolean;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'add-device', device: Asset): void;
}>();

// Generate a simple unique ID as number
const generateId = () => {
  return Date.now();
};

const createEmptyDevice = (): Asset => ({
  id: generateId(),
  name: '',
  kind: 'device',
  attributes: { hostname: '', warranty_status: 'Unknown' },
  serial_number: '',
  model: '',
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
  is_editable: true,
});

const device = ref<Asset>(createEmptyDevice());

// Local mirror of the IT attributes that BaseDropdown / inputs
// bind against. Re-applied to `device.attributes` on submit so
// the device row leaves with the typed values populated.
const hostnameInput = ref('');
const warrantyInput = ref<string>('Unknown');

const handleSubmit = () => {
  device.value.attributes = {
    ...device.value.attributes,
    hostname: hostnameInput.value,
    warranty_status: warrantyInput.value,
  };
  emit('add-device', { ...device.value });
  device.value = createEmptyDevice();
  hostnameInput.value = '';
  warrantyInput.value = 'Unknown';
  emit('close');
};
</script>

<template>
  <Modal :show="show" :title="$t('ticket-picker-device-title')" @close="emit('close')">
    <form @submit.prevent="handleSubmit" class="flex flex-col gap-4">
      <!-- Name -->
      <div class="flex flex-col gap-1">
        <label for="name" class="text-sm text-tertiary">{{ $t('ticket-picker-device-name-label') }}</label>
        <input
          id="name"
          v-model="device.name"
          type="text"
          required
          class="bg-surface text-secondary rounded-lg p-2 border-none focus:ring-2 focus:ring-accent"
          :placeholder="$t('ticket-picker-device-name-placeholder')"
        />
      </div>

      <!-- Hostname -->
      <div class="flex flex-col gap-1">
        <label for="hostname" class="text-sm text-tertiary">{{ $t('ticket-picker-device-hostname-label') }}</label>
        <input
          id="hostname"
          v-model="hostnameInput"
          type="text"
          required
          class="bg-surface text-secondary rounded-lg p-2 border-none focus:ring-2 focus:ring-accent"
          :placeholder="$t('ticket-picker-device-hostname-placeholder')"
        />
      </div>

      <!-- Serial Number -->
      <div class="flex flex-col gap-1">
        <label for="serial_number" class="text-sm text-tertiary">{{ $t('ticket-picker-device-serial-label') }}</label>
        <input
          id="serial_number"
          v-model="device.serial_number"
          type="text"
          required
          class="bg-surface text-secondary rounded-lg p-2 border-none focus:ring-2 focus:ring-accent"
          :placeholder="$t('ticket-picker-device-serial-placeholder')"
        />
      </div>

      <!-- Model -->
      <div class="flex flex-col gap-1">
        <label for="model" class="text-sm text-tertiary">{{ $t('ticket-picker-device-model-label') }}</label>
        <input
          id="model"
          v-model="device.model"
          type="text"
          required
          class="bg-surface text-secondary rounded-lg p-2 border-none focus:ring-2 focus:ring-accent"
          :placeholder="$t('ticket-picker-device-model-placeholder')"
        />
      </div>

      <!-- Warranty Status -->
      <div class="flex flex-col gap-1">
        <label for="warranty_status" class="text-sm text-tertiary">{{ $t('ticket-picker-device-warranty-label') }}</label>
        <BaseDropdown
          v-model="warrantyInput"
          :options="warrantyOptions"
          size="sm"
        />
      </div>

      <!-- Buttons -->
      <div class="flex justify-end gap-3 mt-4">
        <button
          type="button"
          @click="emit('close')"
          class="px-4 py-2 text-sm text-secondary hover:text-primary"
        >
          {{ $t('ticket-picker-device-cancel') }}
        </button>
        <button
          type="submit"
          class="px-4 py-2 text-sm bg-accent text-white rounded-lg hover:opacity-90"
        >
          {{ $t('ticket-picker-device-add') }}
        </button>
      </div>
    </form>
  </Modal>
</template>
