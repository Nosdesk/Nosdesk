<script setup lang="ts">
import { formatDate, formatDateTime } from '@/utils/dateUtils';
import { ref, onMounted } from 'vue';
import { useRouter } from 'vue-router';
import axios from 'axios';
import BackButton from '@/components/common/BackButton.vue';
import Modal from '@/components/Modal.vue';

const router = useRouter();

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
const sampleTemplates = [
  {
    type: 'users',
    name: 'Users Template',
    description: 'Import user accounts with roles and contact information',
    fields: ['username', 'email', 'first_name', 'last_name', 'role', 'department', 'phone']
  },
  {
    type: 'devices',
    name: 'Devices Template',
    description: 'Import devices with hardware details and ownership information',
    fields: ['name', 'type', 'serial_number', 'manufacturer', 'model', 'owner_email', 'status']
  },
  {
    type: 'tickets',
    name: 'Tickets Template',
    description: 'Import support tickets with details and assignees',
    fields: ['title', 'description', 'status', 'priority', 'category', 'assignee_email', 'reporter_email']
  }
];

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
    errorMessage.value = 'Please select a file to import';
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
      lastImport.value = formatDateTime();
      successMessage.value = 'Import completed successfully';
      
      // Close the modal
      showImportModal.value = false;
    } else {
      importStatus.value = 'error';
      errorMessage.value = response.data.message || 'Import failed';
    }
  } catch (error: any) {
    console.error('Import error:', error);
    importStatus.value = 'error';
    errorMessage.value = error.response?.data?.message || 'Failed to import data';
  } finally {
    isLoading.value = false;
  }
};

// Download sample template
const downloadTemplate = (type: string) => {
  // This would normally generate and download a CSV file
  // For now, we'll just show a success message
  successMessage.value = `${type} template downloaded`;
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
      <BackButton fallbackRoute="/admin/data-import" label="Back to Data Import" />
    </div>
    
    <div class="flex flex-col gap-4 px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-6">
        <h1 class="text-2xl font-bold text-primary">CSV Import</h1>
        <p class="text-secondary mt-2">
          Import data from CSV files into your system
        </p>
      </div>
      
      <!-- Status Messages -->
      <div 
        v-if="successMessage" 
        class="p-4 bg-green-900/50 text-green-400 rounded-lg border border-green-700"
      >
        {{ successMessage }}
      </div>
      
      <div 
        v-if="errorMessage" 
        class="p-4 bg-red-900/50 text-red-400 rounded-lg border border-red-700"
      >
        {{ errorMessage }}
      </div>
      
      <!-- Action buttons -->
      <div class="flex flex-wrap gap-3 mb-4">
        <button 
          @click="showImportDialog"
          class="px-4 py-2 bg-brand-blue text-white rounded-lg hover:opacity-90 transition-colors flex items-center gap-2"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M15 13l-3-3m0 0l-3 3m3-3v12" />
          </svg>
          Import Data
        </button>
        
        <button
          @click="showTemplateDialog"
          class="px-4 py-2 bg-surface-alt text-primary rounded-lg hover:bg-surface-hover transition-colors border border-subtle flex items-center gap-2"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
          </svg>
          Download Templates
        </button>
      </div>
      
      <!-- Import status card (shows after an import) -->
      <div v-if="importStatus !== 'none'" class="bg-surface border border-default rounded-lg p-6 mb-4">
        <div class="flex flex-col md:flex-row md:justify-between md:items-center gap-4">
          <div>
            <h2 class="text-xl font-medium text-primary mb-2">Import Status</h2>
            <div class="flex items-center">
              <span 
                :class="[
                  'px-3 py-1 rounded-full text-sm inline-flex items-center border',
                  importStatus === 'success' ? 'bg-green-900/50 text-green-400 border-green-700' : 
                  importStatus === 'in-progress' ? 'bg-blue-900/50 text-blue-400 border-blue-700' :
                  'bg-red-900/50 text-red-400 border-red-700'
                ]"
              >
                <span class="h-2 w-2 rounded-full mr-2" 
                      :class="{ 
                        'bg-green-400': importStatus === 'success',
                        'bg-blue-400': importStatus === 'in-progress',
                        'bg-red-400': importStatus === 'error'
                      }"></span>
                {{ 
                  importStatus === 'success' ? 'Import Completed' : 
                  importStatus === 'in-progress' ? 'Import in Progress' : 
                  'Import Failed' 
                }}
              </span>
            </div>
            <p v-if="lastImport" class="text-sm text-secondary mt-2">
              Last import: {{ lastImport }}
            </p>
          </div>

          <div v-if="importStatus === 'success'" class="bg-surface-alt p-4 rounded-lg">
            <div class="text-center">
              <div class="text-lg text-primary">{{ importResults.total }}</div>
              <div class="text-xs text-secondary">Total Records</div>
            </div>
            <div class="flex justify-between mt-3">
              <div class="text-center px-3">
                <div class="text-green-400">{{ importResults.success }}</div>
                <div class="text-xs text-secondary">Successful</div>
              </div>
              <div class="text-center px-3">
                <div class="text-red-400">{{ importResults.errors }}</div>
                <div class="text-xs text-secondary">Failed</div>
              </div>
            </div>
          </div>
        </div>
      </div>
      
      <!-- Import Guidelines -->
      <div class="bg-surface border border-default rounded-lg p-6 mb-4">
        <h2 class="text-xl font-medium text-primary mb-4">CSV Import Guidelines</h2>
        <div class="flex flex-col gap-4 text-sm text-secondary">
          <div class="bg-blue-900/20 border border-blue-800/50 rounded-md p-4">
            <h3 class="font-medium text-blue-300 mb-2 flex items-center">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 mr-2" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
              CSV File Requirements
            </h3>
            <ul class="list-disc list-inside flex flex-col gap-1 ml-2">
              <li>Files must be in CSV format with UTF-8 encoding</li>
              <li>The first row must contain column headers matching the expected fields</li>
              <li>Required fields must not be empty</li>
              <li>Date fields should use the format YYYY-MM-DD</li>
              <li>Maximum file size: 10MB</li>
            </ul>
          </div>
          
          <div class="bg-amber-900/20 border border-amber-800/50 rounded-md p-4">
            <h3 class="font-medium text-amber-300 mb-2 flex items-center">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 mr-2" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
              </svg>
              Important Notes
            </h3>
            <ul class="list-disc list-inside flex flex-col gap-1 ml-2">
              <li>Existing records will be updated if they share a unique identifier (like email or ID)</li>
              <li>Data validation is performed before import - records with invalid data will be skipped</li>
              <li>For large imports, the process may take several minutes to complete</li>
              <li>Download and use our template files to ensure proper formatting</li>
            </ul>
          </div>
        </div>
      </div>
      
      <!-- Available templates -->
      <div class="bg-surface border border-default rounded-lg p-6">
        <h2 class="text-xl font-medium text-primary mb-4">Available Templates</h2>
        <p class="text-secondary mb-4">
          Use these templates as a starting point for your CSV imports
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
                  class="px-3 py-2 text-sm bg-brand-blue text-white rounded-md hover:opacity-90 transition-colors flex items-center gap-2"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
                  </svg>
                  Download
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
      title="Import Data from CSV"
      contentClass="max-w-lg"
      @close="showImportModal = false"
    >
      <div class="flex flex-col gap-4">
        <div>
          <label class="block text-sm font-medium text-secondary mb-1">
            Data Type
          </label>
          <select
            v-model="selectedFileType"
            class="w-full rounded-md bg-surface-alt border-subtle text-primary py-2 px-3 focus:border-brand-blue focus:ring focus:ring-brand-blue focus:ring-opacity-50"
          >
            <option value="users">Users</option>
            <option value="devices">Devices</option>
            <option value="tickets">Tickets</option>
          </select>
        </div>
        
        <div>
          <label class="block text-sm font-medium text-secondary mb-1">
            CSV File
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
              <div v-if="fileUploaded" class="text-blue-400 text-center mx-auto">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-12 w-12 mx-auto" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                </svg>
                <p class="text-sm mt-2">{{ uploadedFile?.name }}</p>
              </div>
              <div v-else class="flex text-sm text-tertiary">
                <label
                  for="file-upload"
                  class="relative cursor-pointer bg-surface-alt rounded-md font-medium text-blue-400 hover:text-blue-300 focus-within:outline-none"
                >
                  <span class="px-3 py-2 inline-block">Upload a file</span>
                  <input
                    id="file-upload"
                    name="file-upload"
                    type="file"
                    accept=".csv"
                    class="sr-only"
                    @change="handleFileSelect"
                  />
                </label>
                <p class="pl-1 pt-2">or drag and drop</p>
              </div>
              <p v-if="!fileUploaded" class="text-xs text-tertiary">
                CSV files up to 10MB
              </p>
            </div>
          </div>
        </div>
        
        <div class="pt-4 flex justify-end gap-3">
          <button
            @click="showImportModal = false"
            class="px-4 py-2 bg-surface-alt text-primary rounded-lg hover:bg-surface-hover transition-colors"
          >
            Cancel
          </button>
          <button
            @click="startImport"
            :disabled="!fileUploaded || isLoading"
            :class="[
              'px-4 py-2 text-white rounded-lg transition-colors flex items-center gap-2',
              !fileUploaded || isLoading ? 'bg-brand-blue/50 cursor-not-allowed' : 'bg-brand-blue hover:opacity-90'
            ]"
          >
            <svg v-if="isLoading" class="animate-spin h-4 w-4 text-white" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
            {{ isLoading ? 'Importing...' : 'Start Import' }}
          </button>
        </div>
      </div>
    </Modal>
    
    <!-- Templates Modal -->
    <Modal
      :show="showTemplateModal"
      title="CSV Templates"
      contentClass="max-w-lg"
      @close="showTemplateModal = false"
    >
      <div class="flex flex-col gap-4">
        <p class="text-secondary mb-4">
          Download our CSV templates to ensure your data is formatted correctly for import.
        </p>

        <div class="flex flex-col gap-3">
          <div v-for="template in sampleTemplates" :key="template.type"
               class="p-3 bg-surface-alt rounded-lg flex justify-between items-center">
            <div>
              <h4 class="text-primary font-medium">{{ template.name }}</h4>
              <p class="text-xs text-secondary">{{ template.fields.length }} fields</p>
            </div>
            <button 
              @click="downloadTemplate(template.type)"
              class="px-3 py-1 text-sm bg-brand-blue text-white rounded-md hover:opacity-90 transition-colors flex items-center gap-1"
            >
              <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
              </svg>
              Download
            </button>
          </div>
        </div>
        
        <div class="pt-4 flex justify-end">
          <button
            @click="showTemplateModal = false"
            class="px-4 py-2 bg-surface-alt text-primary rounded-lg hover:bg-surface-hover transition-colors"
          >
            Close
          </button>
        </div>
      </div>
    </Modal>
  </div>
</template> 