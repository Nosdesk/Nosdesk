<script setup lang="ts">
/**
 * Right-pane "what the customer will see" preview for the admin
 * editor. Renders the template against fixed placeholder values
 * so the admin can sanity-check variable usage before saving.
 *
 * The placeholder values are intentionally generic ("Jane Doe",
 * "Nosdesk") and never come from real data, since at edit time
 * the template isn't bound to a specific ticket. Once an admin
 * saves and a tech inserts the template, the picker substitutes
 * the live ticket context, see CannedResponsePicker.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import { renderTemplate, type TemplateVars } from '@nosdesk/core/services/cannedResponsesService';

const { $t } = useFluent();

const props = defineProps<{
  body: string;
}>();

// Stable placeholder values. The literal "12345" for ticket_id
// matches what every other helpdesk uses in mock-up screenshots;
// the names are common-enough Western placeholders without being
// tied to a specific demographic.
const SAMPLE_VARS: TemplateVars = {
  ticket_id: 12345,
  ticket_title: 'Mock ticket title',
  customer_name: 'Jane Doe',
  tech_name: 'Alex Smith',
  app_name: 'Nosdesk',
};

const rendered = computed(() => renderTemplate(props.body, SAMPLE_VARS));
</script>

<template>
  <div class="flex flex-col gap-2">
    <div class="flex items-center gap-1.5 text-xs uppercase tracking-wide text-secondary">
      <span>{{ $t('admin-canned-responses-preview-heading') }}</span>
    </div>
    <div
      class="border border-default rounded-lg bg-surface-alt p-3 text-sm text-primary whitespace-pre-wrap min-h-[200px]"
    >
      <template v-if="rendered">{{ rendered }}</template>
      <span v-else class="text-tertiary italic">
        {{ $t('admin-canned-responses-preview-empty') }}
      </span>
    </div>
    <p class="text-xs text-tertiary">
      {{ $t('admin-canned-responses-preview-hint') }}
    </p>
  </div>
</template>
