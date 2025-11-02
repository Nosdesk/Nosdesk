<!-- DocumentIconModal.vue -->
<script setup lang="ts">
import { ref, computed } from 'vue';
import Modal from './Modal.vue';

interface Props {
  show: boolean;
  initialIcon?: string;
}

const props = withDefaults(defineProps<Props>(), {
  initialIcon: '📄',
});

const emit = defineEmits(['update:icon', 'close']);

const selectedIcon = ref(props.initialIcon);
const activeTab = ref('document');
const searchQuery = ref('');

// Common document-related emojis
const documentEmojis = [
  '📄', '📝', '📑', '📃', '📜', '📋', '📁', '📂', 
  '📓', '📔', '📕', '📗', '📘', '📙', '📚', '📖',
  '🗒️', '🗓️', '📊', '📈', '📉', '🔍', '🔎', '🔖'
];

// Common UI/UX emojis
const uiEmojis = [
  '💡', '⚙️', '🔧', '🛠️', '🧰', '📌', '📎', '🔗',
  '🔒', '🔑', '🔔', '🔕', '📱', '💻', '🖥️', '⌨️'
];

// Common status/indicator emojis
const statusEmojis = [
  '✅', '❌', '⚠️', '❓', '❗', '💯', '🆕', '🔄',
  '⏱️', '🚀', '🔥', '❄️', '⭐', '🌟', '💫', '🎯'
];

// Common people/team emojis
const peopleEmojis = [
  '👤', '👥', '👨‍💻', '👩‍💻', '🧑‍💻', '👨‍🔧', '👩‍🔧', '🤝',
  '🙋‍♂️', '🙋‍♀️', '💬', '🗣️', '👁️', '🧠', '❤️', '🤔'
];

// SVG icons (using heroicons as examples)
const svgIcons = [
  { id: 'document', svg: '<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 0 0-3.375-3.375h-1.5A1.125 1.125 0 0 1 13.5 7.125v-1.5a3.375 3.375 0 0 0-3.375-3.375H8.25m2.25 0H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 0 0-9-9Z" /></svg>' },
  { id: 'document-text', svg: '<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 0 0-3.375-3.375h-1.5A1.125 1.125 0 0 1 13.5 7.125v-1.5a3.375 3.375 0 0 0-3.375-3.375H8.25m0 12.75h7.5m-7.5 3H12M10.5 2.25H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 0 0-9-9Z" /></svg>' },
  { id: 'folder', svg: '<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" d="M2.25 12.75V12A2.25 2.25 0 0 1 4.5 9.75h15A2.25 2.25 0 0 1 21.75 12v.75m-8.69-6.44-2.12-2.12a1.5 1.5 0 0 0-1.061-.44H4.5A2.25 2.25 0 0 0 2.25 6v12a2.25 2.25 0 0 0 2.25 2.25h15A2.25 2.25 0 0 0 21.75 18V9a2.25 2.25 0 0 0-2.25-2.25h-5.379a1.5 1.5 0 0 1-1.06-.44Z" /></svg>' },
  { id: 'book-open', svg: '<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" d="M12 6.042A8.967 8.967 0 0 0 6 3.75c-1.052 0-2.062.18-3 .512v14.25A8.987 8.987 0 0 1 6 18c2.305 0 4.408.867 6 2.292m0-14.25a8.966 8.966 0 0 1 6-2.292c1.052 0 2.062.18 3 .512v14.25A8.987 8.987 0 0 0 18 18a8.967 8.967 0 0 0-6 2.292m0-14.25v14.25" /></svg>' },
  { id: 'academic-cap', svg: '<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" d="M4.26 10.147a60.438 60.438 0 0 0-.491 6.347A48.62 48.62 0 0 1 12 20.904a48.62 48.62 0 0 1 8.232-4.41 60.46 60.46 0 0 0-.491-6.347m-15.482 0a50.636 50.636 0 0 0-2.658-.813A59.906 59.906 0 0 1 12 3.493a59.903 59.903 0 0 1 10.399 5.84c-.896.248-1.783.52-2.658.814m-15.482 0A50.717 50.717 0 0 1 12 13.489a50.702 50.702 0 0 1 7.74-3.342M6.75 15a.75.75 0 1 0 0-1.5.75.75 0 0 0 0 1.5Zm0 0v-3.675A55.378 55.378 0 0 1 12 8.443m-7.007 11.55A5.981 5.981 0 0 0 6.75 15.75v-1.5" /></svg>' },
  { id: 'light-bulb', svg: '<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" d="M12 18v-5.25m0 0a6.01 6.01 0 0 0 1.5-.189m-1.5.189a6.01 6.01 0 0 1-1.5-.189m3.75 7.478a12.06 12.06 0 0 1-4.5 0m3.75 2.383a14.406 14.406 0 0 1-3 0M14.25 18v-.192c0-.983.658-1.823 1.508-2.316a7.5 7.5 0 1 0-7.517 0c.85.493 1.509 1.333 1.509 2.316V18" /></svg>' },
  { id: 'cog', svg: '<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" d="M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.325.196.72.257 1.075.124l1.217-.456a1.125 1.125 0 0 1 1.37.49l1.296 2.247a1.125 1.125 0 0 1-.26 1.431l-1.003.827c-.293.241-.438.613-.43.992a7.723 7.723 0 0 1 0 .255c-.008.378.137.75.43.991l1.004.827c.424.35.534.955.26 1.43l-1.298 2.247a1.125 1.125 0 0 1-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.47 6.47 0 0 1-.22.128c-.331.183-.581.495-.644.869l-.213 1.281c-.09.543-.56.94-1.11.94h-2.594c-.55 0-1.019-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 0 1-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 0 1-1.369-.49l-1.297-2.247a1.125 1.125 0 0 1 .26-1.431l1.004-.827c.292-.24.437-.613.43-.991a6.932 6.932 0 0 1 0-.255c.007-.38-.138-.751-.43-.992l-1.004-.827a1.125 1.125 0 0 1-.26-1.43l1.297-2.247a1.125 1.125 0 0 1 1.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.086.22-.128.332-.183.582-.495.644-.869l.214-1.28Z" /><path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z" /></svg>' },
  { id: 'check', svg: '<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" d="m4.5 12.75 6 6 9-13.5" /></svg>' },
  { id: 'star', svg: '<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" d="M11.48 3.499a.562.562 0 0 1 1.04 0l2.125 5.111a.563.563 0 0 0 .475.345l5.518.442c.499.04.701.663.321.988l-4.204 3.602a.563.563 0 0 0-.182.557l1.285 5.385a.562.562 0 0 1-.84.61l-4.725-2.885a.562.562 0 0 0-.586 0L6.982 20.54a.562.562 0 0 1-.84-.61l1.285-5.386a.562.562 0 0 0-.182-.557l-4.204-3.602a.562.562 0 0 1 .321-.988l5.518-.442a.563.563 0 0 0 .475-.345L11.48 3.5Z" /></svg>' },
  { id: 'rocket', svg: '<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" d="M15.59 14.37a6 6 0 0 1-5.84 7.38v-4.8m5.84-2.58a14.98 14.98 0 0 0 6.16-12.12A14.98 14.98 0 0 0 9.631 8.41m5.96 5.96a14.926 14.926 0 0 1-5.841 2.58m-.119-8.54a6 6 0 0 0-7.381 5.84h4.8m2.581-5.84a14.927 14.927 0 0 0-2.58 5.84m2.699 2.7c-.103.021-.207.041-.311.06a15.09 15.09 0 0 1-2.448-2.448 14.9 14.9 0 0 1 .06-.312m-2.24 2.39a4.493 4.493 0 0 0-1.757 4.306 4.493 4.493 0 0 0 4.306-1.758M16.5 9a1.5 1.5 0 1 1-3 0 1.5 1.5 0 0 1 3 0Z" /></svg>' },
];

// Combine all emoji categories for search
const allEmojis = [...documentEmojis, ...uiEmojis, ...statusEmojis, ...peopleEmojis];

// Computed properties for filtered icons
const filteredEmojis = computed(() => {
  if (!searchQuery.value) {
    switch (activeTab.value) {
      case 'document': return documentEmojis;
      case 'ui': return uiEmojis;
      case 'status': return statusEmojis;
      case 'people': return peopleEmojis;
      default: return allEmojis;
    }
  }
  
  return allEmojis.filter(emoji => 
    emoji.includes(searchQuery.value)
  );
});

const filteredSvgIcons = computed(() => {
  if (!searchQuery.value) return svgIcons;
  
  return svgIcons.filter(icon => 
    icon.id.toLowerCase().includes(searchQuery.value.toLowerCase())
  );
});

// Select an emoji
const selectEmoji = (emoji: string) => {
  selectedIcon.value = emoji;
  emit('update:icon', emoji);
  emit('close');
};

// Select an SVG icon
const selectSvgIcon = (svg: string) => {
  selectedIcon.value = svg;
  emit('update:icon', svg);
  emit('close');
};

// Check if the current icon is an SVG
const isSelectedIconSvg = computed(() => {
  return selectedIcon.value.startsWith('<svg');
});

// Handle close
const handleClose = () => {
  emit('close');
};
</script>

<template>
  <Modal :show="show" title="Select Document Icon" @close="handleClose">
    <div class="w-full">
      <!-- Search input -->
      <div class="mb-4">
        <input
          v-model="searchQuery"
          type="text"
          placeholder="Search icons..."
          class="w-full px-3 py-2 bg-surface text-primary rounded-md placeholder-text-tertiary focus:outline-none focus:ring-2 focus:ring-brand-blue"
        />
      </div>

      <!-- Tabs -->
      <div class="flex border-b border-default mb-4">
        <button
          @click="activeTab = 'document'"
          class="px-4 py-2 text-sm font-medium transition-colors"
          :class="activeTab === 'document' ? 'text-brand-blue border-b-2 border-brand-blue' : 'text-tertiary hover:text-secondary'"
        >
          Documents
        </button>
        <button
          @click="activeTab = 'ui'"
          class="px-4 py-2 text-sm font-medium transition-colors"
          :class="activeTab === 'ui' ? 'text-brand-blue border-b-2 border-brand-blue' : 'text-tertiary hover:text-secondary'"
        >
          UI
        </button>
        <button
          @click="activeTab = 'status'"
          class="px-4 py-2 text-sm font-medium transition-colors"
          :class="activeTab === 'status' ? 'text-brand-blue border-b-2 border-brand-blue' : 'text-tertiary hover:text-secondary'"
        >
          Status
        </button>
        <button
          @click="activeTab = 'people'"
          class="px-4 py-2 text-sm font-medium transition-colors"
          :class="activeTab === 'people' ? 'text-brand-blue border-b-2 border-brand-blue' : 'text-tertiary hover:text-secondary'"
        >
          People
        </button>
        <button
          @click="activeTab = 'svg'"
          class="px-4 py-2 text-sm font-medium transition-colors"
          :class="activeTab === 'svg' ? 'text-brand-blue border-b-2 border-brand-blue' : 'text-tertiary hover:text-secondary'"
        >
          SVG
        </button>
      </div>

      <!-- Current selection -->
      <div class="mb-4 p-3 bg-surface rounded-lg">
        <div class="flex items-center gap-3">
          <div class="flex items-center justify-center w-12 h-12 bg-surface-alt rounded-md">
            <span v-if="!isSelectedIconSvg" class="text-3xl select-none">{{ selectedIcon }}</span>
            <span v-else v-html="selectedIcon" class="w-8 h-8 text-primary"></span>
          </div>
          <div>
            <p class="text-sm text-secondary">Current Selection</p>
          </div>
        </div>
      </div>

      <!-- Emoji grid -->
      <div v-if="activeTab !== 'svg'" class="grid grid-cols-8 gap-2 max-h-60 overflow-y-auto p-2">
        <button
          v-for="emoji in filteredEmojis"
          :key="emoji"
          @click="selectEmoji(emoji)"
          class="flex items-center justify-center p-2 hover:bg-surface-hover rounded-md transition-colors"
          :class="{ 'bg-surface ring-2 ring-brand-blue': selectedIcon === emoji }"
        >
          <span class="text-xl select-none">{{ emoji }}</span>
        </button>
      </div>

      <!-- SVG icons grid -->
      <div v-if="activeTab === 'svg'" class="grid grid-cols-6 gap-2 max-h-60 overflow-y-auto p-2">
        <button
          v-for="icon in filteredSvgIcons"
          :key="icon.id"
          @click="selectSvgIcon(icon.svg)"
          class="flex items-center justify-center p-2 hover:bg-surface-hover rounded-md transition-colors"
          :class="{ 'bg-surface ring-2 ring-brand-blue': selectedIcon === icon.svg }"
        >
          <span v-html="icon.svg" class="w-6 h-6 text-primary"></span>
        </button>
      </div>

      <!-- Footer with buttons -->
      <div class="mt-6 flex justify-end gap-2">
        <button
          @click="handleClose"
          class="px-4 py-2 bg-surface text-primary rounded-md hover:bg-surface-hover transition-colors"
        >
          Cancel
        </button>
        <button
          @click="emit('update:icon', selectedIcon); emit('close');"
          class="px-4 py-2 bg-brand-blue text-primary rounded-md hover:opacity-90 transition-colors"
        >
          Select
        </button>
      </div>
    </div>
  </Modal>
</template> 