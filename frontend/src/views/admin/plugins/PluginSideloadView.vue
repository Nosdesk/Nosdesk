<script setup lang="ts">
/**
 * Sideload a signed plugin zip directly. Secondary install path
 * for plugins that aren't in the registry yet (offline air-gapped
 * deployments, pre-publish testing, internal plugins). The
 * signed-bundle requirement is enforced server-side; the upload
 * fails closed if the signature doesn't resolve to a registered
 * publisher or the local signing key.
 *
 * The registry browse view is the recommended path; we link to
 * it prominently so admins don't sideload by default.
 */
import { onMounted, ref } from 'vue';
import { useRouter } from 'vue-router';

import AlertMessage from '@/components/common/AlertMessage.vue';
import pluginService from '@/services/pluginService';
import { usePluginAdminConfig } from '@/composables/usePluginAdminConfig';
import { logger } from '@/utils/logger';
import { formatFileSize } from '@/utils/formatFileSize';

const router = useRouter();

const file = ref<File | null>(null);
const dragOver = ref(false);
const installing = ref(false);
const errorMessage = ref('');
const fileInput = ref<HTMLInputElement | null>(null);

// Web sideload is opt-in. If the operator hasn't set
// NOSDESK_ALLOW_WEB_SIDELOAD, the upload endpoint will 403 anyway,
// so the view's only job is to bounce the admin back rather than
// presenting an upload affordance that can't succeed.
const { load: loadAdminConfig } = usePluginAdminConfig();
onMounted(async () => {
  const cfg = await loadAdminConfig();
  if (!cfg.web_sideload_enabled) {
    router.replace('/admin/plugins');
  }
});

function handleDragOver(e: DragEvent) {
  e.preventDefault();
  dragOver.value = true;
}

function handleDragLeave(e: DragEvent) {
  e.preventDefault();
  dragOver.value = false;
}

function handleDrop(e: DragEvent) {
  e.preventDefault();
  dragOver.value = false;
  const f = e.dataTransfer?.files?.[0];
  if (f) validateAndSet(f);
}

function handleFileSelect(e: Event) {
  const f = (e.target as HTMLInputElement).files?.[0];
  if (f) validateAndSet(f);
}

function validateAndSet(f: File) {
  errorMessage.value = '';
  if (!f.name.endsWith('.zip')) {
    errorMessage.value = 'Please select a .zip file';
    return;
  }
  if (f.size > 2 * 1024 * 1024) {
    errorMessage.value = 'File must be less than 2 MB';
    return;
  }
  file.value = f;
}

async function executeInstall() {
  if (!file.value) return;
  installing.value = true;
  errorMessage.value = '';
  try {
    const installed = await pluginService.installFromZip(file.value);
    logger.info('Plugin sideloaded', { name: installed.name });
    router.replace(`/admin/plugins/${installed.uuid}`);
  } catch (e: unknown) {
    const eo = e as { response?: { data?: { error?: string } } };
    errorMessage.value = eo.response?.data?.error ?? 'Failed to install plugin';
    logger.error('Sideload failed', { error: e });
  } finally {
    installing.value = false;
  }
}
</script>

<template>
  <div class="mx-auto flex w-full max-w-4xl flex-1 flex-col gap-4 px-4 py-4 sm:px-6">
    <RouterLink
      to="/admin/plugins"
      class="inline-flex items-center gap-1.5 text-sm text-secondary transition-colors hover:text-primary"
    >
      <svg
        xmlns="http://www.w3.org/2000/svg"
        class="h-4 w-4"
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
        stroke-width="2"
        aria-hidden="true"
      >
        <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
      </svg>
      Back to plugins
    </RouterLink>

    <header>
      <h1 class="text-xl font-bold text-primary sm:text-2xl">Sideload signed zip</h1>
      <p class="mt-1 text-sm text-secondary">
        For plugins that aren't in the registry yet. The bundle must be signed by a registered
        publisher or this instance's local signing key; unsigned uploads are refused. Looking for
        an official plugin? Browse the
        <RouterLink to="/admin/plugins/registry" class="text-accent hover:underline">
          registry
        </RouterLink>
        first.
      </p>
    </header>

    <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

    <!-- Drop zone -->
    <div
      role="button"
      tabindex="0"
      aria-label="Choose plugin zip file"
      :aria-busy="installing"
      class="rounded-xl border-2 border-dashed p-8 text-center transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
      :class="
        dragOver
          ? 'border-accent bg-accent/10'
          : file
            ? 'border-status-success bg-status-success/10'
            : 'border-default hover:border-strong'
      "
      @dragover="handleDragOver"
      @dragleave="handleDragLeave"
      @drop="handleDrop"
      @click="fileInput?.click()"
      @keydown.enter.prevent="fileInput?.click()"
      @keydown.space.prevent="fileInput?.click()"
    >
      <input
        ref="fileInput"
        type="file"
        accept=".zip"
        class="hidden"
        @change="handleFileSelect"
      />
      <div v-if="file" class="flex flex-col items-center gap-2">
        <svg class="h-10 w-10 text-status-success" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"
          />
        </svg>
        <p class="font-medium text-primary">{{ file.name }}</p>
        <p class="text-xs text-tertiary">{{ formatFileSize(file.size) }}</p>
        <button
          type="button"
          @click.stop="file = null"
          class="text-xs text-tertiary underline hover:text-secondary"
        >
          Choose a different file
        </button>
      </div>
      <div v-else class="flex flex-col items-center gap-2">
        <svg class="h-10 w-10 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M15 13l-3-3m0 0l-3 3m3-3v12"
          />
        </svg>
        <p class="font-medium text-secondary">Drop your plugin zip here</p>
        <p class="text-xs text-tertiary">or click to browse</p>
      </div>
    </div>

    <aside
      role="note"
      class="flex items-start gap-3 rounded-xl border border-status-warning/30 bg-status-warning/10 p-4 text-sm text-status-warning"
    >
      <svg
        xmlns="http://www.w3.org/2000/svg"
        class="mt-0.5 h-5 w-5 flex-shrink-0"
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
        stroke-width="2"
        aria-hidden="true"
      >
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          d="M12 9v2m0 4h.01M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"
        />
      </svg>
      <div class="flex flex-col gap-1">
        <p class="font-medium">Only sideload plugins from sources you trust.</p>
        <p class="text-status-warning/90">
          A signature confirms the bundle hasn't been tampered with after signing, but it doesn't
          vouch for the publisher's intent. An installed plugin runs in the admin UI with access
          to your session. Prefer the
          <RouterLink to="/admin/plugins/registry" class="underline hover:no-underline">
            registry
          </RouterLink>
          for vetted publishers, and review the source of anything you sideload.
        </p>
      </div>
    </aside>

    <div class="flex justify-end gap-2">
      <RouterLink
        to="/admin/plugins"
        class="px-4 py-2 text-sm text-secondary transition-colors hover:text-primary"
      >
        Cancel
      </RouterLink>
      <button
        type="button"
        :disabled="!file || installing"
        @click="executeInstall"
        class="flex items-center gap-2 rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-accent-hover disabled:opacity-50"
      >
        <svg
          v-if="installing"
          class="h-4 w-4 animate-spin"
          fill="none"
          viewBox="0 0 24 24"
          aria-hidden="true"
        >
          <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
          <path
            class="opacity-75"
            fill="currentColor"
            d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
          />
        </svg>
        {{ installing ? 'Installing...' : 'Install plugin' }}
      </button>
    </div>
  </div>
</template>
