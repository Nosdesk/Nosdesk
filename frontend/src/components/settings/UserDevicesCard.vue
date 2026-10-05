<script setup lang="ts">
/**
 * A user's assigned devices, then the devices on loan to them, rendered in
 * the app's canonical dense-list vocabulary (SectionCard + divide-y rows),
 * mirroring MyAssetsWidget. Presentational: the profile bundle already
 * fetches both, so they arrive as props rather than a second request.
 */
import type { Asset } from '@nosdesk/core/types/asset';
import type { ProfileLoan } from '@/services/userService';
import SectionCard from '@/components/common/SectionCard.vue';
import AssetStatusBadge from '@/components/assets/AssetStatusBadge.vue';
import UserLoanRow from './UserLoanRow.vue';

withDefaults(
  defineProps<{
    devices: Asset[];
    loans?: ProfileLoan[];
  }>(),
  { loans: () => [] },
);
</script>

<template>
  <SectionCard content-padding="">
    <template #title>{{ $t('user-profile-assets-title') }}</template>

    <div
      v-if="devices.length === 0 && loans.length === 0"
      class="px-4 py-6 text-sm text-secondary text-center"
    >
      {{ $t('user-profile-assets-empty') }}
    </div>

    <ul v-if="devices.length > 0" class="divide-y divide-default">
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

    <section v-if="loans.length > 0" :class="{ 'border-t border-default': devices.length > 0 }">
      <h3 class="px-4 pt-3 pb-1 text-xs font-medium text-tertiary">
        {{ $t('user-profile-loans-heading') }}
      </h3>
      <ul class="divide-y divide-default">
        <li v-for="loan in loans" :key="loan.id">
          <UserLoanRow :loan="loan" />
        </li>
      </ul>
    </section>
  </SectionCard>
</template>
