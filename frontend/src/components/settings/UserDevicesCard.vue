<script setup lang="ts">
/**
 * A user's assigned devices, rendered in the app's canonical dense-list
 * vocabulary (SectionCard + divide-y rows), mirroring MyAssetsWidget.
 * Presentational: the profile bundle already fetches the devices, so
 * they arrive as a prop rather than a second request.
 */
import type { Asset } from '@nosdesk/core/types/asset';
import SectionCard from '@/components/common/SectionCard.vue';
import AssetStatusBadge from '@/components/assets/AssetStatusBadge.vue';

defineProps<{
  devices: Asset[];
}>();
</script>

<template>
  <SectionCard content-padding="">
    <template #title>{{ $t('user-profile-assets-title') }}</template>

    <div
      v-if="devices.length === 0"
      class="px-4 py-6 text-sm text-secondary text-center"
    >
      {{ $t('user-profile-assets-empty') }}
    </div>

    <ul v-else class="divide-y divide-default">
      <li v-for="d in devices" :key="d.id">
        <router-link
          :to="`/assets/${d.id}`"
          class="flex items-center gap-3 px-4 py-2.5 hover:bg-surface-hover transition-colors group"
        >
          <div class="min-w-0 flex-1">
            <p class="text-sm text-primary truncate group-hover:text-accent transition-colors">
              {{ d.name }}
            </p>
            <p class="mt-0.5 text-2xs text-tertiary truncate">
              {{ [d.manufacturer, d.model].filter(Boolean).join(' ') || $t('user-profile-asset-manufacturer-unknown') }}<template v-if="d.asset_tag && d.asset_tag !== d.name"> &middot; {{ d.asset_tag }}</template>
            </p>
          </div>
          <AssetStatusBadge :status="d.status" variant="plain" class="flex-shrink-0" />
        </router-link>
      </li>
    </ul>
  </SectionCard>
</template>
