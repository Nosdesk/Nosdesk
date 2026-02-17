<!-- components/DeviceDetails.vue -->
<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { Device } from '@/types/ticket';
import SidebarCard from "@/components/ticketComponents/SidebarCard.vue";

const props = defineProps<{
  device: Device;
}>();

const emit = defineEmits<{
  (e: 'remove'): void;
  (e: 'view', deviceId: number): void;
  (e: 'update:name', value: string): void;
  (e: 'update:hostname', value: string): void;
  (e: 'update:serial_number', value: string): void;
  (e: 'update:model', value: string): void;
  (e: 'update:manufacturer', value: string): void;
  (e: 'update:warranty_status', value: string): void;
}>();

// Editable field definitions for DRY field handling
type EditableField = 'name' | 'hostname' | 'serial_number' | 'model' | 'manufacturer' | 'warranty_status';

const editableFields = ref<Record<EditableField, string>>({
  name: props.device.name || '',
  hostname: props.device.hostname || '',
  serial_number: props.device.serial_number || '',
  model: props.device.model || '',
  manufacturer: props.device.manufacturer || '',
  warranty_status: props.device.warranty_status || '',
});

const editingField = ref<EditableField | null>(null);
const isUpdatingFromProps = ref(false);

// Watch each device prop and sync to local state
const fieldKeys: EditableField[] = ['name', 'hostname', 'serial_number', 'model', 'manufacturer', 'warranty_status'];

fieldKeys.forEach((field) => {
  watch(() => props.device[field], (newVal) => {
    const val = (newVal as string) || '';
    if (val !== editableFields.value[field]) {
      isUpdatingFromProps.value = true;
      editableFields.value[field] = val;
      isUpdatingFromProps.value = false;
    }
  });

  watch(() => editableFields.value[field], (newVal, oldVal) => {
    if (!isUpdatingFromProps.value && newVal !== oldVal) {
      emit(`update:${field}` as any, newVal);
    }
  });
});

const startEditing = (field: EditableField) => { editingField.value = field; };
const stopEditing = () => { editingField.value = null; };

const handleKeydown = (event: KeyboardEvent, field: EditableField) => {
  if (event.key === 'Enter') {
    stopEditing();
  } else if (event.key === 'Escape') {
    editableFields.value[field] = (props.device[field] as string) || '';
    stopEditing();
  }
};

const warrantyStatusClass = computed(() => {
  switch (editableFields.value.warranty_status) {
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

const warrantyStatusOptions = ['Active', 'Warning', 'Expired', 'Unknown'];
</script>

<template>
  <SidebarCard remove-title="Remove device" @remove="emit('remove')">
    <template #header>
      <div class="w-2 h-2 bg-accent rounded-full flex-shrink-0"></div>

      <!-- Editable device name -->
      <div v-if="editingField === 'name'" class="flex-1">
        <input
          v-model="editableFields.name"
          @blur="stopEditing()"
          @keydown="handleKeydown($event, 'name')"
          class="w-full bg-surface text-primary rounded px-2 py-1 text-sm font-medium focus:outline-none focus:ring-2 focus:ring-accent/50"
          placeholder="Enter device name..."
        />
      </div>
      <h3
        v-else
        @click="emit('view', device.id)"
        class="text-md font-medium text-primary truncate cursor-pointer hover:text-accent transition-colors"
        :title="editableFields.name || 'View device'"
      >
        {{ editableFields.name || 'Unnamed Device' }}
      </h3>

      <!-- Warranty status badge -->
      <div v-if="editingField === 'warranty_status'" class="flex-shrink-0">
        <select
          v-model="editableFields.warranty_status"
          @blur="stopEditing()"
          @keydown="handleKeydown($event, 'warranty_status')"
          class="px-2 py-1 rounded-md text-xs font-medium border bg-surface text-primary focus:outline-none focus:ring-2 focus:ring-accent/50"
        >
          <option v-for="status in warrantyStatusOptions" :key="status" :value="status">
            {{ status }}
          </option>
        </select>
      </div>
      <div
        v-else-if="editableFields.warranty_status"
        @click="startEditing('warranty_status')"
        class="px-2 py-1 rounded-md text-xs font-medium border flex-shrink-0 cursor-pointer hover:opacity-80 transition-opacity"
        :class="warrantyStatusClass"
        :title="'Click to edit warranty status: ' + editableFields.warranty_status"
      >
        {{ editableFields.warranty_status }}
      </div>
    </template>

    <!-- Device info grid -->
    <div class="flex flex-col gap-3">
      <div class="grid grid-cols-2 gap-3 text-sm">
        <!-- Serial Number -->
        <div class="flex flex-col gap-1">
          <span class="text-xs text-tertiary uppercase tracking-wide">Serial</span>
          <div v-if="editingField === 'serial_number'">
            <input
              v-model="editableFields.serial_number"
              @blur="stopEditing()"
              @keydown="handleKeydown($event, 'serial_number')"
              class="w-full bg-surface text-secondary rounded px-2 py-1 text-xs font-mono focus:outline-none focus:ring-2 focus:ring-accent/50"
              placeholder="Enter serial number..."
            />
          </div>
          <span
            v-else
            @click="startEditing('serial_number')"
            class="text-secondary font-mono text-sm cursor-pointer hover:text-accent transition-colors"
            :title="'Click to edit: ' + (editableFields.serial_number || 'N/A')"
          >
            {{ editableFields.serial_number || 'N/A' }}
          </span>
        </div>

        <!-- Model -->
        <div class="flex flex-col gap-1">
          <span class="text-xs text-tertiary uppercase tracking-wide">Model</span>
          <div v-if="editingField === 'model'">
            <input
              v-model="editableFields.model"
              @blur="stopEditing()"
              @keydown="handleKeydown($event, 'model')"
              class="w-full bg-surface text-secondary rounded px-2 py-1 text-xs focus:outline-none focus:ring-2 focus:ring-accent/50"
              placeholder="Enter model..."
            />
          </div>
          <span
            v-else
            @click="startEditing('model')"
            class="text-secondary text-sm truncate cursor-pointer hover:text-accent transition-colors"
            :title="'Click to edit: ' + (editableFields.model || 'Unknown')"
          >
            {{ editableFields.model || 'Unknown' }}
          </span>
        </div>
      </div>

      <div class="grid grid-cols-2 gap-3 text-sm">
        <!-- Manufacturer -->
        <div class="flex flex-col gap-1">
          <span class="text-xs text-tertiary uppercase tracking-wide">Manufacturer</span>
          <div v-if="editingField === 'manufacturer'">
            <input
              v-model="editableFields.manufacturer"
              @blur="stopEditing()"
              @keydown="handleKeydown($event, 'manufacturer')"
              class="w-full bg-surface text-secondary rounded px-2 py-1 text-xs focus:outline-none focus:ring-2 focus:ring-accent/50"
              placeholder="Enter manufacturer..."
            />
          </div>
          <span
            v-else
            @click="startEditing('manufacturer')"
            class="text-secondary text-sm truncate cursor-pointer hover:text-accent transition-colors"
            :title="'Click to edit: ' + (editableFields.manufacturer || 'Unknown')"
          >
            {{ editableFields.manufacturer || 'Unknown' }}
          </span>
        </div>

        <!-- Hostname -->
        <div class="flex flex-col gap-1">
          <span class="text-xs text-tertiary uppercase tracking-wide">Hostname</span>
          <div v-if="editingField === 'hostname'">
            <input
              v-model="editableFields.hostname"
              @blur="stopEditing()"
              @keydown="handleKeydown($event, 'hostname')"
              class="w-full bg-surface text-secondary rounded px-2 py-1 text-xs font-mono focus:outline-none focus:ring-2 focus:ring-accent/50"
              placeholder="Enter hostname..."
            />
          </div>
          <span
            v-else
            @click="startEditing('hostname')"
            class="text-secondary font-mono text-sm truncate cursor-pointer hover:text-accent transition-colors"
            :title="'Click to edit: ' + (editableFields.hostname || 'N/A')"
          >
            {{ editableFields.hostname || 'N/A' }}
          </span>
        </div>
      </div>
    </div>

    <template #print>
      <div class="hidden print:block print-device-card">
        <div class="print-device-header">
          <span class="print-device-name">{{ editableFields.name || 'Unnamed Device' }}</span>
          <span v-if="editableFields.warranty_status" class="print-device-warranty" :class="`print-warranty-${editableFields.warranty_status.toLowerCase()}`">
            {{ editableFields.warranty_status }}
          </span>
        </div>
        <div class="print-device-details">
          <span v-if="editableFields.serial_number" class="print-device-field">
            <span class="print-field-label">S/N:</span> {{ editableFields.serial_number }}
          </span>
          <span v-if="editableFields.model" class="print-device-field">
            <span class="print-field-label">Model:</span> {{ editableFields.model }}
          </span>
          <span v-if="editableFields.manufacturer" class="print-device-field">
            <span class="print-field-label">Mfr:</span> {{ editableFields.manufacturer }}
          </span>
          <span v-if="editableFields.hostname" class="print-device-field">
            <span class="print-field-label">Host:</span> {{ editableFields.hostname }}
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
