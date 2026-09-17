<script setup lang="ts">
/**
 * Editable combobox (suggest-or-type), on Reka's Autocomplete. The input's
 * text IS the value: the user types any string, and known values are
 * offered as suggestions to save retyping. WAI-ARIA editable combobox
 * with list autocomplete, the sibling of the pick-only
 * `SearchableDropdown`.
 *
 * Reka owns the ARIA (`role=combobox` input with `aria-expanded`,
 * `aria-controls`, `aria-activedescendant` over `role=option` rows), the
 * arrow/Home/End/Enter/Escape model, the locale-aware filter over the
 * typed text (accent and case insensitive) and the positioned popup
 * under the input. The popup follows the input on every viewport: a
 * suggestion list belongs under the field being typed in, not in a
 * sheet over the keyboard.
 *
 * Single value only. Use for free-text-with-suggestions fields (ITAD
 * vendor, ad-hoc location, etc.).
 */
import { computed, ref, useId } from 'vue';
import {
  AutocompleteAnchor,
  AutocompleteContent,
  AutocompleteEmpty,
  AutocompleteInput,
  AutocompleteItem,
  AutocompletePortal,
  AutocompleteRoot,
  AutocompleteTrigger,
  AutocompleteViewport,
} from 'reka-ui';
import Icon from './Icon.vue';

const props = withDefaults(
  defineProps<{
    modelValue: string;
    /** Known values offered as suggestions; a new value can still be typed. */
    options: string[];
    placeholder?: string;
    disabled?: boolean;
    size?: 'xs' | 'sm' | 'md' | 'lg';
    label?: string;
    description?: string;
    error?: string;
    required?: boolean;
  }>(),
  {
    placeholder: '',
    disabled: false,
    size: 'md',
  },
);

const emit = defineEmits<{ (e: 'update:modelValue', value: string): void }>();

const generatedId = useId();
const inputId = computed(() => `autocomplete-${generatedId}`);
const describedById = computed(() =>
  props.error || props.description ? `${inputId.value}-desc` : undefined,
);

const isOpen = ref(false);

const sizeClasses = computed(() => {
  switch (props.size) {
    case 'xs':
      return { input: 'px-1.5 py-0.5 text-sm', option: 'px-3 py-1.5' };
    case 'sm':
      return { input: 'px-3 py-1.5 text-sm', option: 'px-3 py-2' };
    case 'lg':
      return { input: 'px-4 py-3.5 text-base', option: 'px-4 py-3' };
    default:
      return { input: 'px-4 py-3 text-sm', option: 'px-4 py-2.5' };
  }
});

function onUpdate(value: string | undefined) {
  emit('update:modelValue', value ?? '');
}
</script>

<template>
  <div class="flex flex-col gap-1.5">
    <label
      v-if="label"
      :for="inputId"
      class="text-xs font-medium text-tertiary uppercase tracking-wide"
    >
      {{ label
      }}<span v-if="required" class="text-status-error ml-0.5" aria-hidden="true">*</span>
    </label>
    <AutocompleteRoot
      v-model:open="isOpen"
      :model-value="modelValue"
      :disabled="disabled"
      open-on-focus
      open-on-click
      highlight-on-hover
      @update:model-value="onUpdate"
    >
      <AutocompleteAnchor class="relative">
        <AutocompleteInput
          :id="inputId"
          :placeholder="placeholder"
          :aria-invalid="error ? 'true' : undefined"
          :aria-describedby="describedById"
          class="w-full bg-surface-alt border rounded-lg text-primary transition-all duration-200 pr-9"
          :class="[
            sizeClasses.input,
            error ? 'border-status-error' : 'border-subtle',
            disabled
              ? 'opacity-50 cursor-not-allowed'
              : 'hover:border-strong focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent',
            isOpen && !disabled ? 'border-accent ring-1 ring-accent' : '',
          ]"
        />
        <!-- Decorative toggle; the input is the accessible control. -->
        <AutocompleteTrigger
          tabindex="-1"
          aria-hidden="true"
          :disabled="disabled"
          class="absolute right-0 top-0 h-full px-2.5 flex items-center text-tertiary"
          :class="disabled ? '' : 'hover:text-secondary'"
        >
          <span class="transition-transform duration-200 inline-flex" :class="{ 'rotate-180': isOpen }">
            <Icon name="chevronDown" />
          </span>
        </AutocompleteTrigger>
      </AutocompleteAnchor>

      <AutocompletePortal>
        <AutocompleteContent
          position="popper"
          side="bottom"
          align="start"
          :side-offset="2"
          :collision-padding="8"
          hide-when-empty
          class="popover-inner autocomplete-surface z-overlay bg-surface border border-default rounded-lg shadow-xl overflow-hidden"
          :style="{
            width: 'var(--reka-combobox-trigger-width)',
            maxHeight: 'min(16rem, var(--reka-combobox-content-available-height))',
          }"
        >
          <AutocompleteViewport class="py-1 overflow-y-auto text-sm">
            <AutocompleteEmpty />
            <AutocompleteItem
              v-for="opt in options"
              :key="opt"
              :value="opt"
              class="w-full text-left text-primary transition-colors truncate flex items-center min-h-[44px] md:min-h-0 outline-none cursor-default"
              :class="[
                sizeClasses.option,
                opt === modelValue
                  ? 'bg-accent/10 text-accent'
                  : 'hover:bg-surface-hover data-[highlighted]:bg-surface-hover',
              ]"
            >
              {{ opt }}
            </AutocompleteItem>
          </AutocompleteViewport>
        </AutocompleteContent>
      </AutocompletePortal>
    </AutocompleteRoot>
    <p
      v-if="error || description"
      :id="describedById"
      class="text-xs"
      :class="error ? 'text-status-error' : 'text-tertiary'"
    >
      {{ error || description }}
    </p>
  </div>
</template>

<style>
.autocomplete-surface {
  transform-origin: var(--reka-combobox-content-transform-origin);
}
</style>
