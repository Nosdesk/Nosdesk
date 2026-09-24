<script setup lang="ts">
/**
 * The outcome of a test send: which step failed and the one thing to check,
 * with the server's own reply underneath. "Accepted", never "delivered": a
 * server taking the message says nothing about the inbox.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import Icon from '@/components/common/Icon.vue';
import type { EmailTestResult } from '@nosdesk/core/services/workspaceEmailService';

const props = defineProps<{
  result: EmailTestResult;
  /** Hosted hides the operator-only allowlist hint. */
  managed?: boolean;
}>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string>) => fluent.$t(key, args);

const copy = computed(() => {
  const r = props.result;
  if (r.ok) {
    return { title: t('email-test-ok-title'), hint: t('email-test-ok-hint', { to: r.to }) };
  }
  const code = r.code ?? 'unknown';
  const known = [
    'dns',
    'egress_blocked',
    'connect',
    'tls',
    'auth',
    'rejected',
    'timeout',
    'incomplete',
    'invalid',
  ];
  const key = known.includes(code) ? code.replace('_', '-') : 'unknown';
  const hintKey =
    code === 'egress_blocked' && props.managed ? 'email-test-egress-blocked-hosted-hint' : `email-test-${key}-hint`;
  return { title: t(`email-test-${key}-title`), hint: t(hintKey) };
});
</script>

<template>
  <div
    role="status"
    class="flex items-start gap-3 rounded-lg border p-3"
    :class="
      result.ok
        ? 'border-status-success/40 bg-status-success/10'
        : 'border-status-error/40 bg-status-error/10'
    "
  >
    <Icon
      :name="result.ok ? 'checkCircle' : 'xCircle'"
      class="mt-0.5 shrink-0"
      :class="result.ok ? 'text-status-success' : 'text-status-error'"
    />
    <div class="flex min-w-0 flex-col gap-1">
      <p class="text-sm font-medium text-primary">{{ copy.title }}</p>
      <p class="text-sm text-secondary">{{ copy.hint }}</p>
      <code
        v-if="!result.ok && result.detail"
        class="mt-1 font-mono text-xs text-tertiary break-all"
        >{{ result.detail }}</code
      >
      <slot />
    </div>
  </div>
</template>
