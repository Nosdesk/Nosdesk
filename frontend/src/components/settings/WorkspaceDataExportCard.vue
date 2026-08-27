<template>
  <div class="bg-surface border border-default rounded-xl hover:border-strong transition-colors">
    <div class="p-4 flex flex-col gap-3">
      <div class="flex items-center gap-3">
        <div
          class="flex-shrink-0 h-9 w-9 rounded-lg bg-accent/20 flex items-center justify-center text-accent"
        >
          <Icon name="archive" size="md" />
        </div>

        <div class="flex-1 min-w-0">
          <span class="font-medium text-primary">{{ t('settings-export-title') }}</span>
        </div>

        <Button
          variant="secondary"
          size="sm"
          icon="download"
          :loading="busy"
          :disabled="!canRequest"
          @click="requestExport"
        >
          {{ busy ? t('settings-export-preparing') : requestLabel }}
        </Button>
      </div>

      <p class="text-secondary text-sm">
        {{ t('settings-export-description') }}
      </p>

      <!-- Optional password, only offered when starting a fresh export -->
      <div v-if="showPasswordField" class="max-w-sm">
        <FormInput
          v-model="password"
          type="password"
          size="sm"
          autocomplete="new-password"
          :label="t('settings-export-password-label')"
          :description="t('settings-export-password-hint')"
          :disabled="busy"
        />
      </div>

      <p v-else-if="rateLimited" class="text-tertiary text-xs flex items-center gap-1.5">
        <Icon name="clock" size="sm" />
        {{ t('settings-export-rate-limited') }}
      </p>
    </div>

    <!-- Status panel -->
    <div
      v-if="statusPanel"
      class="border-t border-default p-4 bg-surface-alt rounded-b-xl flex flex-col gap-3"
    >
      <!-- Processing -->
      <div v-if="isProcessing" class="flex items-center gap-2 text-sm text-secondary">
        <Spinner size="sm" />
        <span>{{ t('settings-export-status-processing') }}</span>
      </div>

      <!-- Ready -->
      <template v-else-if="isReady">
        <div class="flex items-start gap-2">
          <Icon name="checkCircle" size="sm" class="text-status-success mt-0.5 flex-shrink-0" />
          <div class="flex flex-col gap-0.5 min-w-0">
            <span class="text-sm text-status-success font-medium">
              {{ t('settings-export-status-ready') }}
            </span>
            <span class="text-xs text-tertiary">
              {{ readyMeta }}
            </span>
          </div>
        </div>
        <div>
          <Button variant="primary" size="sm" icon="download" @click="download">
            {{ t('settings-export-download') }}
          </Button>
        </div>
      </template>

      <!-- Expired -->
      <div v-else-if="isExpired" class="flex items-center gap-2 text-sm text-secondary">
        <Icon name="clock" size="sm" class="text-tertiary flex-shrink-0" />
        <span>{{ t('settings-export-status-expired') }}</span>
      </div>

      <!-- Failed -->
      <div v-else-if="isFailed" class="flex items-start gap-2">
        <Icon name="warning" size="sm" class="text-status-error mt-0.5 flex-shrink-0" />
        <span class="text-sm text-status-error">
          {{ job?.error_message || t('settings-export-status-failed') }}
        </span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { useFluent } from 'fluent-vue';
import { useToastStore } from '@nosdesk/core/stores/toast';
import Button from '@/components/common/Button.vue';
import Icon from '@/components/common/Icon.vue';
import FormInput from '@/components/common/FormInput.vue';
import Spinner from '@/components/common/Spinner.vue';
import {
  workspaceExportService,
  type WorkspaceExportJob,
} from '@/services/workspaceExportService';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const toast = useToastStore();

const job = ref<WorkspaceExportJob | null>(null);
const busy = ref(false);
const password = ref('');

const isProcessing = computed(
  () => job.value?.status === 'processing' || job.value?.status === 'pending'
);
const isReady = computed(() => job.value?.status === 'completed' && job.value.download_available);
const isExpired = computed(
  () => job.value?.status === 'completed' && !job.value.download_available
);
const isFailed = computed(() => job.value?.status === 'failed');
const statusPanel = computed(
  () => busy.value || isProcessing.value || isReady.value || isExpired.value || isFailed.value
);

// One completed export per 24h; disable the request while inside that window so
// the user gets a hint instead of a rejected request. The server is the real gate.
const rateLimited = computed(() => {
  if (job.value?.status !== 'completed') return false;
  const created = new Date(job.value.created_at).getTime();
  return Number.isFinite(created) && Date.now() - created < 24 * 60 * 60 * 1000;
});
const canRequest = computed(() => !busy.value && !isProcessing.value && !rateLimited.value);
const showPasswordField = computed(() => canRequest.value && !isReady.value);
const requestLabel = computed(() =>
  job.value ? t('settings-export-action-new') : t('settings-export-action')
);

const readyMeta = computed(() => {
  const parts: string[] = [];
  if (job.value?.file_size) parts.push(formatBytes(job.value.file_size));
  if (job.value?.expires_at) {
    parts.push(t('settings-export-status-ready-until', { date: formatDate(job.value.expires_at) }));
  }
  return parts.join(' · ');
});

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KB', 'MB', 'GB'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unit]}`;
}

function formatDate(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime())
    ? iso
    : d.toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
}

async function requestExport() {
  if (!canRequest.value) return;
  busy.value = true;
  try {
    const started = await workspaceExportService.requestExport(password.value);
    password.value = '';
    job.value = started;
    const finished = await workspaceExportService.pollExport(started.id);
    job.value = finished;
    if (finished.status === 'completed') {
      toast.success(t('settings-export-success'));
    } else {
      toast.error(finished.error_message || t('settings-export-status-failed'));
    }
  } catch (e) {
    const status = (e as { response?: { status?: number } })?.response?.status;
    if (status === 429) {
      toast.error(t('settings-export-rate-limited'));
    } else if (status === 409) {
      toast.error(t('settings-export-in-progress'));
      await refresh();
    } else {
      toast.error(t('settings-export-error'));
    }
  } finally {
    busy.value = false;
  }
}

function download() {
  if (job.value?.id) workspaceExportService.downloadExport(job.value.id);
}

async function refresh() {
  try {
    const latest = await workspaceExportService.getLatest();
    job.value = latest;
    // Resume tracking an export that is still running (e.g. after a reload).
    if (latest && (latest.status === 'processing' || latest.status === 'pending')) {
      busy.value = true;
      try {
        job.value = await workspaceExportService.pollExport(latest.id);
      } finally {
        busy.value = false;
      }
    }
  } catch {
    // Non-fatal: the card simply starts in its idle state.
  }
}

onMounted(refresh);
</script>
