<!--
Inline command / code snippet with a copy button. One styled block so
shell commands (onboarding, docs, admin hints) look consistent and are
one click to copy instead of an error-prone manual select.

`tone="dark"` is for placing on a dark surface (e.g. the auth hero); the
default tone uses the app's surface tokens and works on any themed page.
Long commands scroll horizontally rather than wrapping.
-->
<script setup lang="ts">
import { ref, onBeforeUnmount } from 'vue';
import Icon from '@/components/common/Icon.vue';

withDefaults(
  defineProps<{
    /** The exact text copied to the clipboard and rendered. */
    code: string;
    tone?: 'default' | 'dark';
  }>(),
  { tone: 'default' },
);

const copied = ref(false);
let timer: ReturnType<typeof setTimeout> | undefined;

async function copy(text: string) {
  const ok = await writeClipboard(text);
  if (!ok) return;
  copied.value = true;
  clearTimeout(timer);
  timer = setTimeout(() => {
    copied.value = false;
  }, 1800);
}

/** Clipboard API needs a secure context (https / localhost); fall back
 *  to the legacy execCommand path for plain-http self-host instances. */
async function writeClipboard(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // fall through to the legacy path
  }
  try {
    const ta = document.createElement('textarea');
    ta.value = text;
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand('copy');
    document.body.removeChild(ta);
    return ok;
  } catch {
    return false;
  }
}

onBeforeUnmount(() => clearTimeout(timer));
</script>

<template>
  <div
    :class="[
      'flex items-stretch overflow-hidden rounded-lg border font-mono text-xs',
      tone === 'dark'
        ? 'border-white/10 bg-black/30 text-white/80'
        : 'border-default bg-surface-alt text-secondary',
    ]"
  >
    <code class="scrollbar-hide flex-1 overflow-x-auto whitespace-nowrap px-3 py-2 leading-relaxed">{{ code }}</code>
    <button
      type="button"
      @click="copy(code)"
      :aria-label="copied ? 'Copied' : 'Copy to clipboard'"
      :class="[
        'flex flex-shrink-0 items-center justify-center border-l px-2.5 transition-colors',
        tone === 'dark'
          ? 'border-white/10 text-white/50 hover:bg-white/10 hover:text-white'
          : 'border-default text-tertiary hover:bg-surface-hover hover:text-primary',
        copied ? 'text-status-success' : '',
      ]"
    >
      <Icon :name="copied ? 'check' : 'copy'" size="sm" />
    </button>
  </div>
</template>
