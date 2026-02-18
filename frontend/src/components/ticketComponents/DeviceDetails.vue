<!-- components/DeviceDetails.vue -->
<script setup lang="ts">
import { computed, ref } from 'vue';
import type { Device } from '@/types/ticket';
import SidebarCard from "@/components/ticketComponents/SidebarCard.vue";

const props = defineProps<{
  device: Device;
}>();

const emit = defineEmits<{
  (e: 'remove'): void;
  (e: 'view', deviceId: number): void;
}>();

const copiedField = ref<string | null>(null);
let copiedTimeout: ReturnType<typeof setTimeout> | null = null;

const copyValue = async (field: string, value: string | undefined | null) => {
  if (!value) return;
  try {
    await navigator.clipboard.writeText(value);
    copiedField.value = field;
    if (copiedTimeout) clearTimeout(copiedTimeout);
    copiedTimeout = setTimeout(() => { copiedField.value = null; }, 800);
  } catch {
    // Clipboard not available
  }
};

const warrantyStatusClass = computed(() => {
  switch (props.device.warranty_status) {
    case 'Active':
      return 'bg-status-success/20 text-status-success border-status-success/40';
    case 'Warning':
      return 'bg-status-warning/20 text-status-warning border-status-warning/40';
    case 'Expired':
      return 'bg-status-error/20 text-status-error border-status-error/40';
    default:
      return 'bg-surface-alt text-secondary border-default';
  }
});
</script>

<template>
  <SidebarCard remove-title="Remove device" @remove="emit('remove')">
    <template #header>
      <div class="w-2 h-2 bg-accent rounded-full flex-shrink-0"></div>

      <h3
        @click="emit('view', device.id)"
        class="text-md font-medium text-primary truncate cursor-pointer hover:text-accent transition-colors"
        :title="device.name || 'View device'"
      >
        {{ device.name || 'Unnamed Device' }}
      </h3>

      <!-- Warranty status badge -->
      <div
        v-if="device.warranty_status"
        class="px-2 py-1 rounded-md text-xs font-medium border flex-shrink-0"
        :class="warrantyStatusClass"
      >
        {{ device.warranty_status }}
      </div>
    </template>

    <!-- Device info grid -->
    <div class="flex flex-col gap-3">
      <div class="grid grid-cols-2 gap-3 text-sm">
        <!-- Serial Number -->
        <div class="flex flex-col gap-1">
          <span class="text-xs text-tertiary uppercase tracking-wide">Serial</span>
          <span
            @click="copyValue('serial_number', device.serial_number)"
            class="text-secondary font-mono text-sm cursor-pointer hover:text-accent transition-colors"
            :title="device.serial_number ? 'Click to copy' : ''"
          >
            <span v-if="copiedField === 'serial_number'" class="text-status-success">Copied!</span>
            <template v-else>{{ device.serial_number || 'N/A' }}</template>
          </span>
        </div>

        <!-- Model -->
        <div class="flex flex-col gap-1">
          <span class="text-xs text-tertiary uppercase tracking-wide">Model</span>
          <span
            @click="copyValue('model', device.model)"
            class="text-secondary text-sm truncate cursor-pointer hover:text-accent transition-colors"
            :title="device.model ? 'Click to copy' : ''"
          >
            <span v-if="copiedField === 'model'" class="text-status-success">Copied!</span>
            <template v-else>{{ device.model || 'Unknown' }}</template>
          </span>
        </div>
      </div>

      <div class="grid grid-cols-2 gap-3 text-sm">
        <!-- Manufacturer -->
        <div class="flex flex-col gap-1">
          <span class="text-xs text-tertiary uppercase tracking-wide">Manufacturer</span>
          <span
            @click="copyValue('manufacturer', device.manufacturer)"
            class="text-secondary text-sm truncate cursor-pointer hover:text-accent transition-colors"
            :title="device.manufacturer ? 'Click to copy' : ''"
          >
            <span v-if="copiedField === 'manufacturer'" class="text-status-success">Copied!</span>
            <template v-else>{{ device.manufacturer || 'Unknown' }}</template>
          </span>
        </div>

        <!-- Hostname -->
        <div class="flex flex-col gap-1">
          <span class="text-xs text-tertiary uppercase tracking-wide">Hostname</span>
          <span
            @click="copyValue('hostname', device.hostname)"
            class="text-secondary font-mono text-sm truncate cursor-pointer hover:text-accent transition-colors"
            :title="device.hostname ? 'Click to copy' : ''"
          >
            <span v-if="copiedField === 'hostname'" class="text-status-success">Copied!</span>
            <template v-else>{{ device.hostname || 'N/A' }}</template>
          </span>
        </div>
      </div>
    </div>

    <template #print>
      <div class="hidden print:block print-device-card">
        <div class="print-device-header">
          <span class="print-device-name">{{ device.name || 'Unnamed Device' }}</span>
          <span v-if="device.warranty_status" class="print-device-warranty" :class="`print-warranty-${device.warranty_status.toLowerCase()}`">
            {{ device.warranty_status }}
          </span>
        </div>
        <div class="print-device-details">
          <span v-if="device.serial_number" class="print-device-field">
            <span class="print-field-label">S/N:</span> {{ device.serial_number }}
          </span>
          <span v-if="device.model" class="print-device-field">
            <span class="print-field-label">Model:</span> {{ device.model }}
          </span>
          <span v-if="device.manufacturer" class="print-device-field">
            <span class="print-field-label">Mfr:</span> {{ device.manufacturer }}
          </span>
          <span v-if="device.hostname" class="print-device-field">
            <span class="print-field-label">Host:</span> {{ device.hostname }}
          </span>
        </div>
      </div>
    </template>
  </SidebarCard>
</template>

<style scoped>
@media print {
  .print-device-card {
    border: 1px solid #ccc;
    padding: 6pt 8pt;
    margin-bottom: 4pt;
    background: #fafafa;
    font-size: 9pt;
  }

  .print-device-header {
    display: flex;
    align-items: center;
    gap: 8pt;
    margin-bottom: 4pt;
  }

  .print-device-name { font-weight: 600; color: #000; }

  .print-device-warranty {
    font-size: 8pt;
    padding: 1pt 4pt;
    border: 1px solid currentColor;
    border-radius: 2pt;
  }

  .print-warranty-active { color: #047857; }
  .print-warranty-warning { color: #b45309; }
  .print-warranty-expired { color: #dc2626; }

  .print-device-details {
    display: flex;
    flex-wrap: wrap;
    gap: 8pt;
    color: #333;
  }

  .print-device-field { white-space: nowrap; }
  .print-field-label { color: #666; }
}
</style>
