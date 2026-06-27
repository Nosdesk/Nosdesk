<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 sm:gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <div>
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('admin-backup-title') }}</h1>
        <p class="text-secondary text-sm sm:text-base mt-1">{{ $t('admin-backup-description') }}</p>
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
              <span class="font-medium text-primary text-sm sm:text-base block">{{ $t('admin-backup-create-heading') }}</span>
              <p class="text-xs sm:text-sm text-secondary mt-1">{{ $t('admin-backup-create-description') }}</p>
            </div>
          </div>

          <!-- Export options -->
          <div class="flex flex-col gap-3">
            <ToggleSwitch
              v-model="includeSensitive"
              :label="$t('admin-backup-include-sensitive-label')"
              :description="$t('admin-backup-include-sensitive-description')"
            />

            <!-- Password fields when sensitive data is included -->
            <div v-if="includeSensitive" class="flex flex-col gap-4">
              <div class="p-3 bg-status-warning/10 border border-status-warning/30 rounded-lg">
                <p class="text-xs sm:text-sm text-status-warning">
                  {{ $t('admin-backup-encryption-warning') }}
                </p>
              </div>
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-3 sm:gap-4">
                <div class="flex flex-col gap-1.5">
                  <label class="block text-xs sm:text-sm font-medium text-secondary">{{ $t('admin-backup-encryption-password-label') }}</label>
                  <PasswordInput
                    v-model="exportPassword"
                    :placeholder="$t('admin-backup-encryption-password-placeholder')"
                    input-class="text-sm"
                  />
                </div>
                <div class="flex flex-col gap-1.5">
                  <label class="block text-xs sm:text-sm font-medium text-secondary">{{ $t('admin-backup-confirm-password-label') }}</label>
                  <PasswordInput
                    v-model="exportPasswordConfirm"
                    :placeholder="$t('admin-backup-confirm-password-placeholder')"
                    input-class="text-sm"
                  />
                </div>
              </div>
              <p v-if="includeSensitive && exportPassword && exportPassword !== exportPasswordConfirm" class="text-xs sm:text-sm text-status-error">
                {{ $t('admin-backup-passwords-no-match') }}
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
              <Spinner v-if="isExporting" />
              {{ isExporting ? $t('admin-backup-creating') : $t('admin-backup-create-button') }}
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
                <Icon name="archive" size="md" />
              </div>
              <span class="font-medium text-primary text-sm sm:text-base">{{ $t('admin-backup-recent-heading') }}</span>
            </div>
            <button @click="loadJobs" class="flex-shrink-0 text-sm text-accent hover:text-accent-hover">
              {{ $t('admin-backup-refresh') }}
            </button>
          </div>

          <!-- Content -->
          <div v-if="isLoadingJobs" class="flex items-center justify-center py-8">
            <Spinner size="lg" class="text-accent" />
          </div>

          <div v-else-if="exportJobs.length === 0" class="text-center py-8 text-secondary text-sm">
            {{ $t('admin-backup-empty') }}
          </div>

          <div v-else class="flex flex-col gap-2">
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
                  {{ formatDateTime(job.created_at) }}
                </span>

                <!-- Encrypted badge -->
                <span v-if="job.include_sensitive" class="text-xs px-1.5 py-0.5 bg-status-warning/20 text-status-warning rounded font-medium">
                  {{ $t('admin-backup-encrypted-badge') }}
                </span>

                <!-- File size / status -->
                <span class="text-xs text-secondary">
                  <span v-if="job.file_size">{{ formatFileSize(job.file_size) }}</span>
                  <span v-else-if="job.status === 'processing'">{{ $t('admin-backup-creating-status') }}</span>
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
                    :title="$t('admin-backup-download-title')"
                  >
                    <Icon name="download" />
                  </button>
                  <button
                    @click="deleteJob(job.id)"
                    class="p-2 text-status-error hover:bg-status-error/10 rounded-lg transition-colors"
                    :title="$t('admin-backup-delete-title')"
                  >
                    <Icon name="trash" />
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
              <Icon name="copyMd" size="md" />
            </div>
            <div class="flex-1 min-w-0">
              <span class="font-medium text-primary text-sm sm:text-base block">{{ $t('admin-backup-docs-heading') }}</span>
              <p class="text-xs sm:text-sm text-secondary mt-1">{{ $t('admin-backup-docs-description') }}</p>
            </div>
          </div>

          <!-- Export button with progress -->
          <div class="flex flex-col sm:flex-row items-stretch sm:items-center gap-2 sm:gap-3 sm:pl-12">
            <button
              @click="exportDocumentation"
              :disabled="isExportingDocs"
              class="px-4 py-2 bg-accent text-on-accent rounded-lg text-sm font-medium hover:bg-accent-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
            >
              <Spinner v-if="isExportingDocs" />
              <Icon v-else name="download" />
              {{ isExportingDocs ? (docsExportProgress ? t('admin-backup-docs-exporting', { current: docsExportProgress.current, total: docsExportProgress.total }) : $t('admin-backup-docs-preparing')) : $t('admin-backup-docs-export') }}
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
              <Icon name="refresh" size="md" />
            </div>
            <div class="flex-1 min-w-0">
              <span class="font-medium text-primary text-sm sm:text-base block">{{ $t('admin-backup-restore-heading') }}</span>
              <p class="text-xs sm:text-sm text-secondary mt-1">{{ $t('admin-backup-restore-description') }}</p>
            </div>
          </div>

          <!-- Upload area -->
          <div
            class="border-2 border-dashed border-default rounded-lg p-4 sm:p-6 cursor-pointer transition-colors hover:border-accent/50"
            :class="{ 'border-accent bg-accent/5': isDragging }"
            @dragover.prevent="isDragging = true"
            @dragleave.prevent="isDragging = false"
            @drop.prevent="handleDrop"
            @click="($refs.fileInput as HTMLInputElement | undefined)?.click()"
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
                  {{ $t('admin-backup-restore-dnd') }}
                </p>
                <span class="text-xs sm:text-sm text-accent hover:text-accent-hover font-medium">
                  {{ $t('admin-backup-restore-browse') }}
                </span>
              </div>
            </div>
          </div>

          <!-- Restore preview -->
          <div v-if="restorePreview" class="flex flex-col gap-3 sm:gap-4">
            <div class="p-3 bg-surface-alt rounded-lg">
              <h4 class="text-xs sm:text-sm font-medium text-primary mb-2">{{ $t('admin-backup-details-heading') }}</h4>
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-1.5 sm:gap-2 text-xs sm:text-sm">
                <div><span class="text-secondary">{{ $t('admin-backup-detail-created') }}</span> <span class="text-primary ml-1">{{ formatDateTime(restorePreview.manifest.created_at) }}</span></div>
                <div><span class="text-secondary">{{ $t('admin-backup-detail-version') }}</span> <span class="text-primary ml-1">{{ restorePreview.manifest.nosdesk_version }}</span></div>
                <div><span class="text-secondary">{{ $t('admin-backup-detail-files') }}</span> <span class="text-primary ml-1">{{ restorePreview.manifest.files.total_count }}</span></div>
                <div><span class="text-secondary">{{ $t('admin-backup-detail-size') }}</span> <span class="text-primary ml-1">{{ formatFileSize(restorePreview.manifest.files.total_size_bytes) }}</span></div>
              </div>

              <!-- Tables summary -->
              <div class="mt-3">
                <span class="text-xs sm:text-sm text-secondary">{{ $t('admin-backup-detail-tables') }}</span>
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
              <h4 class="text-xs sm:text-sm font-medium text-status-warning mb-2">{{ $t('admin-backup-warnings-heading') }}</h4>
              <ul class="text-xs sm:text-sm text-status-warning list-disc list-inside space-y-1">
                <li v-for="(warning, idx) in restorePreview.warnings" :key="idx">{{ warning }}</li>
              </ul>
            </div>

            <!-- Password for encrypted backup -->
            <div v-if="restorePreview.has_encrypted_sensitive" class="flex flex-col gap-1.5">
              <label class="block text-xs sm:text-sm font-medium text-secondary">{{ $t('admin-backup-decryption-password-label') }}</label>
              <div class="max-w-full sm:max-w-md">
                <PasswordInput
                  v-model="restorePassword"
                  :placeholder="$t('admin-backup-decryption-password-placeholder')"
                  input-class="text-sm"
                />
              </div>
            </div>

            <!-- Restore confirmation -->
            <div class="p-3 bg-status-error/10 border border-status-error/30 rounded-lg">
              <p class="text-xs sm:text-sm text-status-error">
                {{ $t('admin-backup-restore-warning') }}
              </p>
            </div>

            <!-- Restore actions -->
            <div class="flex flex-col sm:flex-row items-stretch sm:items-center gap-2 sm:gap-3">
              <button
                @click="executeRestore"
                :disabled="isRestoring || (restorePreview.has_encrypted_sensitive && !restorePassword)"
                class="px-4 py-2 bg-status-warning text-white rounded-lg text-sm font-medium hover:bg-status-warning/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
              >
                <Spinner v-if="isRestoring" />
                {{ isRestoring ? $t('admin-backup-restoring') : $t('admin-backup-restore-button') }}
              </button>
              <button
                @click="cancelRestore"
                class="px-4 py-2 border border-default rounded-lg text-sm text-secondary hover:bg-surface-alt transition-colors"
              >
                {{ $t('admin-backup-cancel') }}
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>

    <ConfirmModal
      :show="pendingDeleteJobId !== null"
      variant="danger"
      :title="$t('admin-backup-delete-confirm-title')"
      :message="$t('admin-backup-delete-confirm-message')"
      :confirm-label="$t('admin-backup-delete-confirm-label')"
      @confirm="doDeleteJob"
      @close="pendingDeleteJobId = null"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { useFluent } from 'fluent-vue';

import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import PasswordInput from '@/components/common/PasswordInput.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import Icon from '@/components/common/Icon.vue';
import Spinner from '@/components/common/Spinner.vue';
import backupService from '@/services/backupService';
import { useToastStore } from '@nosdesk/core/stores/toast';
import { formatDateTime } from '@nosdesk/core/utils/dateUtils';
import { downloadDocumentationExport, type ExportProgress } from '@/services/markdownExportService';
import type { BackupJob, RestorePreview } from '@nosdesk/core/types/backup';
import { formatFileSize } from '@nosdesk/core/utils/formatFileSize';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const toast = useToastStore();

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

const pendingDeleteJobId = ref<string | null>(null);

const deleteJob = (id: string) => {
  pendingDeleteJobId.value = id;
};

const doDeleteJob = async () => {
  const id = pendingDeleteJobId.value;
  pendingDeleteJobId.value = null;
  if (!id) return;
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
    toast.error(t('admin-backup-docs-error'));
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
    toast.warning(t('admin-backup-restore-not-zip'));
    return;
  }

  try {
    const job = await backupService.uploadRestore(file);
    restoreJobId.value = job.id;
    restorePreview.value = await backupService.getRestorePreview(job.id);
  } catch (error) {
    console.error('Failed to upload backup:', error);
    toast.error(t('admin-backup-upload-error'));
  }
};

const executeRestore = async () => {
  if (!restoreJobId.value) return;

  isRestoring.value = true;
  try {
    const result = await backupService.executeRestore(restoreJobId.value, {
      password: restorePreview.value?.has_encrypted_sensitive ? restorePassword.value : undefined,
    });

    toast.success(t('admin-backup-restore-success', { files: result.files_restored, message: result.message }));
    cancelRestore();
    await loadJobs();
  } catch (error) {
    console.error('Failed to restore backup:', error);
    toast.error(t('admin-backup-restore-error'));
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
