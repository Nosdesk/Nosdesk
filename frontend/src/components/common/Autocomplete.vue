<script setup lang="ts">
/**
 * Editable combobox (suggest-or-type). The input's text IS the value: the
 * user types any string, and prior/known values are offered as suggestions to
 * save retyping. WAI-ARIA "editable combobox with list autocomplete" (value =
 * the typed text), the sibling of the pick-only `SearchableDropdown`.
 *
 * Why a separate component rather than an `allowCustom` flag on
 * `SearchableDropdown`: the value contracts differ. SearchableDropdown emits
 * one of its options and discards unmatched input; this commits whatever is
 * typed. Mature systems split these for the same reason (Ant Design
 * AutoComplete vs Select, Reka UI Autocomplete vs Combobox), and it matches
 * SearchableDropdown's own "not a boolean on BaseDropdown" reasoning.
 *
 * Single value only. Use for free-text-with-suggestions fields (ITAD vendor,
 * ad-hoc location, etc.).
 */
import { computed, nextTick, ref, useId, watch } from 'vue';
import ResponsiveMenu from './ResponsiveMenu.vue';
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
const listboxId = computed(() => `autocomplete-${generatedId}-listbox`);
const optionId = (index: number) => `${listboxId.value}-opt-${index}`;
const describedById = computed(() =>
  props.error || props.description ? `${inputId.value}-desc` : undefined,
);

const isOpen = ref(false);
const inputRef = ref<HTMLInputElement | null>(null);
const wrapperRef = ref<HTMLElement | null>(null);
const listboxRef = ref<HTMLElement | null>(null);
const highlightedIndex = ref(-1);

const anchor = computed(() => ({
  type: 'element' as const,
  element: () => wrapperRef.value,
}));

// Case-insensitive substring match on the typed value; an empty value shows
// every known value so focusing a blank field reveals the suggestions.
const filteredOptions = computed<string[]>(() => {
  const q = props.modelValue.trim().toLowerCase();
  if (!q) return props.options;
  return props.options.filter((o) => o.toLowerCase().includes(q));
});

const showMenu = computed(() => isOpen.value && filteredOptions.value.length > 0);
const activeDescendant = computed(() =>
  isOpen.value && highlightedIndex.value >= 0 ? optionId(highlightedIndex.value) : undefined,
);

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

function open() {
  if (props.disabled) return;
  isOpen.value = true;
}
function close() {
  isOpen.value = false;
  highlightedIndex.value = -1;
}
function onInput(event: Event) {
  emit('update:modelValue', (event.target as HTMLInputElement).value);
  highlightedIndex.value = -1;
  open();
}
function commit(value: string) {
  emit('update:modelValue', value);
  close();
  inputRef.value?.focus();
}
function toggleMenu() {
  if (props.disabled) return;
  if (isOpen.value) {
    close();
  } else {
    open();
    inputRef.value?.focus();
  }
}

function onKeydown(event: KeyboardEvent) {
  switch (event.key) {
    case 'ArrowDown':
      event.preventDefault();
      if (!isOpen.value) {
        open();
        highlightedIndex.value = 0;
      } else {
        highlightedIndex.value = Math.min(
          highlightedIndex.value + 1,
          filteredOptions.value.length - 1,
        );
      }
      break;
    case 'ArrowUp':
      if (!isOpen.value) return;
      event.preventDefault();
      highlightedIndex.value = Math.max(highlightedIndex.value - 1, 0);
      break;
    case 'Enter':
      if (
        isOpen.value &&
        highlightedIndex.value >= 0 &&
        highlightedIndex.value < filteredOptions.value.length
      ) {
        event.preventDefault();
        commit(filteredOptions.value[highlightedIndex.value]);
      }
      break;
    case 'Escape':
      if (isOpen.value) {
        event.preventDefault();
        close();
      }
      break;
  }
}

// Keep the highlighted option scrolled into view as the user arrow-keys.
watch(highlightedIndex, async (index) => {
  if (index < 0) return;
  await nextTick();
  const items = listboxRef.value?.querySelectorAll('[role="option"]');
  items?.[index]?.scrollIntoView({ block: 'nearest' });
});
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
    <div ref="wrapperRef" class="relative">
      <input
        :id="inputId"
        ref="inputRef"
        type="text"
        role="combobox"
        aria-autocomplete="list"
        autocomplete="off"
        :value="modelValue"
        :placeholder="placeholder"
        :disabled="disabled"
        :aria-expanded="isOpen"
        :aria-controls="listboxId"
        :aria-activedescendant="activeDescendant"
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
        @input="onInput"
        @focus="open"
        @blur="close"
        @keydown="onKeydown"
      />
      <button
        type="button"
        tabindex="-1"
        aria-hidden="true"
        :disabled="disabled"
        class="absolute right-0 top-0 h-full px-2.5 flex items-center text-tertiary"
        :class="disabled ? '' : 'hover:text-secondary'"
        @mousedown.prevent
        @click="toggleMenu"
      >
        <span
          class="transition-transform duration-200 inline-flex"
          :class="{ 'rotate-180': isOpen }"
        >
          <Icon name="chevronDown" />
        </span>
      </button>

      <ResponsiveMenu
        :open="showMenu"
        :anchor="anchor"
        :title="label"
        placement="bottom-start"
        react-to-scroll="reposition"
        match-anchor-width
        :offset="2"
        :auto-focus="false"
        popover-class="bg-surface border border-default rounded-lg shadow-xl overflow-hidden"
        @close="close"
      >
        <div
          :id="listboxId"
          ref="listboxRef"
          role="listbox"
          class="py-1 overflow-y-auto max-h-64 text-sm"
        >
          <button
            v-for="(opt, index) in filteredOptions"
            :id="optionId(index)"
            :key="opt"
            type="button"
            role="option"
            :aria-selected="opt === modelValue"
            class="w-full text-left text-primary transition-colors truncate flex items-center min-h-[44px] md:min-h-0"
            :class="[
              sizeClasses.option,
              opt === modelValue
                ? 'bg-accent/10 text-accent'
                : highlightedIndex === index
                  ? 'bg-surface-hover'
                  : 'hover:bg-surface-hover',
            ]"
            @mousedown.prevent
            @click="commit(opt)"
            @mouseenter="highlightedIndex = index"
          >
            {{ opt }}
          </button>
        </div>
      </ResponsiveMenu>
    </div>
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
