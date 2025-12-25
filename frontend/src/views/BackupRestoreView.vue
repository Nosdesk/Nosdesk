<template>
  <div class="flex-1">
    <!-- Navigation and actions bar -->
    <div class="pt-4 px-4 sm:px-6 flex justify-between items-center">
      <BackButton fallbackRoute="/admin/settings" label="Back to Administration" />
    </div>

    <div class="flex flex-col gap-4 sm:gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <div>
        <h1 class="text-xl sm:text-2xl font-bold text-primary">Backup & Restore</h1>
        <p class="text-secondary text-sm sm:text-base mt-1">Export and restore system data and attachments</p>
      </div>

      <!-- Export Section -->
      <div class="bg-surface border border-default rounded-xl">
        <div class="p-3 sm:p-4 flex flex-col gap-3 sm:gap-4">
          <!-- Header row with icon -->
          <div class="flex flex-row items-start gap-3">
            <div class="flex-shrink-0 h-9 w-9 sm:h-10 sm:w-10 rounded-lg bg-accent/15 flex items-center justify-center text-accent">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-8l-4-4m0 0L8 8m4-4v12" />
              </svg>
            </div>
            <div class="flex-1 min-w-0">
              <span class="font-medium text-primary text-sm sm:text-base block">Create Backup</span>
              <p class="text-xs sm:text-sm text-secondary mt-1">Export all system data and attachments to a ZIP archive</p>
            </div>
          </div>

          <!-- Export options -->
          <div class="space-y-3">
            <ToggleSwitch
              v-model="includeSensitive"
              label="Include sensitive data"
              description="Includes passwords, MFA secrets, and authentication tokens (encrypted with password)"
            />

            <!-- Password fields when sensitive data is included -->
            <div v-if="includeSensitive" class="flex flex-col gap-4">
              <div class="p-3 bg-status-warning/10 border border-status-warning/30 rounded-lg">
                <p class="text-xs sm:text-sm text-status-warning">
                  Sensitive data will be encrypted. If you lose the password, the data cannot be recovered.
                </p>
              </div>
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-3 sm:gap-4">
                <div class="space-y-1.5">
                  <label class="block text-xs sm:text-sm font-medium text-secondary">Encryption Password</label>
                  <PasswordInput
                    v-model="exportPassword"
                    placeholder="Enter encryption password"
                    input-class="text-sm"
                  />
                </div>
                <div class="space-y-1.5">
                  <label class="block text-xs sm:text-sm font-medium text-secondary">Confirm Password</label>
                  <PasswordInput
                    v-model="exportPasswordConfirm"
                    placeholder="Confirm encryption password"
                    input-class="text-sm"
                  />
                </div>
              </div>
              <p v-if="includeSensitive && exportPassword && exportPassword !== exportPasswordConfirm" class="text-xs sm:text-sm text-status-error">
                Passwords do not match
              </p>
            </div>
          </div>

          <!-- Export button -->
          <div class="flex flex-col sm:flex-row items-stretch sm:items-center gap-2 sm:gap-3">
            <button
              @click="startExport"
              :disabled="isExporting || (includeSensitive && (!exportPassword || exportPassword !== exportPasswordConfirm))"
              class="px-4 py-2 bg-accent text-on-accent rounded-lg text-sm font-medium hover:bg-accent-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
            >
              <svg v-if="isExporting" class="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24">
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
              {{ isExporting ? 'Creating Backup...' : 'Create Backup' }}
            </button>
          </div>
        </div>
      </div>

      <!-- Recent Backups Section -->
      <div class="bg-surface border border-default rounded-xl">
        <div class="p-3 sm:p-4 flex flex-col gap-3 sm:gap-4">
          <!-- Header -->
          <div class="flex flex-row items-center justify-between gap-3">
            <div class="flex flex-row items-center gap-3 min-w-0">
              <div class="flex-shrink-0 h-9 w-9 sm:h-10 sm:w-10 rounded-lg bg-accent/15 flex items-center justify-center text-accent">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M5 8h14M5 8a2 2 0 110-4h14a2 2 0 110 4M5 8v10a2 2 0 002 2h10a2 2 0 002-2V8m-9 4h4" />
                </svg>
              </div>
              <span class="font-medium text-primary text-sm sm:text-base">Recent Backups</span>
            </div>
            <button @click="loadJobs" class="flex-shrink-0 text-sm text-accent hover:text-accent-hover">
              Refresh
            </button>
          </div>

          <!-- Content -->
          <div v-if="isLoadingJobs" class="flex items-center justify-center py-8">
            <svg class="animate-spin h-6 w-6 text-accent" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
          </div>

          <div v-else-if="exportJobs.length === 0" class="text-center py-8 text-secondary text-sm">
            No backups yet. Create your first backup above.
          </div>

          <div v-else class="space-y-2">
            <div
              v-for="job in exportJobs"
              :key="job.id"
              class="p-3 bg-surface-alt rounded-lg"
            >
              <div class="flex flex-wrap items-center gap-2 sm:gap-3">
                <!-- Status indicator -->
                <span
                  class="flex-shrink-0 inline-flex h-2.5 w-2.5 rounded-full"
                  :class="{
                    'bg-status-success': job.status === 'completed',
                    'bg-status-error': job.status === 'failed',
                    'bg-status-warning animate-pulse': job.status === 'processing',
                    'bg-tertiary': job.status === 'pending',
                  }"
                ></span>

                <!-- Date -->
                <span class="text-xs sm:text-sm text-primary font-medium">
                  {{ formatDate(job.created_at) }}
                </span>

                <!-- Encrypted badge -->
                <span v-if="job.include_sensitive" class="text-xs px-1.5 py-0.5 bg-status-warning/20 text-status-warning rounded font-medium">
                  Encrypted
                </span>

                <!-- File size / status -->
                <span class="text-xs text-secondary">
                  <span v-if="job.file_size">{{ formatFileSize(job.file_size) }}</span>
                  <span v-else-if="job.status === 'processing'">Creating...</span>
                  <span v-else-if="job.error_message" class="text-status-error">{{ job.error_message }}</span>
                </span>

                <!-- Spacer -->
                <div class="flex-1"></div>

                <!-- Actions -->
                <div class="flex items-center gap-1">
                  <button
                    v-if="job.status === 'completed'"
                    @click="downloadBackup(job.id)"
                    class="p-2 text-accent hover:bg-accent/10 rounded-lg transition-colors"
                    title="Download"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
                    </svg>
                  </button>
                  <button
                    @click="deleteJob(job.id)"
                    class="p-2 text-status-error hover:bg-status-error/10 rounded-lg transition-colors"
                    title="Delete"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                    </svg>
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Documentation Export Section -->
      <div class="bg-surface border border-default rounded-xl">
        <div class="p-3 sm:p-4 flex flex-col gap-3 sm:gap-4">
          <!-- Header row with icon -->
          <div class="flex flex-row items-start gap-3">
            <div class="flex-shrink-0 h-9 w-9 sm:h-10 sm:w-10 rounded-lg bg-accent/15 flex items-center justify-center text-accent">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
              </svg>
            </div>
            <div class="flex-1 min-w-0">
              <span class="font-medium text-primary text-sm sm:text-base block">Export Documentation to Markdown</span>
              <p class="text-xs sm:text-sm text-secondary mt-1">Export all documentation pages as markdown files in a ZIP archive</p>
            </div>
          </div>

          <!-- Export button with progress -->
          <div class="flex flex-col sm:flex-row items-stretch sm:items-center gap-2 sm:gap-3 sm:pl-12">
            <button
              @click="exportDocumentation"
              :disabled="isExportingDocs"
              class="px-4 py-2 bg-accent text-on-accent rounded-lg text-sm font-medium hover:bg-accent-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
            >
              <svg v-if="isExportingDocs" class="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24">
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
              <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
              </svg>
              {{ isExportingDocs ? (docsExportProgress ? `Exporting ${docsExportProgress.current}/${docsExportProgress.total}...` : 'Preparing...') : 'Export as Markdown' }}
            </button>
            <span v-if="docsExportProgress" class="text-xs sm:text-sm text-secondary">
              {{ docsExportProgress.currentPage }}
            </span>
          </div>
        </div>
      </div>

      <!-- Restore Section -->
      <div class="bg-surface border border-default rounded-xl">
        <div class="p-3 sm:p-4 flex flex-col gap-3 sm:gap-4">
          <!-- Header row with icon -->
          <div class="flex flex-row items-start gap-3">
            <div class="flex-shrink-0 h-9 w-9 sm:h-10 sm:w-10 rounded-lg bg-status-warning/20 flex items-center justify-center text-status-warning">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
              </svg>
            </div>
            <div class="flex-1 min-w-0">
              <span class="font-medium text-primary text-sm sm:text-base block">Restore from Backup</span>
              <p class="text-xs sm:text-sm text-secondary mt-1">Upload a backup file to restore system data and attachments</p>
            </div>
          </div>

          <!-- Upload area -->
          <div
            class="border-2 border-dashed border-default rounded-lg p-4 sm:p-6 cursor-pointer transition-colors hover:border-accent/50"
            :class="{ 'border-accent bg-accent/5': isDragging }"
            @dragover.prevent="isDragging = true"
            @dragleave.prevent="isDragging = false"
            @drop.prevent="handleDrop"
            @click="$refs.fileInput?.click()"
          >
            <input
              type="file"
              ref="fileInput"
              accept=".zip"
              @change="handleFileSelect"
              class="hidden"
            />
            <div class="flex flex-col items-center justify-center gap-2">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-8 w-8 sm:h-10 sm:w-10 text-tertiary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M15 13l-3-3m0 0l-3 3m3-3v12" />
              </svg>
              <div class="text-center">
                <p class="text-xs sm:text-sm text-secondary">
                  Drag and drop a backup file here, or
                </p>
                <span class="text-xs sm:text-sm text-accent hover:text-accent-hover font-medium">
                  browse to select a file
                </span>
              </div>
            </div>
          </div>

          <!-- Restore preview -->
          <div v-if="restorePreview" class="space-y-3 sm:space-y-4">
            <div class="p-3 bg-surface-alt rounded-lg">
              <h4 class="text-xs sm:text-sm font-medium text-primary mb-2">Backup Details</h4>
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-1.5 sm:gap-2 text-xs sm:text-sm">
                <div><span class="text-secondary">Created:</span> <span class="text-primary ml-1">{{ formatDate(restorePreview.manifest.created_at) }}</span></div>
                <div><span class="text-secondary">Version:</span> <span class="text-primary ml-1">{{ restorePreview.manifest.nosdesk_version }}</span></div>
                <div><span class="text-secondary">Files:</span> <span class="text-primary ml-1">{{ restorePreview.manifest.files.total_count }}</span></div>
                <div><span class="text-secondary">Size:</span> <span class="text-primary ml-1">{{ formatFileSize(restorePreview.manifest.files.total_size_bytes) }}</span></div>
              </div>

              <!-- Tables summary -->
              <div class="mt-3">
                <span class="text-xs sm:text-sm text-secondary">Tables:</span>
                <div class="flex flex-wrap gap-1 mt-1">
                  <span
                    v-for="(info, table) in restorePreview.manifest.tables"
                    :key="table"
                    class="text-xs px-1.5 py-0.5 bg-surface rounded text-secondary"
                  >
                    {{ table }}: {{ info.count }}
                  </span>
                </div>
              </div>
            </div>

            <!-- Warnings -->
            <div v-if="restorePreview.warnings.length > 0" class="p-3 bg-status-warning/10 border border-status-warning/30 rounded-lg">
              <h4 class="text-xs sm:text-sm font-medium text-status-warning mb-2">Warnings</h4>
              <ul class="text-xs sm:text-sm text-status-warning list-disc list-inside space-y-1">
                <li v-for="(warning, idx) in restorePreview.warnings" :key="idx">{{ warning }}</li>
              </ul>
            </div>

            <!-- Password for encrypted backup -->
            <div v-if="restorePreview.has_encrypted_sensitive" class="space-y-1.5">
              <label class="block text-xs sm:text-sm font-medium text-secondary">Decryption Password</label>
              <div class="max-w-full sm:max-w-md">
                <PasswordInput
                  v-model="restorePassword"
                  placeholder="Enter backup encryption password"
                  input-class="text-sm"
                />
              </div>
            </div>

            <!-- Restore confirmation -->
            <div class="p-3 bg-status-error/10 border border-status-error/30 rounded-lg">
              <p class="text-xs sm:text-sm text-status-error">
                Restoring will replace existing files. This action cannot be undone.
              </p>
            </div>

            <!-- Restore actions -->
            <div class="flex flex-col sm:flex-row items-stretch sm:items-center gap-2 sm:gap-3">
              <button
                @click="executeRestore"
                :disabled="isRestoring || (restorePreview.has_encrypted_sensitive && !restorePassword)"
                class="px-4 py-2 bg-status-warning text-white rounded-lg text-sm font-medium hover:bg-status-warning/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
              >
                <svg v-if="isRestoring" class="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24">
                  <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                  <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                </svg>
                {{ isRestoring ? 'Restoring...' : 'Restore Files' }}
              </button>
              <button
                @click="cancelRestore"
                class="px-4 py-2 border border-default rounded-lg text-sm text-secondary hover:bg-surface-alt transition-colors"
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import BackButton from '@/components/common/BackButton.vue';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import PasswordInput from '@/components/common/PasswordInput.vue';
import backupService from '@/services/backupService';
import { downloadDocumentationExport, type ExportProgress } from '@/services/markdownExportService';
import type { BackupJob, RestorePreview } from '@/types/backup';

// Export state
const includeSensitive = ref(false);
const exportPassword = ref('');
const exportPasswordConfirm = ref('');
const isExporting = ref(false);

// Jobs state
const jobs = ref<BackupJob[]>([]);
const isLoadingJobs = ref(false);

// Restore state
const isDragging = ref(false);
const fileInput = ref<HTMLInputElement | null>(null);
const restoreJobId = ref<string | null>(null);
const restorePreview = ref<RestorePreview | null>(null);
const restorePassword = ref('');
const isRestoring = ref(false);

// Documentation export state
const isExportingDocs = ref(false);
const docsExportProgress = ref<ExportProgress | null>(null);

// Computed
const exportJobs = computed(() =>
  jobs.value.filter(j => j.job_type === 'export').slice(0, 10)
);

// Methods
const formatDate = (dateStr: string) => {
  return new Date(dateStr).toLocaleString();
};

const formatFileSize = (bytes: number) => {
  return backupService.formatFileSize(bytes);
};

const loadJobs = async () => {
  isLoadingJobs.value = true;
  try {
    jobs.value = await backupService.getJobs();
  } catch (error) {
    console.error('Failed to load backup jobs:', error);
  } finally {
    isLoadingJobs.value = false;
  }
};

const startExport = async () => {
  isExporting.value = true;
  try {
    const job = await backupService.startExport({
      include_sensitive: includeSensitive.value,
      password: includeSensitive.value ? exportPassword.value : undefined,
    });

    // Poll for completion
    const completedJob = await backupService.pollJob(job.id);

    if (completedJob.status === 'completed') {
      // Download automatically
      backupService.downloadBackup(completedJob.id);
    }

    // Refresh job list
    await loadJobs();

    // Reset form
    includeSensitive.value = false;
    exportPassword.value = '';
    exportPasswordConfirm.value = '';
  } catch (error) {
    console.error('Failed to create backup:', error);
  } finally {
    isExporting.value = false;
  }
};

const downloadBackup = (id: string) => {
  backupService.downloadBackup(id);
};

const deleteJob = async (id: string) => {
  if (!confirm('Are you sure you want to delete this backup?')) return;

  try {
    await backupService.deleteJob(id);
    await loadJobs();
  } catch (error) {
    console.error('Failed to delete backup:', error);
  }
};

const exportDocumentation = async () => {
  isExportingDocs.value = true;
  docsExportProgress.value = null;
  try {
    await downloadDocumentationExport((progress) => {
      docsExportProgress.value = progress;
    });
  } catch (error) {
    console.error('Failed to export documentation:', error);
    alert('Failed to export documentation. Please check the console for details.');
  } finally {
    isExportingDocs.value = false;
    docsExportProgress.value = null;
  }
};

const handleFileSelect = async (event: Event) => {
  const input = event.target as HTMLInputElement;
  if (input.files?.length) {
    await uploadFile(input.files[0]);
  }
};

const handleDrop = async (event: DragEvent) => {
  isDragging.value = false;
  if (event.dataTransfer?.files.length) {
    await uploadFile(event.dataTransfer.files[0]);
  }
};

const uploadFile = async (file: File) => {
  if (!file.name.endsWith('.zip')) {
    alert('Please select a .zip backup file');
    return;
  }

  try {
    const job = await backupService.uploadRestore(file);
    restoreJobId.value = job.id;
    restorePreview.value = await backupService.getRestorePreview(job.id);
  } catch (error) {
    console.error('Failed to upload backup:', error);
    alert('Failed to upload backup file');
  }
};

const executeRestore = async () => {
  if (!restoreJobId.value) return;

  isRestoring.value = true;
  try {
    const result = await backupService.executeRestore(restoreJobId.value, {
      password: restorePreview.value?.has_encrypted_sensitive ? restorePassword.value : undefined,
    });

    alert(`Restore completed: ${result.files_restored} files restored. ${result.message}`);
    cancelRestore();
    await loadJobs();
  } catch (error) {
    console.error('Failed to restore backup:', error);
    alert('Restore failed. Please check the console for details.');
  } finally {
    isRestoring.value = false;
  }
};

const cancelRestore = () => {
  restoreJobId.value = null;
  restorePreview.value = null;
  restorePassword.value = '';
  if (fileInput.value) {
    fileInput.value.value = '';
  }
};

// Load jobs on mount
onMounted(() => {
  loadJobs();
});
</script>
