import { computed, type Ref } from 'vue'
import { useFluent } from 'fluent-vue'
import {
  DOCUMENT_ICON_CATEGORY_IDS,
  DOCUMENT_ICON_CATEGORY_LABEL_KEYS,
  allDocumentIconEmojis,
  documentIconsForCategory,
  searchDocumentIcons,
  type DocumentIconCategoryId,
} from '@/data/documentIconCategories'

export function useDocumentIconPicker(options: {
  activeCategory: Ref<string>
  searchQuery: Ref<string>
}) {
  const { $t } = useFluent()

  const iconCategories = computed(() => {
    const categories: Record<string, { label: string; icons: readonly string[] }> = {}
    for (const id of DOCUMENT_ICON_CATEGORY_IDS) {
      categories[id] = {
        label: $t(DOCUMENT_ICON_CATEGORY_LABEL_KEYS[id]),
        icons: documentIconsForCategory(id),
      }
    }
    return categories
  })

  const categoryKeys = computed(() => [...DOCUMENT_ICON_CATEGORY_IDS])

  const filteredIcons = computed(() => {
    const query = options.searchQuery.value
    if (query.trim()) return searchDocumentIcons(query)
    const category = options.activeCategory.value as DocumentIconCategoryId
    return documentIconsForCategory(category)
  })

  return {
    iconCategories,
    categoryKeys,
    filteredIcons,
    allIcons: allDocumentIconEmojis,
  }
}
