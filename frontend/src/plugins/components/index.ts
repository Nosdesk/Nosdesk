/**
 * Plugin Component Library
 *
 * Shared components available to plugins for building consistent UIs.
 * These components use Nosdesk's design system for a cohesive look.
 */

// Layout components
export { default as PluginPanel } from './PluginPanel.vue';
export { default as PluginSlot } from './PluginSlot.vue';

// Loading states
export { default as PluginLoading } from './PluginLoading.vue';
export { default as PluginError } from './PluginError.vue';

// Re-export common components from the main app that plugins can use
// Note: These will be expanded as needed
