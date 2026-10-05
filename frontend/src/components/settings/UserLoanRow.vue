<script setup lang="ts">
/**
 * A device on loan to the user, in UserDevicesCard's row style: the device,
 * and when it's due back (or "On loan" for an open-ended loan).
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import StatusPill from '@/components/common/StatusPill.vue';
import AssetStatusBadge from '@/components/assets/AssetStatusBadge.vue';
import { loanDue } from '@/components/assets/loanDue';
import type { ProfileLoan } from '@/services/userService';

const props = defineProps<{
  loan: ProfileLoan;
}>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const due = computed(() => loanDue(props.loan, t));
</script>

<template>
  <router-link
    :to="`/assets/${loan.asset.id}`"
    class="flex items-center gap-3 px-4 py-2.5 hover:bg-surface-hover transition-colors group"
  >
    <div class="min-w-0 flex-1">
      <p class="text-sm text-primary truncate group-hover:text-accent transition-colors">
        {{ loan.asset.name }}
      </p>
      <p class="mt-0.5 text-2xs text-tertiary truncate">
        {{ [loan.asset.manufacturer, loan.asset.model].filter(Boolean).join(' ') || $t('user-profile-asset-manufacturer-unknown') }}<template v-if="loan.asset.asset_tag && loan.asset.asset_tag !== loan.asset.name"> &middot; {{ loan.asset.asset_tag }}</template>
      </p>
    </div>
    <StatusPill v-if="due" :label="due.label" :tone="due.tone" class="flex-shrink-0" />
    <AssetStatusBadge v-else :status="loan.asset.status" variant="plain" class="flex-shrink-0" />
  </router-link>
</template>
