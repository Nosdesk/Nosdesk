import {
  allDocumentIconEmojis,
  documentIconEmojiDatabase,
  uniqueEmojis,
} from '@/data/documentIconEmojis'

export const DOCUMENT_ICON_CATEGORY_IDS = [
  'suggested',
  'documents',
  'objects',
  'symbols',
  'nature',
  'animals',
  'people',
  'travel',
  'food',
  'activities',
] as const

export type DocumentIconCategoryId = (typeof DOCUMENT_ICON_CATEGORY_IDS)[number]

export const DOCUMENT_ICON_SUGGESTED: readonly string[] = [
  '📄', '📝', '📋', '📁', '📚', '💡', '⚙️', '🚀', '✅', '📌', '🔗', '💻', '🎯', '⭐', '🔒',
]

/** i18n keys for picker category tab labels (`doc-icon-selector-category-*`). */
export const DOCUMENT_ICON_CATEGORY_LABEL_KEYS: Record<DocumentIconCategoryId, string> = {
  suggested: 'doc-icon-selector-category-suggested',
  documents: 'doc-icon-selector-category-documents',
  objects: 'doc-icon-selector-category-objects',
  symbols: 'doc-icon-selector-category-symbols',
  nature: 'doc-icon-selector-category-nature',
  animals: 'doc-icon-selector-category-animals',
  people: 'doc-icon-selector-category-people',
  travel: 'doc-icon-selector-category-travel',
  food: 'doc-icon-selector-category-food',
  activities: 'doc-icon-selector-category-activities',
}

const CATEGORY_KEYWORDS: Record<Exclude<DocumentIconCategoryId, 'suggested'>, readonly string[]> = {
  documents: ['document', 'file', 'book', 'note', 'paper', 'folder', 'mail', 'email', 'card', 'calendar'],
  objects: ['tool', 'computer', 'phone', 'device', 'light', 'key', 'lock', 'bell', 'clock', 'battery'],
  symbols: ['check', 'cross', 'warning', 'question', 'exclamation', 'arrow', 'play', 'stop', 'plus', 'minus', 'star', 'heart'],
  nature: ['plant', 'tree', 'flower', 'leaf', 'sun', 'moon', 'weather', 'cloud', 'rain', 'snow', 'earth', 'ocean', 'water'],
  animals: ['dog', 'cat', 'bird', 'fish', 'animal', 'pet', 'bear', 'monkey', 'insect', 'bug'],
  people: ['face', 'person', 'user', 'people', 'hand', 'heart', 'love', 'smile', 'happy', 'think'],
  travel: ['car', 'plane', 'train', 'ship', 'building', 'house', 'city', 'rocket', 'travel', 'transport'],
  food: ['food', 'fruit', 'vegetable', 'drink', 'coffee', 'eat', 'meal', 'dessert', 'sweet'],
  activities: ['sport', 'game', 'music', 'art', 'party', 'celebration', 'play', 'ball', 'camera', 'movie'],
}

function iconsMatchingKeywords(keywords: readonly string[]): string[] {
  return uniqueEmojis(
    documentIconEmojiDatabase
      .filter((entry) => entry.keywords.some((keyword) => keywords.includes(keyword)))
      .map((entry) => entry.emoji),
  )
}

export function documentIconsForCategory(category: DocumentIconCategoryId): readonly string[] {
  if (category === 'suggested') return DOCUMENT_ICON_SUGGESTED
  return iconsMatchingKeywords(CATEGORY_KEYWORDS[category])
}

export function searchDocumentIcons(query: string): string[] {
  const normalized = query.trim().toLowerCase()
  if (!normalized) return []
  return documentIconEmojiDatabase
    .filter((entry) => entry.keywords.some((keyword) => keyword.includes(normalized)))
    .map((entry) => entry.emoji)
}

export { allDocumentIconEmojis }
