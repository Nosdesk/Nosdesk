<script setup lang="ts">
/**
 * A time-elapsed trigger's settings: the clock the rule waits on, how long
 * after it started the rule runs (before the due date, for the due date
 * clock), and an optional repeat while the clock keeps running. Times are an
 * amount and a unit here; the editor saves them as minutes. A time out of
 * the server's range shows its rule as soon as it's typed; a missing pick
 * waits for a save attempt.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';

import BaseDropdown from '@/components/common/BaseDropdown.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import FormNumber from '@/components/common/FormNumber.vue';
import {
  ELAPSED_CLOCKS,
  elapsedProblems,
  isElapsedClock,
  type ElapsedDuration,
  type ElapsedUnit,
} from './triggerConfig';

const props = defineProps<{
  clockName: string;
  after: ElapsedDuration;
  repeat: boolean;
  every: ElapsedDuration;
  disabled?: boolean;
  /** Set by a save attempt, so a missing pick shows its hint. */
  showProblems?: boolean;
}>();

const emit = defineEmits<{
  (e: 'update:clockName', value: string): void;
  (e: 'update:after', value: ElapsedDuration): void;
  (e: 'update:repeat', value: boolean): void;
  (e: 'update:every', value: ElapsedDuration): void;
}>();

const { $t: t } = useFluent();

const clockKey = (clock: string) => clock.replace(/_/g, '-');
const clockOptions = computed(() =>
  ELAPSED_CLOCKS.map((clock) => ({
    value: clock,
    label: t(`admin-rule-editor-elapsed-clock-${clockKey(clock)}`),
    description: t(`admin-rule-editor-elapsed-clock-${clockKey(clock)}-hint`),
  })),
);
const clockHint = computed(() =>
  isElapsedClock(props.clockName) ? t(`admin-rule-editor-elapsed-clock-${clockKey(props.clockName)}-hint`) : undefined,
);

const UNITS: ElapsedUnit[] = ['minutes', 'hours', 'days'];
const unitOptions = computed(() =>
  UNITS.map((unit) => ({ value: unit, label: t(`admin-rule-editor-elapsed-unit-${unit}`) })),
);

const countsDown = computed(() => props.clockName === 'until_due_at');

const problems = computed(() =>
  elapsedProblems({
    changedFields: [],
    replyKind: 'customer',
    pluginUuid: '',
    pluginEvent: '',
    clockName: props.clockName,
    after: props.after,
    repeat: props.repeat,
    every: props.every,
  }),
);
/** A missing value waits for a save attempt; a value out of range doesn't. */
const shown = (key: string | null, missing: boolean): string | undefined =>
  key && (props.showProblems || !missing) ? t(key) : undefined;
const clockError = computed(() => shown(problems.value.clock, true));
const afterError = computed(() => shown(problems.value.after, props.after.amount == null));
const repeatError = computed(() => shown(problems.value.repeat, props.every.amount == null));
</script>

<template>
  <div class="flex flex-col gap-3" data-test="time-elapsed">
    <BaseDropdown
      :model-value="clockName"
      :options="clockOptions"
      :label="t('admin-rule-editor-elapsed-clock-label')"
      :placeholder="t('admin-rule-editor-elapsed-clock-placeholder')"
      :description="clockHint"
      :error="clockError"
      :disabled="disabled"
      size="sm"
      data-test="elapsed-clock"
      @update:model-value="emit('update:clockName', String($event))"
    />

    <div class="flex items-start gap-2">
      <FormNumber
        class="flex-1 min-w-0"
        :model-value="after.amount"
        :label="t(countsDown ? 'admin-rule-editor-elapsed-before-label' : 'admin-rule-editor-elapsed-after-label')"
        :error="afterError"
        :min="0"
        :disabled="disabled"
        integer
        size="sm"
        data-test="elapsed-after"
        @update:model-value="emit('update:after', { amount: $event, unit: after.unit })"
      />
      <BaseDropdown
        class="w-32 shrink-0"
        :model-value="after.unit"
        :options="unitOptions"
        :label="t('admin-rule-editor-elapsed-unit-label')"
        :disabled="disabled"
        size="sm"
        data-test="elapsed-after-unit"
        @update:model-value="emit('update:after', { amount: after.amount, unit: $event as ElapsedUnit })"
      />
    </div>

    <Checkbox
      :model-value="repeat"
      :label="t('admin-rule-editor-elapsed-repeat-label')"
      :disabled="disabled"
      data-test="elapsed-repeat"
      @update:model-value="emit('update:repeat', $event)"
    />
    <div v-if="repeat" class="flex items-start gap-2">
      <FormNumber
        class="flex-1 min-w-0"
        :model-value="every.amount"
        :label="t('admin-rule-editor-elapsed-every-label')"
        :error="repeatError"
        :min="1"
        :disabled="disabled"
        integer
        size="sm"
        data-test="elapsed-every"
        @update:model-value="emit('update:every', { amount: $event, unit: every.unit })"
      />
      <BaseDropdown
        class="w-32 shrink-0"
        :model-value="every.unit"
        :options="unitOptions"
        :label="t('admin-rule-editor-elapsed-unit-label')"
        :disabled="disabled"
        size="sm"
        data-test="elapsed-every-unit"
        @update:model-value="emit('update:every', { amount: every.amount, unit: $event as ElapsedUnit })"
      />
    </div>

    <p class="text-xs text-secondary">{{ t('admin-rule-editor-elapsed-hint') }}</p>
  </div>
</template>
