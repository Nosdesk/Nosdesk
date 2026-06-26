<script setup lang="ts">
import { computed } from 'vue';
import { RouterLink } from 'vue-router';
import { useFluent } from 'fluent-vue';
import Button from '@/components/common/Button.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import { useReference } from '@/sync/composables';
import { useUsersDirectory } from '@/composables/useUsersDirectory';
import { formatCompactDate } from '@nosdesk/core/utils/dateUtils';
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

interface DueInfo {
  label: string;
  tone: 'overdue' | 'soon' | 'normal';
}
const due = computed<DueInfo | null>(() => {
  if (!isActive.value || !props.loan.due_back) return null;
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const d = new Date(`${props.loan.due_back}T00:00:00`);
  const days = Math.round((d.getTime() - today.getTime()) / 86_400_000);
  if (days < 0) return { label: t('asset-loan-due-overdue'), tone: 'overdue' };
  if (days === 0) return { label: t('asset-loan-due-today'), tone: 'soon' };
  if (days <= 2) return { label: t('asset-loan-due-soon', { days }), tone: 'soon' };
  return { label: t('asset-loan-due-on', { date: formatCompactDate(props.loan.due_back) }), tone: 'normal' };
});
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
            'bg-status-error-muted text-status-error': due.tone === 'overdue',
            'bg-status-warning-bg text-status-warning': due.tone === 'soon',
            'text-tertiary': due.tone === 'normal',
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
