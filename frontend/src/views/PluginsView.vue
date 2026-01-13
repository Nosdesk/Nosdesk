<script setup lang="ts">
/**
 * Plugins Management View
 *
 * Admin interface for managing installed plugins.
 * Features: List, enable/disable, configure settings, view activity.
 */
import { ref, computed, onMounted } from 'vue';
import pluginService from '@/services/pluginService';
import EmptyState from '@/components/common/EmptyState.vue';
import { logger } from '@/utils/logger';
import type { Plugin, PluginSetting, PluginActivity, PluginTrustLevel } from '@/types/plugin';

// State
const plugins = ref<Plugin[]>([]);
const isLoading = ref(true);
const error = ref<string | null>(null);
const selectedPlugin = ref<Plugin | null>(null);
const showSettingsModal = ref(false);
const showActivityModal = ref(false);

// Settings modal state
const settings = ref<PluginSetting[]>([]);
const settingsLoading = ref(false);
const settingValues = ref<Record<string, unknown>>({});

// Activity modal state
const activity = ref<PluginActivity[]>([]);
const activityLoading = ref(false);

// Confirmation
const confirmUninstall = ref(false);
const uninstallTarget = ref<Plugin | null>(null);

// Load plugins on mount
onMounted(async () => {
  await loadPlugins();
});

async function loadPlugins() {
  isLoading.value = true;
  error.value = null;
  try {
    plugins.value = await pluginService.listPlugins();
  } catch (err) {
    error.value = 'Failed to load plugins';
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
    // Update in list
    const index = plugins.value.findIndex(p => p.uuid === plugin.uuid);
    if (index !== -1) {
      plugins.value[index] = updated;
    }
  } catch (err) {
    error.value = 'Failed to update plugin';
    logger.error('Failed to toggle plugin', { error: err, plugin: plugin.uuid });
  }
}

// Open settings modal
async function openSettings(plugin: Plugin) {
  selectedPlugin.value = plugin;
  showSettingsModal.value = true;
  settingsLoading.value = true;

  try {
    settings.value = await pluginService.getPluginSettings(plugin.uuid);
    // Initialize setting values
    settingValues.value = {};
    for (const setting of settings.value) {
      settingValues.value[setting.key] = setting.value;
    }
    // Add default values from manifest for settings not yet saved
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
  } catch (err) {
    logger.error('Failed to save setting', { error: err, key });
  }
}

// Open activity modal
async function openActivity(plugin: Plugin) {
  selectedPlugin.value = plugin;
  showActivityModal.value = true;
  activityLoading.value = true;

  try {
    activity.value = await pluginService.getPluginActivity(plugin.uuid);
  } catch (err) {
    logger.error('Failed to load plugin activity', { error: err });
  } finally {
    activityLoading.value = false;
  }
}

// Confirm uninstall
function promptUninstall(plugin: Plugin) {
  uninstallTarget.value = plugin;
  confirmUninstall.value = true;
}

// Execute uninstall
async function executeUninstall() {
  if (!uninstallTarget.value) return;
  try {
    await pluginService.uninstallPlugin(uninstallTarget.value.uuid);
    plugins.value = plugins.value.filter(p => p.uuid !== uninstallTarget.value?.uuid);
    confirmUninstall.value = false;
    uninstallTarget.value = null;
  } catch (err) {
    error.value = 'Failed to uninstall plugin';
    logger.error('Failed to uninstall plugin', { error: err });
  }
}

// Trust level badge color
function getTrustLevelColor(level: PluginTrustLevel): string {
  switch (level) {
    case 'official':
      return 'bg-green-500/20 text-green-400';
    case 'verified':
      return 'bg-blue-500/20 text-blue-400';
    case 'community':
      return 'bg-gray-500/20 text-gray-400';
    default:
      return 'bg-gray-500/20 text-gray-400';
  }
}

// Format date
function formatDate(date: string): string {
  return new Date(date).toLocaleDateString('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}
</script>

<template>
  <div class="p-6 max-w-6xl mx-auto">
    <!-- Header -->
    <div class="mb-6">
      <h1 class="text-2xl font-semibold text-primary">Plugins</h1>
      <p class="text-tertiary mt-1">
        Manage installed plugins and integrations
      </p>
    </div>

    <!-- Error State -->
    <div v-if="error" class="mb-4 p-4 bg-red-500/10 border border-red-500/50 rounded-lg text-red-400">
      {{ error }}
      <button @click="loadPlugins" class="ml-2 underline hover:no-underline">Retry</button>
    </div>

    <!-- Loading State -->
    <div v-if="isLoading" class="flex items-center justify-center py-12">
      <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-accent"></div>
    </div>

    <!-- Empty State -->
    <EmptyState
      v-else-if="plugins.length === 0"
      icon="folder"
      title="No plugins installed"
      description="Plugins extend Nosdesk with custom integrations and features. Install plugins from the marketplace or upload custom plugins."
    />

    <!-- Plugin List -->
    <div v-else class="space-y-4">
      <div
        v-for="plugin in plugins"
        :key="plugin.uuid"
        class="bg-surface border border-border rounded-lg p-4 hover:border-border-hover transition-colors"
      >
        <div class="flex items-start justify-between">
          <!-- Plugin Info -->
          <div class="flex-1">
            <div class="flex items-center gap-3">
              <h3 class="font-medium text-primary">{{ plugin.display_name }}</h3>
              <span class="text-xs text-tertiary">v{{ plugin.version }}</span>
              <span
                class="px-2 py-0.5 rounded-full text-xs font-medium"
                :class="getTrustLevelColor(plugin.trust_level)"
              >
                {{ plugin.trust_level }}
              </span>
              <span
                v-if="!plugin.enabled"
                class="px-2 py-0.5 rounded-full text-xs font-medium bg-yellow-500/20 text-yellow-400"
              >
                Disabled
              </span>
            </div>
            <p v-if="plugin.description" class="text-sm text-tertiary mt-1">
              {{ plugin.description }}
            </p>
            <div class="flex items-center gap-4 mt-2 text-xs text-tertiary">
              <span>{{ plugin.name }}</span>
              <span>Installed {{ formatDate(plugin.installed_at) }}</span>
              <span v-if="plugin.manifest.permissions.length">
                {{ plugin.manifest.permissions.length }} permissions
              </span>
            </div>
          </div>

          <!-- Actions -->
          <div class="flex items-center gap-2">
            <!-- Enable/Disable Toggle -->
            <button
              @click="togglePlugin(plugin)"
              class="relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus:ring-2 focus:ring-accent focus:ring-offset-2 focus:ring-offset-surface"
              :class="plugin.enabled ? 'bg-accent' : 'bg-gray-600'"
              :title="plugin.enabled ? 'Disable plugin' : 'Enable plugin'"
            >
              <span
                class="pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out"
                :class="plugin.enabled ? 'translate-x-5' : 'translate-x-0'"
              ></span>
            </button>

            <!-- Settings -->
            <button
              v-if="plugin.manifest.settings.length > 0"
              @click="openSettings(plugin)"
              class="p-2 rounded-lg hover:bg-hover text-tertiary hover:text-primary transition-colors"
              title="Settings"
            >
              <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
              </svg>
            </button>

            <!-- Activity -->
            <button
              @click="openActivity(plugin)"
              class="p-2 rounded-lg hover:bg-hover text-tertiary hover:text-primary transition-colors"
              title="Activity log"
            >
              <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
            </button>

            <!-- Uninstall -->
            <button
              @click="promptUninstall(plugin)"
              class="p-2 rounded-lg hover:bg-red-500/20 text-tertiary hover:text-red-400 transition-colors"
              title="Uninstall"
            >
              <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
              </svg>
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Settings Modal -->
    <div
      v-if="showSettingsModal && selectedPlugin"
      class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
      @click.self="showSettingsModal = false"
    >
      <div class="bg-surface border border-border rounded-lg w-full max-w-lg max-h-[80vh] overflow-hidden">
        <div class="flex items-center justify-between px-4 py-3 border-b border-border">
          <h2 class="font-semibold text-primary">{{ selectedPlugin.display_name }} Settings</h2>
          <button
            @click="showSettingsModal = false"
            class="p-1 rounded hover:bg-hover text-tertiary"
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div class="p-4 overflow-y-auto max-h-96">
          <div v-if="settingsLoading" class="flex justify-center py-8">
            <div class="animate-spin rounded-full h-6 w-6 border-b-2 border-accent"></div>
          </div>
          <div v-else-if="selectedPlugin.manifest.settings.length === 0" class="text-center py-8 text-tertiary">
            No settings available
          </div>
          <div v-else class="space-y-4">
            <div v-for="def in selectedPlugin.manifest.settings" :key="def.key">
              <label class="block text-sm font-medium text-secondary mb-1">
                {{ def.label }}
                <span v-if="def.required" class="text-red-400">*</span>
              </label>
              <p v-if="def.description" class="text-xs text-tertiary mb-2">{{ def.description }}</p>

              <!-- String input -->
              <input
                v-if="def.type === 'string' || def.type === 'secret'"
                v-model="settingValues[def.key]"
                :type="def.type === 'secret' ? 'password' : 'text'"
                @blur="saveSetting(def.key)"
                class="w-full px-3 py-2 bg-surface-alt border border-border rounded-lg text-primary focus:ring-2 focus:ring-accent focus:border-accent"
              />

              <!-- Number input -->
              <input
                v-else-if="def.type === 'number'"
                v-model.number="settingValues[def.key]"
                type="number"
                @blur="saveSetting(def.key)"
                class="w-full px-3 py-2 bg-surface-alt border border-border rounded-lg text-primary focus:ring-2 focus:ring-accent focus:border-accent"
              />

              <!-- Boolean toggle -->
              <label v-else-if="def.type === 'boolean'" class="flex items-center gap-2">
                <input
                  v-model="settingValues[def.key]"
                  type="checkbox"
                  @change="saveSetting(def.key)"
                  class="rounded border-border text-accent focus:ring-accent"
                />
                <span class="text-sm text-secondary">Enabled</span>
              </label>

              <!-- Select -->
              <select
                v-else-if="def.type === 'select' && def.options"
                v-model="settingValues[def.key]"
                @change="saveSetting(def.key)"
                class="w-full px-3 py-2 bg-surface-alt border border-border rounded-lg text-primary focus:ring-2 focus:ring-accent focus:border-accent"
              >
                <option v-for="opt in def.options" :key="opt.value" :value="opt.value">
                  {{ opt.label }}
                </option>
              </select>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Activity Modal -->
    <div
      v-if="showActivityModal && selectedPlugin"
      class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
      @click.self="showActivityModal = false"
    >
      <div class="bg-surface border border-border rounded-lg w-full max-w-lg max-h-[80vh] overflow-hidden">
        <div class="flex items-center justify-between px-4 py-3 border-b border-border">
          <h2 class="font-semibold text-primary">{{ selectedPlugin.display_name }} Activity</h2>
          <button
            @click="showActivityModal = false"
            class="p-1 rounded hover:bg-hover text-tertiary"
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div class="p-4 overflow-y-auto max-h-96">
          <div v-if="activityLoading" class="flex justify-center py-8">
            <div class="animate-spin rounded-full h-6 w-6 border-b-2 border-accent"></div>
          </div>
          <div v-else-if="activity.length === 0" class="text-center py-8 text-tertiary">
            No activity yet
          </div>
          <div v-else class="space-y-3">
            <div
              v-for="item in activity"
              :key="item.uuid"
              class="p-3 bg-surface-alt rounded-lg text-sm"
            >
              <div class="flex items-center justify-between">
                <span class="font-medium text-primary">{{ item.action }}</span>
                <span class="text-xs text-tertiary">{{ formatDate(item.created_at) }}</span>
              </div>
              <pre v-if="item.details" class="mt-2 text-xs text-tertiary overflow-x-auto">{{ JSON.stringify(item.details, null, 2) }}</pre>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Confirm Uninstall Modal -->
    <div
      v-if="confirmUninstall && uninstallTarget"
      class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
      @click.self="confirmUninstall = false"
    >
      <div class="bg-surface border border-border rounded-lg p-6 w-full max-w-md">
        <h2 class="text-lg font-semibold text-primary mb-2">Uninstall Plugin?</h2>
        <p class="text-tertiary mb-4">
          Are you sure you want to uninstall <strong>{{ uninstallTarget.display_name }}</strong>?
          This will remove all plugin data and cannot be undone.
        </p>
        <div class="flex justify-end gap-3">
          <button
            @click="confirmUninstall = false"
            class="px-4 py-2 rounded-lg bg-surface-alt hover:bg-hover text-secondary transition-colors"
          >
            Cancel
          </button>
          <button
            @click="executeUninstall"
            class="px-4 py-2 rounded-lg bg-red-500 hover:bg-red-600 text-white transition-colors"
          >
            Uninstall
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
