<!--
The workspace's email as recipients see it: the test email, drawn by the
server's own template on the light or the dark paper, and a button that
sends it for real.

The frame is `sandbox="allow-same-origin"` with no `allow-scripts`, as for
inbound mail (EmailHtmlBody): nothing in it runs, and the page can read its
height. The preview's images load from this origin and its links open
nothing.
-->
<script setup lang="ts">
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQuery } from '@pinia/colada'
import AlertMessage from '@/components/common/AlertMessage.vue'
import Button from '@/components/common/Button.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import EmailTestResult from '@/components/admin/email/EmailTestResult.vue'
import brandingService from '@nosdesk/core/services/brandingService'
import workspaceEmailService, {
  type EmailTestResult as TestResult,
} from '@nosdesk/core/services/workspaceEmailService'
import { errorStatus, extractErrorMessage } from '@/utils/errors'

const props = defineProps<{
  /** The branding's `updated_at`: a new value draws the preview again. */
  version: string | null
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const previewQuery = useQuery({
  key: () => ['branding-email-preview', props.version ?? ''],
  query: () => brandingService.getEmailPreview(),
  // Keep the last drawing up while a change redraws it.
  placeholderData: (previous) => previous,
})
const loadError = computed(() =>
  previewQuery.error.value && !previewQuery.data.value
    ? extractErrorMessage(previewQuery.error.value, t('admin-branding-email-error-load'))
    : '',
)

type Paper = 'light' | 'dark'
const paper = ref<Paper>('light')
const paperOptions = computed<{ value: Paper; label: string }[]>(() => [
  { value: 'light', label: t('admin-branding-email-light') },
  { value: 'dark', label: t('admin-branding-email-dark') },
])
const html = computed(() => previewQuery.data.value?.[paper.value] ?? '')

// The letter's own height, so the card shows all of it.
const frame = ref<HTMLIFrameElement | null>(null)
const height = ref(640)
function fitHeight() {
  const body = frame.value?.contentDocument?.body
  if (body) height.value = Math.ceil(body.scrollHeight)
}

// Hosted hides the operator-only hint in a failed test's result.
const serverQuery = useQuery({
  key: ['email-config'],
  query: () => workspaceEmailService.getServerConfig(),
})
const managed = computed(() => !!serverQuery.data.value?.managed)

const testing = ref(false)
const testError = ref('')
const testResult = ref<TestResult | null>(null)
async function sendTest() {
  testError.value = ''
  testResult.value = null
  testing.value = true
  try {
    testResult.value = await workspaceEmailService.sendTest()
  } catch (e) {
    testError.value =
      errorStatus(e) === 429
        ? t('email-relay-error-rate-limited')
        : extractErrorMessage(e, t('email-domain-error-test'))
  } finally {
    testing.value = false
  }
}
</script>

<template>
  <section class="bg-surface border border-default rounded-xl p-6 flex flex-col gap-4">
    <div class="flex items-start justify-between gap-3 flex-wrap">
      <div class="flex flex-col gap-1">
        <h2 class="text-lg font-semibold text-primary">{{ t('admin-branding-email-heading') }}</h2>
        <p class="text-sm text-secondary">{{ t('admin-branding-email-description') }}</p>
      </div>
      <SegmentedControl
        v-model="paper"
        :options="paperOptions"
        :aria-label="t('admin-branding-email-paper-label')"
        size="sm"
      />
    </div>

    <AlertMessage v-if="loadError" type="error" :message="loadError" />

    <iframe
      v-if="html"
      ref="frame"
      :srcdoc="html"
      sandbox="allow-same-origin"
      :title="t('admin-branding-email-frame-title')"
      class="block w-full rounded-lg border border-default"
      :style="{ height: `${height}px` }"
      referrerpolicy="no-referrer"
      @load="fitHeight"
    />
    <div
      v-else-if="!loadError"
      class="w-full rounded-lg border border-default bg-surface-alt"
      :style="{ height: `${height}px` }"
    />
    <p v-if="paper === 'dark'" class="text-xs text-tertiary">
      {{ t('admin-branding-email-dark-hint') }}
    </p>

    <div class="flex flex-col gap-3 border-t border-default pt-4">
      <div class="flex items-center justify-between gap-3 flex-wrap">
        <span class="text-sm text-secondary">{{ t('admin-branding-email-test-description') }}</span>
        <Button variant="secondary" size="sm" icon="send" :loading="testing" @click="sendTest">
          {{ t('email-domain-test-button') }}
        </Button>
      </div>
      <AlertMessage v-if="testError" type="error" :message="testError" />
      <EmailTestResult v-if="testResult" :result="testResult" :managed="managed" />
    </div>
  </section>
</template>
