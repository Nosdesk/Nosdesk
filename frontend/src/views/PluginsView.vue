<script setup lang="ts">
/**
 * Plugins Management View
 *
 * Admin interface for managing installed plugins.
 * Features: List, enable/disable, configure settings, view activity, install from zip.
 */
import { ref, onMounted } from 'vue';
import pluginService from '@/services/pluginService';
import BackButton from '@/components/common/BackButton.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Modal from '@/components/Modal.vue';
import { logger } from '@/utils/logger';
import type { Plugin, PluginSetting, PluginTrustLevel, PluginSource } from '@/types/plugin';

// State
const plugins = ref<Plugin[]>([]);
const isLoading = ref(true);
const errorMessage = ref('');
const successMessage = ref('');
const selectedPlugin = ref<Plugin | null>(null);
const showSettingsModal = ref(false);

// Settings modal state
const settings = ref<PluginSetting[]>([]);
const settingsLoading = ref(false);
const settingValues = ref<Record<string, unknown>>({});
const editingSecrets = ref<Set<string>>(new Set());

// Confirmation
const showUninstallConfirm = ref(false);
const uninstallTarget = ref<Plugin | null>(null);

// Bundle upload state
const bundleUploading = ref<string | null>(null);

// Install from zip state
const showInstallModal = ref(false);
const installFile = ref<File | null>(null);
const installDragOver = ref(false);
const installing = ref(false);
const installError = ref('');

// Load plugins on mount
onMounted(async () => {
  await loadPlugins();
});

async function loadPlugins() {
  isLoading.value = true;
  errorMessage.value = '';
  try {
    plugins.value = await pluginService.listPlugins();
  } catch (err) {
    errorMessage.value = 'Failed to load plugins';
    logger.error('Failed to load plugins', { error: err });
  } finally {
    isLoading.value = false;
  }
}

// Toggle plugin enabled state
async function togglePlugin(plugin: Plugin) {
  try {
    const updated = await pluginService.updatePlugin(plugin.uuid, {
      enabled: !plugin.enabled,
    });
    const index = plugins.value.findIndex(p => p.uuid === plugin.uuid);
    if (index !== -1) {
      plugins.value[index] = updated;
    }
    successMessage.value = `Plugin ${updated.enabled ? 'enabled' : 'disabled'}`;
    setTimeout(() => successMessage.value = '', 3000);
  } catch (err) {
    errorMessage.value = 'Failed to update plugin';
    logger.error('Failed to toggle plugin', { error: err, plugin: plugin.uuid });
  }
}

// Open settings modal
async function openSettings(plugin: Plugin) {
  selectedPlugin.value = plugin;
  showSettingsModal.value = true;
  settingsLoading.value = true;
  editingSecrets.value = new Set(); // Reset editing state

  try {
    settings.value = await pluginService.getPluginSettings(plugin.uuid);
    settingValues.value = {};
    for (const setting of settings.value) {
      settingValues.value[setting.key] = setting.value;
    }
    for (const def of plugin.manifest.settings) {
      if (!(def.key in settingValues.value)) {
        settingValues.value[def.key] = def.default;
      }
    }
  } catch (err) {
    logger.error('Failed to load plugin settings', { error: err });
  } finally {
    settingsLoading.value = false;
  }
}

// Save a setting
async function saveSetting(key: string) {
  if (!selectedPlugin.value) return;
  try {
    await pluginService.setPluginSetting(selectedPlugin.value.uuid, {
      key,
      value: settingValues.value[key],
    });
    // If this was a secret being edited, mark it as configured and stop editing
    if (editingSecrets.value.has(key) && settingValues.value[key]) {
      editingSecrets.value.delete(key);
      // Reload settings to get fresh state
      settings.value = await pluginService.getPluginSettings(selectedPlugin.value.uuid);
    }
  } catch (err) {
    logger.error('Failed to save setting', { error: err, key });
  }
}

// Check if a secret setting is configured (exists in backend but value is hidden)
function isSecretConfigured(key: string): boolean {
  const setting = settings.value.find(s => s.key === key);
  return setting?.is_secret === true;
}

// Start editing a secret
function editSecret(key: string) {
  editingSecrets.value.add(key);
  settingValues.value[key] = ''; // Clear the placeholder
}

// Cancel editing a secret
function cancelEditSecret(key: string) {
  editingSecrets.value.delete(key);
  settingValues.value[key] = null; // Reset to null (configured state)
}

// Confirm uninstall
function confirmUninstall(plugin: Plugin) {
  uninstallTarget.value = plugin;
  showUninstallConfirm.value = true;
}

// Execute uninstall
async function executeUninstall() {
  if (!uninstallTarget.value) return;
  try {
    await pluginService.uninstallPlugin(uninstallTarget.value.uuid);
    plugins.value = plugins.value.filter(p => p.uuid !== uninstallTarget.value?.uuid);
    showUninstallConfirm.value = false;
    successMessage.value = 'Plugin uninstalled successfully';
    setTimeout(() => successMessage.value = '', 3000);
    uninstallTarget.value = null;
  } catch (err) {
    errorMessage.value = 'Failed to uninstall plugin';
    logger.error('Failed to uninstall plugin', { error: err });
  }
}

// Handle bundle file selection
async function handleBundleUpload(plugin: Plugin, event: Event) {
  const input = event.target as HTMLInputElement;
  const file = input.files?.[0];
  if (!file) return;

  if (!file.name.endsWith('.js')) {
    errorMessage.value = 'Bundle must be a JavaScript file (.js)';
    return;
  }

  if (file.size > 500 * 1024) {
    errorMessage.value = 'Bundle must be less than 500KB';
    return;
  }

  bundleUploading.value = plugin.uuid;
  errorMessage.value = '';

  try {
    const updated = await pluginService.uploadBundle(plugin.uuid, file);
    const index = plugins.value.findIndex(p => p.uuid === plugin.uuid);
    if (index !== -1) {
      plugins.value[index] = updated;
    }
    successMessage.value = 'Bundle uploaded successfully';
    setTimeout(() => successMessage.value = '', 3000);
    logger.info('Plugin bundle uploaded', { uuid: plugin.uuid, size: file.size });
  } catch (err) {
    errorMessage.value = 'Failed to upload bundle';
    logger.error('Failed to upload bundle', { error: err, plugin: plugin.uuid });
  } finally {
    bundleUploading.value = null;
    input.value = '';
  }
}

// Check if plugin can have bundles (only official/verified)
function canUploadBundle(plugin: Plugin): boolean {
  return plugin.trust_level === 'official' || plugin.trust_level === 'verified';
}

// Format file size
function formatFileSize(bytes: number | null): string {
  if (!bytes) return '';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

// Trust level badge
function getTrustLevelBadge(level: PluginTrustLevel): { label: string; class: string } {
  switch (level) {
    case 'official':
      return { label: 'Official', class: 'bg-status-success/10 text-status-success' };
    case 'verified':
      return { label: 'Verified', class: 'bg-accent/10 text-accent' };
    case 'community':
      return { label: 'Community', class: 'bg-surface-alt text-secondary' };
    default:
      return { label: level, class: 'bg-surface-alt text-secondary' };
  }
}

// Source badge styling
function getSourceBadge(source: PluginSource): { label: string; class: string } {
  switch (source) {
    case 'provisioned':
      return { label: 'Provisioned', class: 'bg-status-info/10 text-status-info' };
    case 'uploaded':
      return { label: 'Uploaded', class: 'bg-tertiary/10 text-tertiary' };
    default:
      return { label: 'Unknown', class: 'bg-surface-alt text-secondary' };
  }
}

// Determine plugin icon type
function getPluginIconType(icon?: string): 'image' | 'emoji' | 'none' {
  if (!icon) return 'none';
  // URLs and data URIs are images
  if (icon.startsWith('http://') || icon.startsWith('https://') || icon.startsWith('/') || icon.startsWith('data:')) {
    return 'image';
  }
  // Short strings (1-4 chars) are likely emojis
  if (icon.length <= 4) {
    return 'emoji';
  }
  // Fallback to none for unrecognized formats
  return 'none';
}

// Format date
function formatDate(date: string): string {
  return new Date(date).toLocaleDateString('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  });
}

// Install modal handlers
function openInstallModal() {
  showInstallModal.value = true;
  installFile.value = null;
  installError.value = '';
}

function closeInstallModal() {
  showInstallModal.value = false;
  installFile.value = null;
  installError.value = '';
  installDragOver.value = false;
}

function handleInstallDragOver(e: DragEvent) {
  e.preventDefault();
  installDragOver.value = true;
}

function handleInstallDragLeave(e: DragEvent) {
  e.preventDefault();
  installDragOver.value = false;
}

function handleInstallDrop(e: DragEvent) {
  e.preventDefault();
  installDragOver.value = false;
  const files = e.dataTransfer?.files;
  if (files && files.length > 0) {
    validateAndSetInstallFile(files[0]);
  }
}

function handleInstallFileSelect(e: Event) {
  const input = e.target as HTMLInputElement;
  const file = input.files?.[0];
  if (file) {
    validateAndSetInstallFile(file);
  }
}

function validateAndSetInstallFile(file: File) {
  installError.value = '';

  if (!file.name.endsWith('.zip')) {
    installError.value = 'Please select a .zip file';
    return;
  }

  if (file.size > 2 * 1024 * 1024) {
    installError.value = 'File must be less than 2MB';
    return;
  }

  installFile.value = file;
}

async function executeInstall() {
  if (!installFile.value) return;

  installing.value = true;
  installError.value = '';

  try {
    const newPlugin = await pluginService.installFromZip(installFile.value);
    plugins.value.push(newPlugin);
    closeInstallModal();
    successMessage.value = `Plugin "${newPlugin.display_name}" installed successfully`;
    setTimeout(() => successMessage.value = '', 3000);
    logger.info('Plugin installed from zip', { name: newPlugin.name });
  } catch (err: unknown) {
    const errorObj = err as { response?: { data?: { error?: string } } };
    installError.value = errorObj.response?.data?.error || 'Failed to install plugin';
    logger.error('Failed to install plugin from zip', { error: err });
  } finally {
    installing.value = false;
  }
}
</script>

<template>
  <div class="flex-1">
    <!-- Navigation and actions bar -->
    <div class="pt-4 px-4 sm:px-6 flex flex-col sm:flex-row justify-between items-start sm:items-center gap-3 sm:gap-4">
      <BackButton fallbackRoute="/admin" label="Back to Administration" />
      <button
        @click="openInstallModal"
        class="px-3 py-1.5 bg-accent text-white rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors flex items-center gap-1.5"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6m0 0v6m0-6h6m-6 0H6" />
        </svg>
        <span class="hidden xs:inline">Install Plugin</span>
        <span class="xs:hidden">Install</span>
      </button>
    </div>

    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-2">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">Plugins</h1>
        <p class="text-secondary text-sm sm:text-base mt-1">Manage installed plugins and integrations</p>
      </div>

      <!-- Success message -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

      <!-- Error message -->
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Loading state -->
      <LoadingSpinner v-if="isLoading" text="Loading plugins..." />

      <!-- Plugin list -->
      <div v-else class="flex flex-col gap-4">
        <!-- Plugins -->
        <div v-if="plugins.length > 0" class="flex flex-col gap-2 sm:gap-3">
          <div
            v-for="plugin in plugins"
            :key="plugin.uuid"
            class="bg-surface border border-default rounded-xl overflow-hidden"
          >
            <!-- Main content -->
            <div class="p-4">
              <div class="flex items-start gap-3">
                <!-- Icon -->
                <div class="w-10 h-10 rounded-lg bg-accent/10 flex items-center justify-center flex-shrink-0 overflow-hidden">
                  <!-- Custom image icon -->
                  <img
                    v-if="getPluginIconType(plugin.manifest.icon) === 'image'"
                    :src="plugin.manifest.icon"
                    :alt="plugin.display_name"
                    class="w-full h-full object-cover"
                  />
                  <!-- Emoji icon -->
                  <span
                    v-else-if="getPluginIconType(plugin.manifest.icon) === 'emoji'"
                    class="text-xl"
                  >
                    {{ plugin.manifest.icon }}
                  </span>
                  <!-- Default plugin icon -->
                  <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-accent" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M11 4a2 2 0 114 0v1a1 1 0 001 1h3a1 1 0 011 1v3a1 1 0 01-1 1h-1a2 2 0 100 4h1a1 1 0 011 1v3a1 1 0 01-1 1h-3a1 1 0 01-1-1v-1a2 2 0 10-4 0v1a1 1 0 01-1 1H7a1 1 0 01-1-1v-3a1 1 0 00-1-1H4a2 2 0 110-4h1a1 1 0 001-1V7a1 1 0 011-1h3a1 1 0 001-1V4z" />
                  </svg>
                </div>

                <!-- Content -->
                <div class="flex-1 min-w-0">
                  <div class="flex flex-wrap items-center gap-1.5 sm:gap-2">
                    <h3 class="font-semibold text-primary">{{ plugin.display_name }}</h3>
                    <code class="px-1.5 py-0.5 text-xs bg-surface-alt text-secondary rounded font-mono">v{{ plugin.version }}</code>
                    <span
                      class="px-1.5 py-0.5 text-xs rounded font-medium"
                      :class="getTrustLevelBadge(plugin.trust_level).class"
                    >
                      {{ getTrustLevelBadge(plugin.trust_level).label }}
                    </span>
                    <span
                      class="px-1.5 py-0.5 text-xs rounded font-medium"
                      :class="getSourceBadge(plugin.source).class"
                    >
                      {{ getSourceBadge(plugin.source).label }}
                    </span>
                    <span
                      v-if="!plugin.enabled"
                      class="px-1.5 py-0.5 text-xs rounded font-medium bg-status-warning/10 text-status-warning"
                    >
                      Disabled
                    </span>
                  </div>
                  <p v-if="plugin.description" class="text-sm text-secondary mt-1.5 line-clamp-2">
                    {{ plugin.description }}
                  </p>
                  <div class="flex flex-wrap items-center gap-x-1.5 gap-y-1 mt-2 text-xs text-tertiary">
                    <code class="font-mono bg-surface-alt px-1.5 py-0.5 rounded">{{ plugin.name }}</code>
                    <span class="text-border">·</span>
                    <span>Installed {{ formatDate(plugin.installed_at) }}</span>
                    <template v-if="plugin.manifest.permissions.length">
                      <span class="text-border">·</span>
                      <span>{{ plugin.manifest.permissions.length }} permissions</span>
                    </template>
                  </div>
                </div>

                <!-- Desktop actions (hidden on mobile/tablet) -->
                <div class="hidden lg:flex items-center gap-1 flex-shrink-0">
                  <button
                    @click="togglePlugin(plugin)"
                    class="relative inline-flex h-5 w-9 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus:ring-2 focus:ring-accent focus:ring-offset-2 focus:ring-offset-surface"
                    :class="plugin.enabled ? 'bg-accent' : 'bg-border'"
                    :title="plugin.enabled ? 'Disable plugin' : 'Enable plugin'"
                  >
                    <span
                      class="pointer-events-none inline-block h-4 w-4 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out"
                      :class="plugin.enabled ? 'translate-x-4' : 'translate-x-0'"
                    ></span>
                  </button>
                  <button
                    v-if="plugin.manifest.settings.length > 0"
                    @click="openSettings(plugin)"
                    class="p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
                    title="Settings"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                      <path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                    </svg>
                  </button>
                  <a
                    v-if="plugin.manifest.repository"
                    :href="plugin.manifest.repository"
                    target="_blank"
                    rel="noopener noreferrer"
                    class="p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
                    title="View source"
                    @click.stop
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
                    </svg>
                  </a>
                  <button
                    @click="confirmUninstall(plugin)"
                    class="p-2 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-lg transition-colors"
                    title="Uninstall"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                    </svg>
                  </button>
                </div>
              </div>
            </div>

            <!-- Mobile/tablet actions footer (hidden on desktop) -->
            <div class="lg:hidden px-4 py-2 bg-surface-alt border-t border-default flex items-center justify-between">
              <div class="flex items-center gap-1">
                <button
                  v-if="plugin.manifest.settings.length > 0"
                  @click="openSettings(plugin)"
                  class="p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
                  title="Settings"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                    <path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                  </svg>
                </button>
                <a
                  v-if="plugin.manifest.repository"
                  :href="plugin.manifest.repository"
                  target="_blank"
                  rel="noopener noreferrer"
                  class="p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
                  title="View source"
                  @click.stop
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
                  </svg>
                </a>
                <button
                  @click="confirmUninstall(plugin)"
                  class="p-2 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-lg transition-colors"
                  title="Uninstall"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                  </svg>
                </button>
              </div>
              <div class="flex items-center gap-2">
                <span class="text-xs text-tertiary">{{ plugin.enabled ? 'Enabled' : 'Disabled' }}</span>
                <button
                  @click="togglePlugin(plugin)"
                  class="relative inline-flex h-5 w-9 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus:ring-2 focus:ring-accent focus:ring-offset-2 focus:ring-offset-surface"
                  :class="plugin.enabled ? 'bg-accent' : 'bg-border'"
                  :title="plugin.enabled ? 'Disable plugin' : 'Enable plugin'"
                >
                  <span
                    class="pointer-events-none inline-block h-4 w-4 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out"
                    :class="plugin.enabled ? 'translate-x-4' : 'translate-x-0'"
                  ></span>
                </button>
              </div>
            </div>
          </div>
        </div>

        <!-- Empty state -->
        <EmptyState
          v-if="plugins.length === 0 && !isLoading"
          icon="folder"
          title="No plugins installed"
          description="Plugins extend Nosdesk with custom integrations and features. Install plugins from zip files or provision them via Docker volume mounts."
          action-label="Install Plugin"
          variant="card"
          @action="openInstallModal"
        />
      </div>
    </div>

    <!-- Install Plugin Modal -->
    <Modal
      :show="showInstallModal"
      title="Install Plugin"
      size="sm"
      @close="closeInstallModal"
    >
      <div class="flex flex-col gap-4">
        <p class="text-sm text-secondary">
          Upload a plugin zip file containing a <code class="text-primary bg-surface-alt px-1 rounded">manifest.json</code> and optionally a <code class="text-primary bg-surface-alt px-1 rounded">bundle.js</code>.
        </p>

        <!-- Drop zone -->
        <div
          @dragover="handleInstallDragOver"
          @dragleave="handleInstallDragLeave"
          @drop="handleInstallDrop"
          :class="[
            'border-2 border-dashed rounded-xl p-8 text-center transition-colors cursor-pointer',
            installDragOver
              ? 'border-accent bg-accent/10'
              : installFile
                ? 'border-status-success bg-status-success/10'
                : 'border-default hover:border-strong'
          ]"
          @click="($refs.fileInput as HTMLInputElement)?.click()"
        >
          <input
            ref="fileInput"
            type="file"
            accept=".zip"
            class="hidden"
            @change="handleInstallFileSelect"
          />

          <div v-if="installFile" class="flex flex-col items-center gap-2">
            <svg class="w-10 h-10 text-status-success" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <p class="text-primary font-medium">{{ installFile.name }}</p>
            <p class="text-xs text-tertiary">{{ formatFileSize(installFile.size) }}</p>
            <button
              @click.stop="installFile = null"
              class="text-xs text-tertiary hover:text-secondary underline"
            >
              Choose a different file
            </button>
          </div>
          <div v-else class="flex flex-col items-center gap-2">
            <svg class="w-10 h-10 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M15 13l-3-3m0 0l-3 3m3-3v12" />
            </svg>
            <p class="text-secondary font-medium">Drop your plugin zip here</p>
            <p class="text-xs text-tertiary">or click to browse</p>
          </div>
        </div>

        <!-- Error -->
        <AlertMessage v-if="installError" type="error" :message="installError" />

        <!-- Info box -->
        <div class="p-3 bg-surface-alt border border-default rounded-xl text-xs text-tertiary">
          <p class="font-medium text-secondary mb-2">Plugin Zip Structure</p>
          <div class="flex flex-col gap-1 font-mono text-secondary">
            <span>my-plugin.zip</span>
            <span class="pl-3">├── manifest.json <span class="text-tertiary">(required)</span></span>
            <span class="pl-3">└── bundle.js <span class="text-tertiary">(optional)</span></span>
          </div>
        </div>

        <!-- Actions -->
        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="closeInstallModal"
            class="px-4 py-2 text-sm text-secondary hover:text-primary transition-colors"
          >
            Cancel
          </button>
          <button
            @click="executeInstall"
            :disabled="!installFile || installing"
            class="px-4 py-2 bg-accent text-white rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors disabled:opacity-50 flex items-center gap-2"
          >
            <svg v-if="installing" class="w-4 h-4 animate-spin" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
            {{ installing ? 'Installing...' : 'Install Plugin' }}
          </button>
        </div>
      </div>
    </Modal>

    <!-- Settings Modal -->
    <Modal
      :show="showSettingsModal && selectedPlugin !== null"
      :title="`${selectedPlugin?.display_name} Settings`"
      size="sm"
      @close="showSettingsModal = false"
    >
      <LoadingSpinner v-if="settingsLoading" text="Loading settings..." />
      <div v-else-if="!selectedPlugin || selectedPlugin.manifest.settings.length === 0" class="text-center py-8 text-tertiary">
        No settings available
      </div>
      <div v-else class="flex flex-col gap-5">
        <div v-for="def in selectedPlugin.manifest.settings" :key="def.key" class="flex flex-col gap-2">
          <div>
            <label class="block text-sm font-medium text-primary">
              {{ def.label }}
              <span v-if="def.required" class="text-status-error">*</span>
            </label>
            <p v-if="def.description" class="text-xs text-tertiary mt-1">{{ def.description }}</p>
          </div>

          <!-- String input -->
          <input
            v-if="def.type === 'string'"
            v-model="settingValues[def.key]"
            type="text"
            @blur="saveSetting(def.key)"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
          />

          <!-- Secret input with configured state -->
          <div v-else-if="def.type === 'secret'">
            <!-- Configured state - show indicator with update option -->
            <div
              v-if="isSecretConfigured(def.key) && !editingSecrets.has(def.key)"
              class="flex items-center gap-3"
            >
              <div class="flex-1 flex items-center gap-2 px-3 py-2 bg-surface-alt border border-default rounded-lg">
                <svg class="w-4 h-4 text-status-success flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z" />
                </svg>
                <span class="text-sm text-secondary">Configured</span>
              </div>
              <button
                type="button"
                @click="editSecret(def.key)"
                class="px-3 py-2 text-sm text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
              >
                Update
              </button>
            </div>
            <!-- Not configured or editing - show password input -->
            <div v-else class="flex items-center gap-2">
              <input
                v-model="settingValues[def.key]"
                type="password"
                :placeholder="editingSecrets.has(def.key) ? 'Enter new value' : 'Enter value'"
                @blur="saveSetting(def.key)"
                class="flex-1 px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
              />
              <button
                v-if="editingSecrets.has(def.key)"
                type="button"
                @click="cancelEditSecret(def.key)"
                class="px-3 py-2 text-sm text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
              >
                Cancel
              </button>
            </div>
          </div>

          <!-- Number input -->
          <input
            v-else-if="def.type === 'number'"
            v-model.number="settingValues[def.key]"
            type="number"
            @blur="saveSetting(def.key)"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
          />

          <!-- Boolean toggle -->
          <label v-else-if="def.type === 'boolean'" class="flex items-center gap-2">
            <input
              v-model="settingValues[def.key]"
              type="checkbox"
              @change="saveSetting(def.key)"
              class="rounded border-default text-accent focus:ring-accent"
            />
            <span class="text-sm text-secondary">Enabled</span>
          </label>

          <!-- Select -->
          <select
            v-else-if="def.type === 'select' && def.options"
            v-model="settingValues[def.key]"
            @change="saveSetting(def.key)"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
          >
            <option v-for="opt in def.options" :key="opt.value" :value="opt.value">
              {{ opt.label }}
            </option>
          </select>
        </div>
      </div>
    </Modal>

    <!-- Confirm Uninstall Modal -->
    <Modal
      :show="showUninstallConfirm && uninstallTarget !== null"
      title="Uninstall Plugin"
      size="sm"
      @close="showUninstallConfirm = false"
    >
      <div class="flex flex-col gap-4">
        <p class="text-secondary">
          Are you sure you want to uninstall <strong class="text-primary">{{ uninstallTarget?.display_name }}</strong>?
          This will remove all plugin data and cannot be undone.
        </p>
        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="showUninstallConfirm = false"
            class="px-4 py-2 text-sm text-secondary hover:text-primary transition-colors"
          >
            Cancel
          </button>
          <button
            @click="executeUninstall"
            class="px-4 py-2 bg-status-error text-white rounded-lg text-sm hover:bg-status-error/90 font-medium transition-colors"
          >
            Uninstall
          </button>
        </div>
      </div>
    </Modal>
  </div>
</template>
