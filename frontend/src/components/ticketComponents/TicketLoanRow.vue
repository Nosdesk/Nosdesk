<script setup lang="ts">
import { computed } from 'vue';
import { RouterLink } from 'vue-router';
import { useFluent } from 'fluent-vue';
import Button from '@/components/common/Button.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import { useReference } from '@nosdesk/core/sync/composables';
import { useUsersDirectory } from '@/composables/useUsersDirectory';
import { formatCompactDate } from '@nosdesk/core/utils/dateUtils';
import { loanDue } from '@/components/assets/loanDue';
import type { Asset, AssetLoan } from '@nosdesk/core/types/asset';

const props = defineProps<{ loan: AssetLoan; canReturn?: boolean }>();
const emit = defineEmits<{ (e: 'return'): void }>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const { getUserHandle } = useUsersDirectory();

const asset = useReference<Asset>('asset', () => props.loan.asset_id);
const deviceName = computed(
  () => asset.value?.name ?? t('asset-loan-device-fallback', { id: props.loan.asset_id }),
);
const borrowerName = computed(
  () => getUserHandle(props.loan.borrower_user_uuid).user.value?.name ?? t('asset-loan-unknown-borrower'),
);
const isActive = computed(() => !props.loan.returned_at);

const due = computed(() => loanDue(props.loan, t));
</script>

<template>
  <div class="flex items-start gap-2 py-2">
    <UserAvatar :uuid="loan.borrower_user_uuid" size="xs" :clickable="false" class="mt-0.5" />
    <div class="min-w-0 flex-1">
      <div class="flex items-center gap-1.5 flex-wrap">
        <RouterLink
          :to="`/assets/${loan.asset_id}`"
          class="text-sm font-medium text-primary hover:underline truncate"
        >
          {{ deviceName }}
        </RouterLink>
        <span
          v-if="due"
          class="inline-flex items-center px-1.5 py-0.5 rounded-full text-xs font-medium whitespace-nowrap"
          :class="{
            'bg-status-error-muted text-status-error': due.tone === 'critical',
            'bg-status-warning-bg text-status-warning': due.tone === 'caution',
            'text-tertiary': due.tone === 'neutral',
          }"
        >
          {{ due.label }}
        </span>
      </div>
      <p class="text-xs text-tertiary truncate">
        {{ borrowerName
        }}<span v-if="!isActive && loan.returned_at">
          · {{ $t('asset-loan-returned-on', { date: formatCompactDate(loan.returned_at) }) }}</span>
      </p>
    </div>
    <Button
      v-if="isActive && canReturn"
      class="shrink-0"
      size="sm"
      variant="secondary"
      icon="check"
      @click="emit('return')"
    >
      {{ $t('asset-loan-return') }}
    </Button>
  </div>
</template>
