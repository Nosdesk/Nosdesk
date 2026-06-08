import { computed, watch, type Ref } from 'vue'
import { useFluent } from 'fluent-vue'
import {
  allDocumentIconEmojis,
  documentIconEmojiDatabase,
  uniqueEmojis,
} from '@/data/documentIconEmojis'
import { preloadTwemoji } from '@/composables/useTwemoji'

export function useDocumentIconPicker(options: {
  activeCategory: Ref<string>
  searchQuery: Ref<string>
  showDropdown: Ref<boolean>
}) {
  const { $t } = useFluent()

  const iconCategories = computed(() => ({
    suggested: {
      label: $t('doc-icon-selector-category-suggested'),
      icons: ['📄', '📝', '📋', '📁', '📚', '💡', '⚙️', '🚀', '✅', '📌', '🔗', '💻', '🎯', '⭐', '🔒'],
    },
    documents: {
      label: $t('doc-icon-selector-category-documents'),
      icons: uniqueEmojis(documentIconEmojiDatabase.filter(e =>
        e.keywords.some(k => ['document', 'file', 'book', 'note', 'paper', 'folder', 'mail', 'email', 'card', 'calendar'].includes(k)),
      ).map(e => e.emoji)),
    },
    objects: {
      label: $t('doc-icon-selector-category-objects'),
      icons: uniqueEmojis(documentIconEmojiDatabase.filter(e =>
        e.keywords.some(k => ['tool', 'computer', 'phone', 'device', 'light', 'key', 'lock', 'bell', 'clock', 'battery'].includes(k)),
      ).map(e => e.emoji)),
    },
    symbols: {
      label: $t('doc-icon-selector-category-symbols'),
      icons: uniqueEmojis(documentIconEmojiDatabase.filter(e =>
        e.keywords.some(k => ['check', 'cross', 'warning', 'question', 'exclamation', 'arrow', 'play', 'stop', 'plus', 'minus', 'star', 'heart'].includes(k)),
      ).map(e => e.emoji)),
    },
    nature: {
      label: $t('doc-icon-selector-category-nature'),
      icons: uniqueEmojis(documentIconEmojiDatabase.filter(e =>
        e.keywords.some(k => ['plant', 'tree', 'flower', 'leaf', 'sun', 'moon', 'weather', 'cloud', 'rain', 'snow', 'earth', 'ocean', 'water'].includes(k)),
      ).map(e => e.emoji)),
    },
    animals: {
      label: $t('doc-icon-selector-category-animals'),
      icons: uniqueEmojis(documentIconEmojiDatabase.filter(e =>
        e.keywords.some(k => ['dog', 'cat', 'bird', 'fish', 'animal', 'pet', 'bear', 'monkey', 'insect', 'bug'].includes(k)),
      ).map(e => e.emoji)),
    },
    people: {
      label: $t('doc-icon-selector-category-people'),
      icons: uniqueEmojis(documentIconEmojiDatabase.filter(e =>
        e.keywords.some(k => ['face', 'person', 'user', 'people', 'hand', 'heart', 'love', 'smile', 'happy', 'think'].includes(k)),
      ).map(e => e.emoji)),
    },
    travel: {
      label: $t('doc-icon-selector-category-travel'),
      icons: uniqueEmojis(documentIconEmojiDatabase.filter(e =>
        e.keywords.some(k => ['car', 'plane', 'train', 'ship', 'building', 'house', 'city', 'rocket', 'travel', 'transport'].includes(k)),
      ).map(e => e.emoji)),
    },
    food: {
      label: $t('doc-icon-selector-category-food'),
      icons: uniqueEmojis(documentIconEmojiDatabase.filter(e =>
        e.keywords.some(k => ['food', 'fruit', 'vegetable', 'drink', 'coffee', 'eat', 'meal', 'dessert', 'sweet'].includes(k)),
      ).map(e => e.emoji)),
    },
    activities: {
      label: $t('doc-icon-selector-category-activities'),
      icons: uniqueEmojis(documentIconEmojiDatabase.filter(e =>
        e.keywords.some(k => ['sport', 'game', 'music', 'art', 'party', 'celebration', 'play', 'ball', 'camera', 'movie'].includes(k)),
      ).map(e => e.emoji)),
    },
  }))

  const categoryKeys = computed(() => Object.keys(iconCategories.value))

  const filteredIcons = computed(() => {
    const query = options.searchQuery.value.trim().toLowerCase()
    if (!query) {
      return iconCategories.value[options.activeCategory.value as keyof typeof iconCategories.value]?.icons ?? []
    }
    return documentIconEmojiDatabase
      .filter(e => e.keywords.some(keyword => keyword.includes(query)))
      .map(e => e.emoji)
  })

  function preloadCatalogueInBackground() {
    const run = () => { void preloadTwemoji(allDocumentIconEmojis) }
    if (typeof requestIdleCallback === 'function') {
      requestIdleCallback(run, { timeout: 4000 })
    } else {
      setTimeout(run, 250)
    }
  }

  watch(
    () => options.showDropdown.value,
    (open) => {
      if (!open) return
      void preloadTwemoji(filteredIcons.value)
      preloadCatalogueInBackground()
    },
  )

  watch(filteredIcons, (icons) => {
    if (!options.showDropdown.value || icons.length === 0) return
    void preloadTwemoji(icons)
  })

  return {
    iconCategories,
    categoryKeys,
    filteredIcons,
    allIcons: allDocumentIconEmojis,
  }
}
