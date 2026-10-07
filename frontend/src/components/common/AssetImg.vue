<script setup lang="ts">
/**
 * An `<img>` whose src goes through the platform's asset resolver
 * (`assetUrl`). On web that returns the path as is. In the mobile app a
 * relative `/api/files/...` or `/uploads/...` path would resolve against
 * `tauri://localhost` and carry no session, so it becomes the scheme the app
 * proxies; `data:`, `blob:` and absolute URLs pass through. Every other
 * attribute and listener lands on the `<img>`.
 */
import { computed } from 'vue'
import { assetUrl } from '@nosdesk/core/transport'

defineOptions({ inheritAttrs: false })

const props = defineProps<{ src?: string | null }>()

const resolved = computed(() => (props.src ? assetUrl(props.src) : undefined))
</script>

<template>
  <img v-bind="$attrs" :src="resolved" />
</template>
