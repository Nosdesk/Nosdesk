<!-- components/DeviceDetails.vue -->
<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { Device } from '@/types/ticket';

// Updated props to accept the full device object
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

// Local reactive state for editable fields
const editableName = ref(props.device.name || '');
const editableHostname = ref(props.device.hostname || '');
const editableSerialNumber = ref(props.device.serial_number || '');
const editableModel = ref(props.device.model || '');
const editableManufacturer = ref(props.device.manufacturer || '');
const editableWarrantyStatus = ref(props.device.warranty_status || '');

// Edit mode states
const editingName = ref(false);
const editingHostname = ref(false);
const editingSerialNumber = ref(false);
const editingModel = ref(false);
const editingManufacturer = ref(false);
const editingWarrantyStatus = ref(false);

// Track if updating from props to prevent circular emissions
const isUpdatingFromProps = ref(false);

// Debug logging for incoming device data
console.log('🔧 DeviceDetails received device data:', {
  id: props.device.id,
  name: props.device.name,
  hostname: props.device.hostname,
  serial_number: props.device.serial_number,
  model: props.device.model,
  manufacturer: props.device.manufacturer,
  warranty_status: props.device.warranty_status
});

// Watch for changes in device props to update local state
watch(() => props.device.name, (newName) => {
  if (newName !== editableName.value) {
    console.log('🔧 DeviceDetails: Updating name from', editableName.value, 'to', newName);
    isUpdatingFromProps.value = true;
    editableName.value = newName || '';
    isUpdatingFromProps.value = false;
  }
});

watch(() => props.device.hostname, (newHostname) => {
  if (newHostname !== editableHostname.value) {
    console.log('🔧 DeviceDetails: Updating hostname from', editableHostname.value, 'to', newHostname);
    isUpdatingFromProps.value = true;
    editableHostname.value = newHostname || '';
    isUpdatingFromProps.value = false;
  }
});

watch(() => props.device.serial_number, (newSerialNumber) => {
  if (newSerialNumber !== editableSerialNumber.value) {
    console.log('🔧 DeviceDetails: Updating serial_number from', editableSerialNumber.value, 'to', newSerialNumber);
    isUpdatingFromProps.value = true;
    editableSerialNumber.value = newSerialNumber || '';
    isUpdatingFromProps.value = false;
  }
});

watch(() => props.device.model, (newModel) => {
  if (newModel !== editableModel.value) {
    console.log('🔧 DeviceDetails: Updating model from', editableModel.value, 'to', newModel);
    isUpdatingFromProps.value = true;
    editableModel.value = newModel || '';
    isUpdatingFromProps.value = false;
  }
});

watch(() => props.device.manufacturer, (newManufacturer) => {
  if (newManufacturer !== editableManufacturer.value) {
    console.log('🔧 DeviceDetails: Updating manufacturer from', editableManufacturer.value, 'to', newManufacturer);
    isUpdatingFromProps.value = true;
    editableManufacturer.value = newManufacturer || '';
    isUpdatingFromProps.value = false;
  }
});

watch(() => props.device.warranty_status, (newWarrantyStatus) => {
  if (newWarrantyStatus !== editableWarrantyStatus.value) {
    console.log('🔧 DeviceDetails: Updating warranty_status from', editableWarrantyStatus.value, 'to', newWarrantyStatus);
    isUpdatingFromProps.value = true;
    editableWarrantyStatus.value = newWarrantyStatus || '';
    isUpdatingFromProps.value = false;
  }
});

// Watch for changes in editable fields and emit updates
watch(editableName, (newName, oldName) => {
  if (!isUpdatingFromProps.value && newName !== oldName) {
    console.log(`DeviceDetails: Emitting update:name with value: ${newName} (was: ${oldName})`);
    emit("update:name", newName);
  }
}, { immediate: false });

watch(editableHostname, (newHostname, oldHostname) => {
  if (!isUpdatingFromProps.value && newHostname !== oldHostname) {
    console.log(`DeviceDetails: Emitting update:hostname with value: ${newHostname} (was: ${oldHostname})`);
    emit("update:hostname", newHostname);
  }
}, { immediate: false });

watch(editableSerialNumber, (newSerialNumber, oldSerialNumber) => {
  if (!isUpdatingFromProps.value && newSerialNumber !== oldSerialNumber) {
    console.log(`DeviceDetails: Emitting update:serial_number with value: ${newSerialNumber} (was: ${oldSerialNumber})`);
    emit("update:serial_number", newSerialNumber);
  }
}, { immediate: false });

watch(editableModel, (newModel, oldModel) => {
  if (!isUpdatingFromProps.value && newModel !== oldModel) {
    console.log(`DeviceDetails: Emitting update:model with value: ${newModel} (was: ${oldModel})`);
    emit("update:model", newModel);
  }
}, { immediate: false });

watch(editableManufacturer, (newManufacturer, oldManufacturer) => {
  if (!isUpdatingFromProps.value && newManufacturer !== oldManufacturer) {
    console.log(`DeviceDetails: Emitting update:manufacturer with value: ${newManufacturer} (was: ${oldManufacturer})`);
    emit("update:manufacturer", newManufacturer);
  }
}, { immediate: false });

watch(editableWarrantyStatus, (newWarrantyStatus, oldWarrantyStatus) => {
  if (!isUpdatingFromProps.value && newWarrantyStatus !== oldWarrantyStatus) {
    console.log(`DeviceDetails: Emitting update:warranty_status with value: ${newWarrantyStatus} (was: ${oldWarrantyStatus})`);
    emit("update:warranty_status", newWarrantyStatus);
  }
}, { immediate: false });

// Function to handle view button click
const handleViewClick = () => {
  emit('view', props.device.id);
};

// Helper functions for field editing
const startEditing = (field: string) => {
  switch (field) {
    case 'name':
      editingName.value = true;
      break;
    case 'hostname':
      editingHostname.value = true;
      break;
    case 'serial_number':
      editingSerialNumber.value = true;
      break;
    case 'model':
      editingModel.value = true;
      break;
    case 'manufacturer':
      editingManufacturer.value = true;
      break;
    case 'warranty_status':
      editingWarrantyStatus.value = true;
      break;
  }
};

const stopEditing = (field: string) => {
  switch (field) {
    case 'name':
      editingName.value = false;
      break;
    case 'hostname':
      editingHostname.value = false;
      break;
    case 'serial_number':
      editingSerialNumber.value = false;
      break;
    case 'model':
      editingModel.value = false;
      break;
    case 'manufacturer':
      editingManufacturer.value = false;
      break;
    case 'warranty_status':
      editingWarrantyStatus.value = false;
      break;
  }
};

const handleKeydown = (event: KeyboardEvent, field: string) => {
  if (event.key === 'Enter') {
    stopEditing(field);
  } else if (event.key === 'Escape') {
    // Reset to original value and stop editing
    switch (field) {
      case 'name':
        editableName.value = props.device.name || '';
        editingName.value = false;
        break;
      case 'hostname':
        editableHostname.value = props.device.hostname || '';
        editingHostname.value = false;
        break;
      case 'serial_number':
        editableSerialNumber.value = props.device.serial_number || '';
        editingSerialNumber.value = false;
        break;
      case 'model':
        editableModel.value = props.device.model || '';
        editingModel.value = false;
        break;
      case 'manufacturer':
        editableManufacturer.value = props.device.manufacturer || '';
        editingManufacturer.value = false;
        break;
      case 'warranty_status':
        editableWarrantyStatus.value = props.device.warranty_status || '';
        editingWarrantyStatus.value = false;
        break;
    }
  }
};

// Computed property for warranty status styling
const warrantyStatusClass = computed(() => {
  switch (editableWarrantyStatus.value) {
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

// Warranty status options
const warrantyStatusOptions = ['Active', 'Warning', 'Expired', 'Unknown'];
</script>

<template>
  <div class="bg-surface rounded-xl border border-default overflow-hidden hover:border-strong transition-colors">
    <!-- Header with device name and actions -->
    <div class="px-4 py-3 bg-surface-alt border-b border-default">
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-3 min-w-0 flex-1">
          <div class="w-2 h-2 bg-accent rounded-full flex-shrink-0"></div>

          <!-- Editable device name (title) -->
          <div v-if="editingName" class="flex-1">
            <input
              v-model="editableName"
              @blur="stopEditing('name')"
              @keydown="handleKeydown($event, 'name')"
              class="w-full bg-surface text-primary rounded px-2 py-1 text-sm font-medium focus:outline-none focus:ring-2 focus:ring-accent/50"
              placeholder="Enter device name..."
            />
          </div>
          <h3
            v-else
            @click="startEditing('name')"
            class="text-md font-medium text-primary truncate cursor-pointer hover:text-accent transition-colors"
            :title="editableName || 'Click to edit device name'"
          >
            {{ editableName || 'Unnamed Device' }}
          </h3>
          
          <!-- Warranty status badge -->
          <div v-if="editingWarrantyStatus" class="flex-shrink-0">
            <select
              v-model="editableWarrantyStatus"
              @blur="stopEditing('warranty_status')"
              @keydown="handleKeydown($event, 'warranty_status')"
              class="px-2 py-1 rounded-md text-xs font-medium border bg-surface text-primary focus:outline-none focus:ring-2 focus:ring-accent/50"
            >
              <option v-for="status in warrantyStatusOptions" :key="status" :value="status">
                {{ status }}
              </option>
            </select>
          </div>
          <div 
            v-else-if="editableWarrantyStatus" 
            @click="startEditing('warranty_status')"
            class="px-2 py-1 rounded-md text-xs font-medium border flex-shrink-0 cursor-pointer hover:opacity-80 transition-opacity"
            :class="warrantyStatusClass"
            :title="'Click to edit warranty status: ' + editableWarrantyStatus"
          >
            {{ editableWarrantyStatus }}
          </div>
        </div>
        
        <!-- Action buttons -->
        <div class="flex items-center gap-1 ml-2">
          <button
            @click="handleViewClick"
            class="p-1.5 text-tertiary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
            title="View device details"
          >
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" />
            </svg>
          </button>
          <button
            @click="emit('remove')"
            class="p-1.5 text-tertiary hover:text-status-error hover:bg-status-error/20 rounded-md transition-colors"
            title="Remove device"
          >
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>
      </div>
    </div>
    
    <!-- Compact device information -->
    <div class="p-4">
      <div class="flex flex-col gap-3">
        <!-- Primary info row -->
        <div class="grid grid-cols-2 gap-3 text-sm">
          <!-- Serial Number -->
          <div class="flex flex-col gap-1">
            <span class="text-xs text-tertiary uppercase tracking-wide">Serial</span>
            <div v-if="editingSerialNumber">
              <input
                v-model="editableSerialNumber"
                @blur="stopEditing('serial_number')"
                @keydown="handleKeydown($event, 'serial_number')"
                class="w-full bg-surface text-secondary rounded px-2 py-1 text-xs font-mono focus:outline-none focus:ring-2 focus:ring-accent/50"
                placeholder="Enter serial number..."
              />
            </div>
            <span
              v-else
              @click="startEditing('serial_number')"
              class="text-secondary font-mono text-sm cursor-pointer hover:text-accent transition-colors"
              :title="'Click to edit: ' + (editableSerialNumber || 'N/A')"
            >
              {{ editableSerialNumber || 'N/A' }}
            </span>
          </div>

          <!-- Model -->
          <div class="flex flex-col gap-1">
            <span class="text-xs text-tertiary uppercase tracking-wide">Model</span>
            <div v-if="editingModel">
              <input
                v-model="editableModel"
                @blur="stopEditing('model')"
                @keydown="handleKeydown($event, 'model')"
                class="w-full bg-surface text-secondary rounded px-2 py-1 text-xs focus:outline-none focus:ring-2 focus:ring-accent/50"
                placeholder="Enter model..."
              />
            </div>
            <span
              v-else
              @click="startEditing('model')"
              class="text-secondary text-sm truncate cursor-pointer hover:text-accent transition-colors"
              :title="'Click to edit: ' + (editableModel || 'Unknown')"
            >
              {{ editableModel || 'Unknown' }}
            </span>
          </div>
        </div>

        <!-- Secondary info row -->
        <div class="grid grid-cols-2 gap-3 text-sm">
          <!-- Manufacturer -->
          <div class="flex flex-col gap-1">
            <span class="text-xs text-tertiary uppercase tracking-wide">Manufacturer</span>
            <div v-if="editingManufacturer">
              <input
                v-model="editableManufacturer"
                @blur="stopEditing('manufacturer')"
                @keydown="handleKeydown($event, 'manufacturer')"
                class="w-full bg-surface text-secondary rounded px-2 py-1 text-xs focus:outline-none focus:ring-2 focus:ring-accent/50"
                placeholder="Enter manufacturer..."
              />
            </div>
            <span
              v-else
              @click="startEditing('manufacturer')"
              class="text-secondary text-sm truncate cursor-pointer hover:text-accent transition-colors"
              :title="'Click to edit: ' + (editableManufacturer || 'Unknown')"
            >
              {{ editableManufacturer || 'Unknown' }}
            </span>
          </div>

          <!-- Hostname -->
          <div class="flex flex-col gap-1">
            <span class="text-xs text-tertiary uppercase tracking-wide">Hostname</span>
            <div v-if="editingHostname">
              <input
                v-model="editableHostname"
                @blur="stopEditing('hostname')"
                @keydown="handleKeydown($event, 'hostname')"
                class="w-full bg-surface text-secondary rounded px-2 py-1 text-xs font-mono focus:outline-none focus:ring-2 focus:ring-accent/50"
                placeholder="Enter hostname..."
              />
            </div>
            <span
              v-else
              @click="startEditing('hostname')"
              class="text-secondary font-mono text-sm truncate cursor-pointer hover:text-accent transition-colors"
              :title="'Click to edit: ' + (editableHostname || 'N/A')"
            >
              {{ editableHostname || 'N/A' }}
            </span>
          </div>
        </div>

        <!-- Quick action button -->
        <div class="pt-2 border-t border-default">
          <button
            @click="handleViewClick"
            class="w-full flex items-center justify-center gap-2 px-3 py-2 bg-accent/10 text-accent rounded-lg hover:bg-accent/20 transition-colors text-sm font-medium"
          >
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            View Details
          </button>
        </div>
      </div>
    </div>
  </div>
</template>