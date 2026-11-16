<script setup lang="ts">
/**
 * A link that saves a file: a stored server path (`/api/files/...`,
 * `/uploads/...`) or a `blob:` URL for one not uploaded yet.
 *
 * On the web it is a plain `<a href download>`. The browser saves the file
 * with the session cookie, and middle-click and Save Link As work as on any
 * link. In the mobile app the webview ignores `download`, so a tap hands the
 * file to `saveFile` (`@/platform/files`), which downloads it natively and
 * opens the share sheet. While that runs the link is busy and its content
 * gives way to a spinner. A failure shows a toast; dismissing the sheet is
 * not one.
 *
 * Every other attribute and listener lands on the `<a>`, so a call site
 * dresses it as an icon button, a list row or a tile overlay.
 */
import { computed } from 'vue'
import Spinner from '@/components/common/Spinner.vue'
import { useFileSave } from '@/composables/useFileSave'
import { isTauriRuntime } from '@/platform/runtime'
import { fileHref } from '@/platform/files'

const props = withDefaults(
  defineProps<{
    /** The stored path, or a `blob:` URL. */
    href: string
    /** The saved file's name. Without one the server's name is used. */
    filename?: string
    spinnerSize?: 'xs' | 'sm' | 'md'
  }>(),
  { spinnerSize: 'sm' },
)

const { save, saving } = useFileSave()
const resolvedHref = computed(() => fileHref(props.href))

function onClick(event: MouseEvent) {
  if (!isTauriRuntime()) return
  event.preventDefault()
  if (!saving.value) void save(props.href, props.filename)
}
</script>

<template>
  <a
    :href="resolvedHref"
    :download="filename ?? ''"
    :aria-busy="saving || undefined"
    @click="onClick"
  >
    <Spinner v-if="saving" :size="spinnerSize" />
    <slot v-else />
  </a>
</template>
