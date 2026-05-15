<script setup lang="ts">
import { formatDateTime } from '@/utils/dateUtils';
import { computed, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import axios from 'axios';
import BackButton from '@/components/common/BackButton.vue';
import Icon from '@/components/common/Icon.vue';
import Spinner from '@/components/common/Spinner.vue';
import Modal from '@/components/Modal.vue';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

// State variables
const isLoading = ref(false);
const errorMessage = ref<string | null>(null);
const successMessage = ref<string | null>(null);
const fileUploaded = ref(false);
const selectedFileType = ref<'users' | 'devices' | 'tickets'>('users');
const uploadedFile = ref<File | null>(null);
const lastImport = ref<string | null>(null);
const importStatus = ref<'none' | 'in-progress' | 'success' | 'error'>('none');
const importResults = ref({
  total: 0,
  success: 0,
  errors: 0
});

// Sample templates
const sampleTemplates = computed(() => [
  {
    type: 'users',
    name: t('csv-import-template-users-name'),
    description: t('csv-import-template-users-description'),
    fields: ['username', 'email', 'first_name', 'last_name', 'role', 'department', 'phone']
  },
  {
    type: 'devices',
    name: t('csv-import-template-devices-name'),
    description: t('csv-import-template-devices-description'),
    fields: ['name', 'type', 'serial_number', 'manufacturer', 'model', 'owner_email', 'status']
  },
  {
    type: 'tickets',
    name: t('csv-import-template-tickets-name'),
    description: t('csv-import-template-tickets-description'),
    fields: ['title', 'description', 'status', 'priority', 'category', 'assignee_email', 'reporter_email']
  }
]);

// Modals
const showImportModal = ref(false);
const showTemplateModal = ref(false);

// Handle file selection
const handleFileSelect = (event: Event) => {
  const input = event.target as HTMLInputElement;
  if (input.files && input.files.length > 0) {
    uploadedFile.value = input.files[0];
    fileUploaded.value = true;

    // Reset error messages when a new file is selected
    errorMessage.value = null;
  }
};

// Start import process
const startImport = async () => {
  if (!uploadedFile.value) {
    errorMessage.value = t('csv-import-error-no-file');
    return;
  }

  isLoading.value = true;
  importStatus.value = 'in-progress';
  errorMessage.value = null;

  try {
    // Create form data for file upload
    const formData = new FormData();
    formData.append('file', uploadedFile.value);
    formData.append('type', selectedFileType.value);

    // This is a placeholder - replace with actual API endpoint
    const response = await axios.post('/api/import/csv', formData, {
      headers: {
        'Content-Type': 'multipart/form-data'
      }
    });

    if (response.data.success) {
      importStatus.value = 'success';
      importResults.value = {
        total: response.data.total || 0,
        success: response.data.success_count || 0,
        errors: response.data.error_count || 0
      };
      lastImport.value = formatDateTime(new Date());
      successMessage.value = t('csv-import-success-completed');

      // Close the modal
      showImportModal.value = false;
    } else {
      importStatus.value = 'error';
      errorMessage.value = response.data.message || t('csv-import-error-failed');
    }
  } catch (error) {
    console.error('Import error:', error);
    importStatus.value = 'error';
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || t('csv-import-error-generic');
  } finally {
    isLoading.value = false;
  }
};

// Download sample template
const downloadTemplate = (type: string) => {
  // This would normally generate and download a CSV file
  // Show success message for now
  successMessage.value = t('csv-import-toast-template-downloaded', { type });
  setTimeout(() => {
    successMessage.value = null;
  }, 3000);
};

// Show the import modal
const showImportDialog = () => {
  // Reset state
  fileUploaded.value = false;
  uploadedFile.value = null;
  errorMessage.value = null;

  // Show modal
  showImportModal.value = true;
};

// Show template modal
const showTemplateDialog = () => {
  showTemplateModal.value = true;
};
</script>

<template>
  <div class="flex-1">
    <!-- Navigation and actions bar -->
    <div class="pt-4 px-6 flex justify-between items-center">
      <BackButton fallbackRoute="/admin/data-import" :label="$t('csv-import-back')" />
    </div>

    <div class="flex flex-col gap-4 px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-6">
        <h1 class="text-2xl font-bold text-primary">{{ $t('csv-import-title') }}</h1>
        <p class="text-secondary mt-2">
          {{ $t('csv-import-subtitle') }}
        </p>
      </div>

      <!-- Status Messages -->
      <div
        v-if="successMessage"
        class="p-4 bg-status-success/20 text-status-success rounded-lg border border-status-success/50"
      >
        {{ successMessage }}
      </div>

      <div
        v-if="errorMessage"
        class="p-4 bg-status-error/20 text-status-error rounded-lg border border-status-error/50"
      >
        {{ errorMessage }}
      </div>

      <!-- Action buttons -->
      <div class="flex flex-wrap gap-3 mb-4">
        <button
          @click="showImportDialog"
          class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 transition-colors flex items-center gap-2"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M15 13l-3-3m0 0l-3 3m3-3v12" />
          </svg>
          {{ $t('csv-import-action-import') }}
        </button>

        <button
          @click="showTemplateDialog"
          class="px-4 py-2 bg-surface-alt text-primary rounded-lg hover:bg-surface-hover transition-colors border border-subtle flex items-center gap-2"
        >
          <Icon name="download" />
          {{ $t('csv-import-action-templates') }}
        </button>
      </div>

      <!-- Import status card (shows after an import) -->
      <div v-if="importStatus !== 'none'" class="bg-surface border border-default rounded-lg p-6 mb-4">
        <div class="flex flex-col md:flex-row md:justify-between md:items-center gap-4">
          <div>
            <h2 class="text-xl font-medium text-primary mb-2">{{ $t('csv-import-status-heading') }}</h2>
            <div class="flex items-center">
              <span
                :class="[
                  'px-3 py-1 rounded-full text-sm inline-flex items-center border',
                  importStatus === 'success' ? 'bg-status-success/20 text-status-success border-status-success/50' :
                  importStatus === 'in-progress' ? 'bg-accent/20 text-accent border-accent' :
                  'bg-status-error/20 text-status-error border-status-error/50'
                ]"
              >
                <span class="h-2 w-2 rounded-full mr-2"
                      :class="{
                        'bg-status-success': importStatus === 'success',
                        'bg-accent': importStatus === 'in-progress',
                        'bg-status-error': importStatus === 'error'
                      }"></span>
                {{
                  importStatus === 'success' ? $t('csv-import-status-success') :
                  importStatus === 'in-progress' ? $t('csv-import-status-in-progress') :
                  $t('csv-import-status-error')
                }}
              </span>
            </div>
            <p v-if="lastImport" class="text-sm text-secondary mt-2">
              {{ $t('csv-import-last-import', { date: lastImport }) }}
            </p>
          </div>

          <div v-if="importStatus === 'success'" class="bg-surface-alt p-4 rounded-lg">
            <div class="text-center">
              <div class="text-lg text-primary">{{ importResults.total }}</div>
              <div class="text-xs text-secondary">{{ $t('csv-import-results-total') }}</div>
            </div>
            <div class="flex justify-between mt-3">
              <div class="text-center px-3">
                <div class="text-status-success">{{ importResults.success }}</div>
                <div class="text-xs text-secondary">{{ $t('csv-import-results-successful') }}</div>
              </div>
              <div class="text-center px-3">
                <div class="text-status-error">{{ importResults.errors }}</div>
                <div class="text-xs text-secondary">{{ $t('csv-import-results-failed') }}</div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Import Guidelines -->
      <div class="bg-surface border border-default rounded-lg p-6 mb-4">
        <h2 class="text-xl font-medium text-primary mb-4">{{ $t('csv-import-guidelines-heading') }}</h2>
        <div class="flex flex-col gap-4 text-sm text-secondary">
          <div class="bg-accent/10 border border-accent/30 rounded-md p-4">
            <h3 class="font-medium text-accent mb-2 flex items-center">
              <Icon name="info" size="md" class="mr-2" />
              {{ $t('csv-import-requirements-heading') }}
            </h3>
            <ul class="list-disc list-inside flex flex-col gap-1 ml-2">
              <li>{{ $t('csv-import-requirements-utf8') }}</li>
              <li>{{ $t('csv-import-requirements-headers') }}</li>
              <li>{{ $t('csv-import-requirements-required') }}</li>
              <li>{{ $t('csv-import-requirements-date-format') }}</li>
              <li>{{ $t('csv-import-requirements-max-size') }}</li>
            </ul>
          </div>

          <div class="bg-status-warning/20 border border-status-warning/50 rounded-md p-4">
            <h3 class="font-medium text-status-warning mb-2 flex items-center">
              <Icon name="warning" size="md" class="mr-2" />
              {{ $t('csv-import-notes-heading') }}
            </h3>
            <ul class="list-disc list-inside flex flex-col gap-1 ml-2">
              <li>{{ $t('csv-import-notes-updates') }}</li>
              <li>{{ $t('csv-import-notes-validation') }}</li>
              <li>{{ $t('csv-import-notes-duration') }}</li>
              <li>{{ $t('csv-import-notes-templates') }}</li>
            </ul>
          </div>
        </div>
      </div>

      <!-- Available templates -->
      <div class="bg-surface border border-default rounded-lg p-6">
        <h2 class="text-xl font-medium text-primary mb-4">{{ $t('csv-import-templates-heading') }}</h2>
        <p class="text-secondary mb-4">
          {{ $t('csv-import-templates-intro') }}
        </p>

        <div class="flex flex-col gap-4">
          <div v-for="template in sampleTemplates" :key="template.type"
               class="p-4 bg-surface-alt rounded-lg border border-subtle">
            <div class="flex items-start md:items-center flex-col md:flex-row md:justify-between">
              <div class="flex-1 mb-3 md:mb-0">
                <h3 class="text-primary font-medium">{{ template.name }}</h3>
                <p class="text-sm text-secondary mt-1">{{ template.description }}</p>
                <div class="mt-2 flex flex-wrap gap-2">
                  <span v-for="field in template.fields" :key="field"
                        class="px-2 py-1 bg-surface text-xs rounded-md text-secondary">
                    {{ field }}
                  </span>
                </div>
              </div>
              <div>
                <button
                  @click="downloadTemplate(template.type)"
                  class="px-3 py-2 text-sm bg-accent text-white rounded-md hover:opacity-90 transition-colors flex items-center gap-2"
                >
                  <Icon name="download" />
                  {{ $t('csv-import-template-download') }}
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Import Modal -->
    <Modal
      :show="showImportModal"
      :title="$t('csv-import-modal-import-title')"
      contentClass="max-w-lg"
      @close="showImportModal = false"
    >
      <div class="flex flex-col gap-4">
        <div>
          <label class="block text-sm font-medium text-secondary mb-1">
            {{ $t('csv-import-modal-data-type') }}
          </label>
          <select
            v-model="selectedFileType"
            class="w-full rounded-md bg-surface-alt border-subtle text-primary py-2 px-3 focus:border-accent focus:ring focus:ring-accent focus:ring-opacity-50"
          >
            <option value="users">{{ $t('csv-import-modal-type-users') }}</option>
            <option value="devices">{{ $t('csv-import-modal-type-devices') }}</option>
            <option value="tickets">{{ $t('csv-import-modal-type-tickets') }}</option>
          </select>
        </div>

        <div>
          <label class="block text-sm font-medium text-secondary mb-1">
            {{ $t('csv-import-modal-file-label') }}
          </label>
          <div class="mt-1 flex justify-center px-6 pt-5 pb-6 border-2 border-dashed border-subtle rounded-md">
            <div class="flex flex-col gap-1 text-center">
              <svg
                v-if="!fileUploaded"
                class="mx-auto h-12 w-12 text-tertiary"
                stroke="currentColor"
                fill="none"
                viewBox="0 0 48 48"
                aria-hidden="true"
              >
                <path
                  d="M28 8H12a4 4 0 00-4 4v20m32-12v8m0 0v8a4 4 0 01-4 4H12a4 4 0 01-4-4v-4m32-4l-3.172-3.172a4 4 0 00-5.656 0L28 28M8 32l9.172-9.172a4 4 0 015.656 0L28 28m0 0l4 4m4-24h8m-4-4v8m-12 4h.02"
                  stroke-width="2"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                />
              </svg>
              <div v-if="fileUploaded" class="text-accent text-center mx-auto">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-12 w-12 mx-auto" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                </svg>
                <p class="text-sm mt-2">{{ uploadedFile?.name }}</p>
              </div>
              <div v-else class="flex text-sm text-tertiary">
                <label
                  for="file-upload"
                  class="relative cursor-pointer bg-surface-alt rounded-md font-medium text-accent hover:text-accent focus-within:outline-none"
                >
                  <span class="px-3 py-2 inline-block">{{ $t('csv-import-modal-upload-link') }}</span>
                  <input
                    id="file-upload"
                    name="file-upload"
                    type="file"
                    accept=".csv"
                    class="sr-only"
                    @change="handleFileSelect"
                  />
                </label>
                <p class="pl-1 pt-2">{{ $t('csv-import-modal-drag-drop') }}</p>
              </div>
              <p v-if="!fileUploaded" class="text-xs text-tertiary">
                {{ $t('csv-import-modal-size-hint') }}
              </p>
            </div>
          </div>
        </div>

        <div class="pt-4 flex justify-end gap-3">
          <button
            @click="showImportModal = false"
            class="px-4 py-2 bg-surface-alt text-primary rounded-lg hover:bg-surface-hover transition-colors"
          >
            {{ $t('csv-import-modal-cancel') }}
          </button>
          <button
            @click="startImport"
            :disabled="!fileUploaded || isLoading"
            :class="[
              'px-4 py-2 text-white rounded-lg transition-colors flex items-center gap-2',
              !fileUploaded || isLoading ? 'bg-accent/50 cursor-not-allowed' : 'bg-accent hover:opacity-90'
            ]"
          >
            <Spinner v-if="isLoading" class="text-white" />
            {{ isLoading ? $t('csv-import-modal-starting') : $t('csv-import-modal-start') }}
          </button>
        </div>
      </div>
    </Modal>

    <!-- Templates Modal -->
    <Modal
      :show="showTemplateModal"
      :title="$t('csv-import-modal-templates-title')"
      contentClass="max-w-lg"
      @close="showTemplateModal = false"
    >
      <div class="flex flex-col gap-4">
        <p class="text-secondary mb-4">
          {{ $t('csv-import-modal-templates-intro') }}
        </p>

        <div class="flex flex-col gap-3">
          <div v-for="template in sampleTemplates" :key="template.type"
               class="p-3 bg-surface-alt rounded-lg flex justify-between items-center">
            <div>
              <h4 class="text-primary font-medium">{{ template.name }}</h4>
              <p class="text-xs text-secondary">{{ $t('csv-import-modal-fields-count', { count: template.fields.length }) }}</p>
            </div>
            <button
              @click="downloadTemplate(template.type)"
              class="px-3 py-1 text-sm bg-accent text-white rounded-md hover:opacity-90 transition-colors flex items-center gap-1"
            >
              <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
              </svg>
              {{ $t('csv-import-template-download') }}
            </button>
          </div>
        </div>

        <div class="pt-4 flex justify-end">
          <button
            @click="showTemplateModal = false"
            class="px-4 py-2 bg-surface-alt text-primary rounded-lg hover:bg-surface-hover transition-colors"
          >
            {{ $t('csv-import-modal-close') }}
          </button>
        </div>
      </div>
    </Modal>
  </div>
</template>
