<script setup lang="ts">
/**
 * Visual primitive for a plugin row. The card renders identity
 * (icon, name, version, badges, description, metadata) and
 * delegates the trailing controls + optional footer to slots,
 * because the installed-list view and the registry-browse view
 * want different action sets and forcing them through a single
 * props-driven `<PluginActions>` would just shift the complexity
 * into prop wiring.
 *
 * Consumers:
 *   <PluginCard :plugin="plugin">
 *     <template #actions> ...buttons... </template>
 *     <template #footer> ...mobile actions row... </template>
 *   </PluginCard>
 */
import type { DeepReadonly } from 'vue';
import type { Plugin } from '@/types/plugin';
import PluginIcon from './PluginIcon.vue';
import PluginStateBadge from './PluginStateBadge.vue';
import PluginTrustBadge from './PluginTrustBadge.vue';

interface Props {
  /// Read-only because callers pass `readonly(plugins)` from
  /// usePlugins; the card never mutates the row.
  plugin: Plugin | DeepReadonly<Plugin>;
  /**
   * When true, the state badge always renders. When false (default)
   * it only renders for non-installed states, so an Active plugin
   * doesn't get a redundant green pill next to its trust badge.
   */
  showStateAlways?: boolean;
}

const { plugin, showStateAlways = false } = defineProps<Props>();

function formatDate(iso: string): string {
  return new Date(iso).toLocaleDateString('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  });
}
</script>

<template>
  <article class="overflow-hidden rounded-xl border border-default bg-surface">
    <div class="p-4">
      <div class="flex items-start gap-3">
        <PluginIcon :uuid="plugin.uuid" :alt="plugin.display_name" />

        <div class="min-w-0 flex-1">
          <header class="flex flex-wrap items-center gap-1.5 sm:gap-2">
            <h3 class="font-semibold text-primary">{{ plugin.display_name }}</h3>
            <code class="rounded bg-surface-alt px-1.5 py-0.5 font-mono text-xs text-secondary">
              v{{ plugin.version }}
            </code>
            <PluginTrustBadge :level="plugin.trust_level" />
            <PluginStateBadge
              v-if="showStateAlways || plugin.state !== 'installed'"
              :state="plugin.state"
            />
          </header>

          <p v-if="plugin.description" class="mt-1.5 line-clamp-2 text-sm text-secondary">
            {{ plugin.description }}
          </p>

          <dl class="mt-2 flex flex-wrap items-center gap-x-1.5 gap-y-1 text-xs text-tertiary">
            <dt class="sr-only">Plugin name</dt>
            <dd>
              <code class="rounded bg-surface-alt px-1.5 py-0.5 font-mono">{{ plugin.name }}</code>
            </dd>
            <span aria-hidden="true" class="text-border">·</span>
            <dt class="sr-only">Installed</dt>
            <dd>Installed {{ formatDate(plugin.installed_at) }}</dd>
            <template v-if="plugin.manifest.permissions.length">
              <span aria-hidden="true" class="text-border">·</span>
              <dt class="sr-only">Permission count</dt>
              <dd>{{ plugin.manifest.permissions.length }} permissions</dd>
            </template>
          </dl>
        </div>

        <div v-if="$slots.actions" class="flex flex-shrink-0 items-center gap-1">
          <slot name="actions" />
        </div>
      </div>
    </div>

    <footer
      v-if="$slots.footer"
      class="flex items-center justify-between border-t border-default bg-surface-alt px-4 py-2"
    >
      <slot name="footer" />
    </footer>
  </article>
</template>
