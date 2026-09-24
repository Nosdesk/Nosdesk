<script setup lang="ts">
/**
 * The outcome of a connection test: which step failed and the one thing to
 * check, with the server's own reply underneath. For a test send the copy
 * says "accepted", never "delivered": a server taking the message says
 * nothing about the inbox.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import Icon from '@/components/common/Icon.vue';
const props = withDefaults(
  defineProps<{
    result: { ok: boolean; code: string | null; detail: string | null; to?: string };
    /** `smtp`: a test send. `imap`: a mailbox sign-in. */
    kind?: 'smtp' | 'imap';
    /** Hosted hides the operator-only allowlist hint. */
    managed?: boolean;
    /** Replaces the hint for a failure the caller knows more about. */
    hint?: string;
  }>(),
  { kind: 'smtp', managed: false, hint: undefined },
);

const fluent = useFluent();
const t = (key: string, args?: Record<string, string>) => fluent.$t(key, args);

const KNOWN = {
  smtp: ['dns', 'egress_blocked', 'connect', 'tls', 'auth', 'rejected', 'timeout', 'incomplete', 'invalid'],
  imap: ['dns', 'egress_blocked', 'connect', 'tls', 'auth', 'mailbox', 'timeout', 'invalid'],
};

const copy = computed(() => {
  const r = props.result;
  const prefix = props.kind === 'imap' ? 'imap-test' : 'email-test';
  if (r.ok) {
    return { title: t(`${prefix}-ok-title`), hint: t(`${prefix}-ok-hint`, { to: r.to ?? '' }) };
  }
  const code = r.code ?? 'unknown';
  const key = KNOWN[props.kind].includes(code) ? code.replace('_', '-') : 'unknown';
  // The egress hints are shared: whether the operator can allow a host
  // depends on hosting, not on the protocol.
  const hintKey =
    code === 'egress_blocked'
      ? props.managed
        ? 'email-test-egress-blocked-hosted-hint'
        : 'email-test-egress-blocked-hint'
      : `${prefix}-${key}-hint`;
  return { title: t(`${prefix}-${key}-title`), hint: props.hint ?? t(hintKey) };
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
