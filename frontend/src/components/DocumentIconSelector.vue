<!-- DocumentIconSelector.vue - Professional Notion-style icon picker -->
<script setup lang="ts">
import { ref, watch, computed, onMounted, onUnmounted } from 'vue';
import { useHorizontalScroll } from '@/composables/useHorizontalScroll';

interface Props {
  initialIcon?: string;
  size?: 'sm' | 'md' | 'lg';
}

const props = withDefaults(defineProps<Props>(), {
  initialIcon: '📄',
  size: 'md',
});

const emit = defineEmits(['update:icon']);

const currentIcon = ref(props.initialIcon);
const showDropdown = ref(false);
const searchQuery = ref('');
const activeCategory = ref('suggested');
const dropdownRef = ref<HTMLElement | null>(null);
const triggerRef = ref<HTMLElement | null>(null);
const categoryTabsRef = ref<HTMLElement | null>(null);

// Horizontal scroll state for category tabs (3 dots for progress)
const DOT_COUNT = 3
const { canScrollLeft, canScrollRight, isOverflowing, activeDotIndex } = useHorizontalScroll(categoryTabsRef, DOT_COUNT);

// Drag-to-scroll state for category tabs
const isDragging = ref(false);
const startX = ref(0);
const scrollLeft = ref(0);

// Watch for prop changes
watch(() => props.initialIcon, (newIcon) => {
  if (newIcon !== currentIcon.value) {
    currentIcon.value = newIcon;
  }
});

// Emoji data with searchable keywords
interface EmojiData {
  emoji: string;
  keywords: string[];
}

const emojiDatabase: EmojiData[] = [
  // Documents & Files
  { emoji: '📄', keywords: ['document', 'page', 'file', 'paper', 'text'] },
  { emoji: '📝', keywords: ['memo', 'note', 'write', 'edit', 'pencil', 'document'] },
  { emoji: '📑', keywords: ['bookmark', 'tabs', 'document', 'file'] },
  { emoji: '📃', keywords: ['page', 'curl', 'document', 'file', 'paper'] },
  { emoji: '📜', keywords: ['scroll', 'paper', 'document', 'ancient', 'history'] },
  { emoji: '📋', keywords: ['clipboard', 'list', 'checklist', 'tasks', 'document'] },
  { emoji: '📁', keywords: ['folder', 'file', 'directory', 'organize'] },
  { emoji: '📂', keywords: ['folder', 'open', 'file', 'directory'] },
  { emoji: '🗂️', keywords: ['dividers', 'index', 'organize', 'tabs', 'files'] },
  { emoji: '📓', keywords: ['notebook', 'journal', 'notes', 'book'] },
  { emoji: '📔', keywords: ['notebook', 'decorative', 'journal', 'book'] },
  { emoji: '📕', keywords: ['book', 'red', 'closed', 'read'] },
  { emoji: '📗', keywords: ['book', 'green', 'closed', 'read'] },
  { emoji: '📘', keywords: ['book', 'blue', 'closed', 'read'] },
  { emoji: '📙', keywords: ['book', 'orange', 'closed', 'read'] },
  { emoji: '📚', keywords: ['books', 'library', 'read', 'study', 'stack'] },
  { emoji: '📖', keywords: ['book', 'open', 'read', 'study'] },
  { emoji: '🗒️', keywords: ['notepad', 'spiral', 'notes', 'memo'] },
  { emoji: '🗓️', keywords: ['calendar', 'spiral', 'date', 'schedule', 'planner'] },
  { emoji: '📰', keywords: ['newspaper', 'news', 'article', 'press', 'media'] },
  { emoji: '🏷️', keywords: ['tag', 'label', 'price', 'category'] },
  { emoji: '📇', keywords: ['card', 'index', 'rolodex', 'contacts'] },
  { emoji: '✉️', keywords: ['envelope', 'email', 'mail', 'letter', 'message'] },
  { emoji: '📧', keywords: ['email', 'mail', 'message', 'inbox'] },
  { emoji: '📨', keywords: ['envelope', 'incoming', 'mail', 'receive'] },
  { emoji: '📩', keywords: ['envelope', 'arrow', 'mail', 'send'] },
  { emoji: '📤', keywords: ['outbox', 'send', 'upload', 'tray'] },
  { emoji: '📥', keywords: ['inbox', 'receive', 'download', 'tray'] },
  { emoji: '📦', keywords: ['package', 'box', 'shipping', 'delivery', 'parcel'] },
  { emoji: '🗃️', keywords: ['card', 'file', 'box', 'archive', 'storage'] },
  { emoji: '🗄️', keywords: ['cabinet', 'file', 'storage', 'archive', 'office'] },
  { emoji: '🗑️', keywords: ['trash', 'delete', 'bin', 'waste', 'garbage'] },

  // Technology & Objects
  { emoji: '💻', keywords: ['laptop', 'computer', 'pc', 'tech', 'work', 'device'] },
  { emoji: '🖥️', keywords: ['desktop', 'computer', 'monitor', 'screen', 'pc'] },
  { emoji: '🖱️', keywords: ['mouse', 'computer', 'click', 'cursor'] },
  { emoji: '⌨️', keywords: ['keyboard', 'type', 'input', 'computer'] },
  { emoji: '🖨️', keywords: ['printer', 'print', 'document', 'office'] },
  { emoji: '💾', keywords: ['floppy', 'disk', 'save', 'storage', 'backup'] },
  { emoji: '💿', keywords: ['cd', 'disc', 'music', 'data', 'optical'] },
  { emoji: '📀', keywords: ['dvd', 'disc', 'movie', 'data', 'optical'] },
  { emoji: '📱', keywords: ['phone', 'mobile', 'smartphone', 'cell', 'device'] },
  { emoji: '📲', keywords: ['phone', 'arrow', 'call', 'mobile'] },
  { emoji: '☎️', keywords: ['telephone', 'phone', 'call', 'landline'] },
  { emoji: '📞', keywords: ['phone', 'receiver', 'call', 'telephone'] },
  { emoji: '📟', keywords: ['pager', 'beeper', 'device', 'communication'] },
  { emoji: '📠', keywords: ['fax', 'machine', 'document', 'office'] },
  { emoji: '📺', keywords: ['tv', 'television', 'screen', 'watch', 'media'] },
  { emoji: '📻', keywords: ['radio', 'audio', 'broadcast', 'music'] },
  { emoji: '🎙️', keywords: ['microphone', 'studio', 'podcast', 'record', 'audio'] },
  { emoji: '🎚️', keywords: ['slider', 'level', 'control', 'audio', 'mixer'] },
  { emoji: '🎛️', keywords: ['knobs', 'control', 'panel', 'audio', 'settings'] },
  { emoji: '🧭', keywords: ['compass', 'navigation', 'direction', 'explore'] },
  { emoji: '⏱️', keywords: ['stopwatch', 'timer', 'time', 'speed', 'track'] },
  { emoji: '⏲️', keywords: ['timer', 'clock', 'countdown', 'time'] },
  { emoji: '⏰', keywords: ['alarm', 'clock', 'time', 'wake', 'reminder'] },
  { emoji: '🕰️', keywords: ['clock', 'mantelpiece', 'time', 'antique'] },
  { emoji: '⌚', keywords: ['watch', 'time', 'wrist', 'clock'] },
  { emoji: '📡', keywords: ['satellite', 'antenna', 'signal', 'broadcast', 'communication'] },
  { emoji: '🔋', keywords: ['battery', 'power', 'charge', 'energy'] },
  { emoji: '🔌', keywords: ['plug', 'electric', 'power', 'connect'] },
  { emoji: '💡', keywords: ['bulb', 'light', 'idea', 'bright', 'electricity', 'innovation'] },
  { emoji: '🔦', keywords: ['flashlight', 'torch', 'light', 'search'] },
  { emoji: '🕯️', keywords: ['candle', 'light', 'flame', 'romantic'] },
  { emoji: '🧯', keywords: ['extinguisher', 'fire', 'safety', 'emergency'] },
  { emoji: '🛒', keywords: ['cart', 'shopping', 'store', 'buy', 'retail'] },
  { emoji: '🛍️', keywords: ['bags', 'shopping', 'store', 'buy', 'retail'] },

  // Tools & Work
  { emoji: '🔧', keywords: ['wrench', 'tool', 'fix', 'repair', 'settings', 'configure'] },
  { emoji: '🔩', keywords: ['bolt', 'nut', 'screw', 'tool', 'hardware'] },
  { emoji: '🛠️', keywords: ['tools', 'hammer', 'wrench', 'fix', 'build', 'repair'] },
  { emoji: '⚙️', keywords: ['gear', 'settings', 'cog', 'configure', 'mechanical', 'options'] },
  { emoji: '🔨', keywords: ['hammer', 'tool', 'build', 'construct', 'hit'] },
  { emoji: '⛏️', keywords: ['pick', 'mining', 'tool', 'dig'] },
  { emoji: '🪓', keywords: ['axe', 'chop', 'wood', 'tool'] },
  { emoji: '🔪', keywords: ['knife', 'cut', 'kitchen', 'blade'] },
  { emoji: '🗡️', keywords: ['dagger', 'sword', 'blade', 'weapon'] },
  { emoji: '⚔️', keywords: ['swords', 'crossed', 'battle', 'fight', 'combat'] },
  { emoji: '🛡️', keywords: ['shield', 'protect', 'defense', 'security', 'guard'] },
  { emoji: '🧰', keywords: ['toolbox', 'tools', 'kit', 'repair', 'fix'] },
  { emoji: '🧲', keywords: ['magnet', 'attract', 'magnetic', 'pull'] },
  { emoji: '⚖️', keywords: ['scale', 'balance', 'justice', 'weigh', 'law', 'legal'] },
  { emoji: '🔗', keywords: ['link', 'chain', 'connect', 'url', 'hyperlink'] },
  { emoji: '⛓️', keywords: ['chains', 'link', 'connect', 'bound'] },
  { emoji: '📌', keywords: ['pin', 'pushpin', 'location', 'mark', 'important'] },
  { emoji: '📍', keywords: ['pin', 'location', 'map', 'place', 'marker'] },
  { emoji: '📎', keywords: ['paperclip', 'attach', 'clip', 'office'] },
  { emoji: '🖇️', keywords: ['paperclips', 'linked', 'attach', 'office'] },
  { emoji: '✂️', keywords: ['scissors', 'cut', 'trim', 'edit'] },
  { emoji: '🖊️', keywords: ['pen', 'write', 'ink', 'sign'] },
  { emoji: '🖋️', keywords: ['pen', 'fountain', 'write', 'fancy'] },
  { emoji: '✒️', keywords: ['pen', 'nib', 'write', 'black'] },
  { emoji: '✏️', keywords: ['pencil', 'write', 'draw', 'edit'] },
  { emoji: '🖍️', keywords: ['crayon', 'draw', 'color', 'art'] },
  { emoji: '🖌️', keywords: ['paintbrush', 'art', 'paint', 'draw', 'create'] },
  { emoji: '📏', keywords: ['ruler', 'straight', 'measure', 'length'] },
  { emoji: '📐', keywords: ['triangle', 'ruler', 'set', 'square', 'measure'] },

  // Security & Privacy
  { emoji: '🔒', keywords: ['lock', 'locked', 'secure', 'private', 'password', 'security'] },
  { emoji: '🔓', keywords: ['unlock', 'unlocked', 'open', 'access'] },
  { emoji: '🔏', keywords: ['lock', 'pen', 'secure', 'sign', 'privacy'] },
  { emoji: '🔐', keywords: ['lock', 'key', 'secure', 'closed', 'password'] },
  { emoji: '🔑', keywords: ['key', 'unlock', 'password', 'access', 'login'] },
  { emoji: '🗝️', keywords: ['key', 'old', 'antique', 'unlock', 'vintage'] },
  { emoji: '🔔', keywords: ['bell', 'notification', 'alert', 'ring', 'alarm'] },
  { emoji: '🔕', keywords: ['bell', 'silent', 'mute', 'quiet', 'notification'] },
  { emoji: '🚨', keywords: ['siren', 'alert', 'emergency', 'police', 'alarm'] },

  // Symbols & Status
  { emoji: '✅', keywords: ['check', 'done', 'complete', 'yes', 'success', 'approve'] },
  { emoji: '❌', keywords: ['cross', 'wrong', 'no', 'delete', 'cancel', 'error'] },
  { emoji: '❎', keywords: ['cross', 'mark', 'no', 'reject'] },
  { emoji: '⭕', keywords: ['circle', 'hollow', 'red', 'record'] },
  { emoji: '⚠️', keywords: ['warning', 'alert', 'caution', 'danger', 'attention'] },
  { emoji: '⛔', keywords: ['stop', 'prohibited', 'forbidden', 'no', 'entry'] },
  { emoji: '🚫', keywords: ['prohibited', 'forbidden', 'no', 'ban', 'block'] },
  { emoji: '❓', keywords: ['question', 'help', 'unknown', 'what', 'ask'] },
  { emoji: '❔', keywords: ['question', 'white', 'help', 'ask'] },
  { emoji: '❗', keywords: ['exclamation', 'important', 'alert', 'attention'] },
  { emoji: '❕', keywords: ['exclamation', 'white', 'important'] },
  { emoji: '‼️', keywords: ['exclamation', 'double', 'important', 'urgent'] },
  { emoji: '⁉️', keywords: ['exclamation', 'question', 'interrobang', 'what'] },
  { emoji: '💯', keywords: ['hundred', 'perfect', 'score', 'full', 'complete'] },
  { emoji: '🆕', keywords: ['new', 'fresh', 'badge', 'label'] },
  { emoji: '🆓', keywords: ['free', 'badge', 'gratis', 'label'] },
  { emoji: '🆙', keywords: ['up', 'upgrade', 'badge', 'level'] },
  { emoji: '🆗', keywords: ['ok', 'okay', 'badge', 'approve'] },
  { emoji: '🆒', keywords: ['cool', 'badge', 'awesome', 'nice'] },
  { emoji: '🆘', keywords: ['sos', 'help', 'emergency', 'rescue'] },
  { emoji: '🔄', keywords: ['refresh', 'reload', 'sync', 'arrows', 'update', 'cycle'] },
  { emoji: '🔃', keywords: ['clockwise', 'arrows', 'refresh', 'rotate'] },
  { emoji: '🔀', keywords: ['shuffle', 'random', 'crossed', 'arrows'] },
  { emoji: '🔁', keywords: ['repeat', 'loop', 'arrows', 'cycle'] },
  { emoji: '🔂', keywords: ['repeat', 'once', 'single', 'arrows'] },
  { emoji: '▶️', keywords: ['play', 'start', 'forward', 'begin', 'video'] },
  { emoji: '⏸️', keywords: ['pause', 'stop', 'hold', 'wait'] },
  { emoji: '⏹️', keywords: ['stop', 'end', 'square', 'halt'] },
  { emoji: '⏺️', keywords: ['record', 'circle', 'red', 'recording'] },
  { emoji: '⏭️', keywords: ['next', 'track', 'forward', 'skip'] },
  { emoji: '⏮️', keywords: ['previous', 'track', 'back', 'rewind'] },
  { emoji: '⏩', keywords: ['forward', 'fast', 'speed', 'skip'] },
  { emoji: '⏪', keywords: ['rewind', 'back', 'fast', 'reverse'] },
  { emoji: '➕', keywords: ['plus', 'add', 'new', 'create', 'positive'] },
  { emoji: '➖', keywords: ['minus', 'subtract', 'remove', 'negative'] },
  { emoji: '➗', keywords: ['divide', 'division', 'math', 'split'] },
  { emoji: '✖️', keywords: ['multiply', 'times', 'math', 'cross'] },
  { emoji: '♾️', keywords: ['infinity', 'forever', 'endless', 'loop'] },
  { emoji: '💲', keywords: ['dollar', 'money', 'currency', 'price', 'cost'] },
  { emoji: '💵', keywords: ['dollar', 'money', 'cash', 'bill', 'currency'] },
  { emoji: '💴', keywords: ['yen', 'money', 'cash', 'japan', 'currency'] },
  { emoji: '💶', keywords: ['euro', 'money', 'cash', 'europe', 'currency'] },
  { emoji: '💷', keywords: ['pound', 'money', 'cash', 'uk', 'currency'] },
  { emoji: '💰', keywords: ['money', 'bag', 'dollar', 'rich', 'wealth', 'budget'] },
  { emoji: '💳', keywords: ['card', 'credit', 'payment', 'bank', 'buy'] },
  { emoji: '💸', keywords: ['money', 'wings', 'flying', 'spending', 'expense'] },
  { emoji: '🏦', keywords: ['bank', 'building', 'money', 'finance'] },

  // Stars & Achievements
  { emoji: '⭐', keywords: ['star', 'favorite', 'bookmark', 'rating', 'important'] },
  { emoji: '🌟', keywords: ['star', 'glow', 'shiny', 'special', 'featured'] },
  { emoji: '✨', keywords: ['sparkles', 'stars', 'magic', 'new', 'clean', 'shine'] },
  { emoji: '💫', keywords: ['dizzy', 'star', 'shooting', 'magic'] },
  { emoji: '⚡', keywords: ['lightning', 'bolt', 'electric', 'fast', 'power', 'energy'] },
  { emoji: '🔥', keywords: ['fire', 'hot', 'flame', 'popular', 'trending', 'lit'] },
  { emoji: '💥', keywords: ['boom', 'collision', 'explosion', 'bang', 'crash'] },
  { emoji: '💢', keywords: ['anger', 'symbol', 'mad', 'annoyed'] },
  { emoji: '💎', keywords: ['diamond', 'gem', 'jewel', 'precious', 'valuable', 'premium'] },
  { emoji: '🏆', keywords: ['trophy', 'winner', 'award', 'prize', 'champion', 'first'] },
  { emoji: '🥇', keywords: ['gold', 'medal', 'first', 'winner', 'champion'] },
  { emoji: '🥈', keywords: ['silver', 'medal', 'second', 'runner'] },
  { emoji: '🥉', keywords: ['bronze', 'medal', 'third', 'place'] },
  { emoji: '🎖️', keywords: ['medal', 'military', 'honor', 'award'] },
  { emoji: '🏅', keywords: ['medal', 'sports', 'award', 'achievement'] },
  { emoji: '🎗️', keywords: ['ribbon', 'awareness', 'reminder', 'support'] },
  { emoji: '🎀', keywords: ['ribbon', 'bow', 'gift', 'decoration'] },
  { emoji: '🎁', keywords: ['gift', 'present', 'box', 'wrapped', 'birthday'] },
  { emoji: '🎯', keywords: ['target', 'bullseye', 'goal', 'aim', 'dart', 'focus'] },
  { emoji: '🧩', keywords: ['puzzle', 'piece', 'jigsaw', 'game', 'solution'] },

  // Nature & Weather
  { emoji: '🌱', keywords: ['seedling', 'plant', 'grow', 'sprout', 'new', 'start'] },
  { emoji: '🌿', keywords: ['herb', 'leaf', 'plant', 'nature', 'green'] },
  { emoji: '☘️', keywords: ['shamrock', 'clover', 'irish', 'luck'] },
  { emoji: '🍀', keywords: ['clover', 'four', 'leaf', 'lucky', 'fortune'] },
  { emoji: '🌲', keywords: ['tree', 'evergreen', 'pine', 'forest', 'nature'] },
  { emoji: '🌳', keywords: ['tree', 'deciduous', 'nature', 'forest', 'oak'] },
  { emoji: '🌴', keywords: ['palm', 'tree', 'tropical', 'beach', 'vacation'] },
  { emoji: '🌵', keywords: ['cactus', 'desert', 'plant', 'dry'] },
  { emoji: '🌾', keywords: ['rice', 'sheaf', 'harvest', 'grain', 'farm'] },
  { emoji: '🌸', keywords: ['cherry', 'blossom', 'flower', 'spring', 'pink'] },
  { emoji: '🌺', keywords: ['hibiscus', 'flower', 'tropical', 'red'] },
  { emoji: '🌻', keywords: ['sunflower', 'flower', 'yellow', 'sun'] },
  { emoji: '🌷', keywords: ['tulip', 'flower', 'spring', 'pink'] },
  { emoji: '🌹', keywords: ['rose', 'flower', 'red', 'love', 'romance'] },
  { emoji: '🥀', keywords: ['wilted', 'flower', 'dead', 'sad'] },
  { emoji: '💐', keywords: ['bouquet', 'flowers', 'gift', 'arrangement'] },
  { emoji: '🍁', keywords: ['maple', 'leaf', 'fall', 'autumn', 'canada'] },
  { emoji: '🍂', keywords: ['leaves', 'fallen', 'fall', 'autumn'] },
  { emoji: '🍃', keywords: ['leaf', 'wind', 'flutter', 'nature'] },
  { emoji: '🌈', keywords: ['rainbow', 'colors', 'weather', 'pride', 'colorful'] },
  { emoji: '☀️', keywords: ['sun', 'sunny', 'bright', 'weather', 'hot', 'day'] },
  { emoji: '🌤️', keywords: ['sun', 'cloud', 'partly', 'weather'] },
  { emoji: '⛅', keywords: ['sun', 'cloud', 'weather', 'partly'] },
  { emoji: '🌥️', keywords: ['cloud', 'sun', 'behind', 'weather'] },
  { emoji: '☁️', keywords: ['cloud', 'weather', 'sky', 'overcast'] },
  { emoji: '🌦️', keywords: ['rain', 'sun', 'cloud', 'weather'] },
  { emoji: '🌧️', keywords: ['rain', 'cloud', 'weather', 'storm'] },
  { emoji: '⛈️', keywords: ['thunder', 'cloud', 'rain', 'storm', 'lightning'] },
  { emoji: '🌩️', keywords: ['lightning', 'cloud', 'storm', 'thunder'] },
  { emoji: '🌨️', keywords: ['snow', 'cloud', 'weather', 'winter'] },
  { emoji: '❄️', keywords: ['snowflake', 'cold', 'winter', 'snow', 'frozen', 'ice'] },
  { emoji: '🌙', keywords: ['moon', 'crescent', 'night', 'sleep', 'dark'] },
  { emoji: '🌚', keywords: ['moon', 'new', 'face', 'dark'] },
  { emoji: '🌝', keywords: ['moon', 'full', 'face', 'bright'] },
  { emoji: '🌛', keywords: ['moon', 'quarter', 'first', 'face'] },
  { emoji: '🌜', keywords: ['moon', 'quarter', 'last', 'face'] },
  { emoji: '🌕', keywords: ['moon', 'full', 'bright', 'night'] },
  { emoji: '🌊', keywords: ['wave', 'ocean', 'sea', 'water', 'surf'] },
  { emoji: '💧', keywords: ['drop', 'water', 'droplet', 'tear', 'rain'] },
  { emoji: '💦', keywords: ['sweat', 'drops', 'water', 'splash'] },
  { emoji: '🌍', keywords: ['earth', 'globe', 'europe', 'africa', 'world', 'planet'] },
  { emoji: '🌎', keywords: ['earth', 'globe', 'americas', 'world', 'planet'] },
  { emoji: '🌏', keywords: ['earth', 'globe', 'asia', 'australia', 'world', 'planet'] },
  { emoji: '🪐', keywords: ['planet', 'saturn', 'ring', 'space'] },
  { emoji: '🌑', keywords: ['moon', 'new', 'dark', 'night'] },
  { emoji: '🌓', keywords: ['moon', 'quarter', 'first', 'half'] },
  { emoji: '🌔', keywords: ['moon', 'waxing', 'gibbous'] },
  { emoji: '🌖', keywords: ['moon', 'waning', 'gibbous'] },
  { emoji: '🌗', keywords: ['moon', 'quarter', 'last', 'half'] },

  // Animals
  { emoji: '🐶', keywords: ['dog', 'puppy', 'pet', 'face', 'cute'] },
  { emoji: '🐱', keywords: ['cat', 'kitten', 'pet', 'face', 'cute'] },
  { emoji: '🐭', keywords: ['mouse', 'face', 'rodent', 'cute'] },
  { emoji: '🐹', keywords: ['hamster', 'pet', 'face', 'cute'] },
  { emoji: '🐰', keywords: ['rabbit', 'bunny', 'face', 'cute'] },
  { emoji: '🦊', keywords: ['fox', 'face', 'animal', 'clever'] },
  { emoji: '🐻', keywords: ['bear', 'face', 'animal', 'teddy'] },
  { emoji: '🐼', keywords: ['panda', 'bear', 'face', 'cute'] },
  { emoji: '🐨', keywords: ['koala', 'face', 'animal', 'cute'] },
  { emoji: '🐯', keywords: ['tiger', 'face', 'animal', 'cat'] },
  { emoji: '🦁', keywords: ['lion', 'face', 'animal', 'king'] },
  { emoji: '🐮', keywords: ['cow', 'face', 'animal', 'farm'] },
  { emoji: '🐷', keywords: ['pig', 'face', 'animal', 'farm'] },
  { emoji: '🐸', keywords: ['frog', 'face', 'animal', 'amphibian'] },
  { emoji: '🐵', keywords: ['monkey', 'face', 'animal', 'primate'] },
  { emoji: '🙈', keywords: ['monkey', 'see', 'no', 'evil', 'hide'] },
  { emoji: '🙉', keywords: ['monkey', 'hear', 'no', 'evil', 'ignore'] },
  { emoji: '🙊', keywords: ['monkey', 'speak', 'no', 'evil', 'quiet'] },
  { emoji: '🐔', keywords: ['chicken', 'bird', 'farm', 'animal'] },
  { emoji: '🐧', keywords: ['penguin', 'bird', 'cold', 'animal', 'linux'] },
  { emoji: '🐦', keywords: ['bird', 'twitter', 'fly', 'animal'] },
  { emoji: '🐤', keywords: ['chick', 'baby', 'bird', 'cute'] },
  { emoji: '🦆', keywords: ['duck', 'bird', 'animal', 'quack'] },
  { emoji: '🦅', keywords: ['eagle', 'bird', 'fly', 'america'] },
  { emoji: '🦉', keywords: ['owl', 'bird', 'wise', 'night'] },
  { emoji: '🦇', keywords: ['bat', 'animal', 'night', 'halloween'] },
  { emoji: '🐺', keywords: ['wolf', 'face', 'animal', 'wild'] },
  { emoji: '🐗', keywords: ['boar', 'pig', 'wild', 'animal'] },
  { emoji: '🐴', keywords: ['horse', 'face', 'animal', 'ride'] },
  { emoji: '🦄', keywords: ['unicorn', 'face', 'magic', 'fantasy', 'rainbow'] },
  { emoji: '🐝', keywords: ['bee', 'honeybee', 'insect', 'buzz', 'honey'] },
  { emoji: '🐛', keywords: ['bug', 'insect', 'caterpillar', 'worm'] },
  { emoji: '🦋', keywords: ['butterfly', 'insect', 'beautiful', 'nature'] },
  { emoji: '🐌', keywords: ['snail', 'slow', 'shell', 'animal'] },
  { emoji: '🐚', keywords: ['shell', 'spiral', 'beach', 'sea'] },
  { emoji: '🐞', keywords: ['ladybug', 'beetle', 'insect', 'bug', 'luck'] },
  { emoji: '🐜', keywords: ['ant', 'insect', 'bug', 'work'] },
  { emoji: '🦗', keywords: ['cricket', 'insect', 'bug', 'chirp'] },
  { emoji: '🕷️', keywords: ['spider', 'insect', 'web', 'bug', 'creepy'] },
  { emoji: '🕸️', keywords: ['web', 'spider', 'cobweb', 'net'] },
  { emoji: '🐢', keywords: ['turtle', 'slow', 'shell', 'animal'] },
  { emoji: '🐍', keywords: ['snake', 'reptile', 'python', 'animal'] },
  { emoji: '🦎', keywords: ['lizard', 'reptile', 'animal', 'gecko'] },
  { emoji: '🦈', keywords: ['shark', 'fish', 'ocean', 'danger'] },
  { emoji: '🐙', keywords: ['octopus', 'sea', 'animal', 'tentacle'] },
  { emoji: '🐠', keywords: ['fish', 'tropical', 'sea', 'animal'] },
  { emoji: '🐟', keywords: ['fish', 'sea', 'animal', 'swimming'] },
  { emoji: '🐬', keywords: ['dolphin', 'sea', 'animal', 'smart'] },
  { emoji: '🐳', keywords: ['whale', 'sea', 'animal', 'spouting'] },
  { emoji: '🐋', keywords: ['whale', 'sea', 'animal', 'humpback'] },
  { emoji: '🐊', keywords: ['crocodile', 'reptile', 'animal', 'alligator'] },
  { emoji: '🦓', keywords: ['zebra', 'animal', 'stripes', 'africa'] },
  { emoji: '🦒', keywords: ['giraffe', 'animal', 'tall', 'africa'] },
  { emoji: '🦔', keywords: ['hedgehog', 'animal', 'spiny', 'cute'] },
  { emoji: '🦕', keywords: ['dinosaur', 'sauropod', 'extinct', 'jurassic'] },
  { emoji: '🦖', keywords: ['dinosaur', 'trex', 'extinct', 'jurassic'] },

  // People & Faces
  { emoji: '😀', keywords: ['grinning', 'face', 'smile', 'happy'] },
  { emoji: '😃', keywords: ['grinning', 'face', 'big', 'eyes', 'happy', 'smile'] },
  { emoji: '😄', keywords: ['grinning', 'face', 'smiling', 'eyes', 'happy'] },
  { emoji: '😁', keywords: ['beaming', 'face', 'smiling', 'eyes', 'happy', 'grin'] },
  { emoji: '😊', keywords: ['smiling', 'face', 'blush', 'happy', 'warm'] },
  { emoji: '😇', keywords: ['smiling', 'face', 'halo', 'angel', 'innocent'] },
  { emoji: '🙂', keywords: ['slightly', 'smiling', 'face', 'happy'] },
  { emoji: '😉', keywords: ['winking', 'face', 'wink', 'flirt'] },
  { emoji: '😍', keywords: ['smiling', 'face', 'heart', 'eyes', 'love'] },
  { emoji: '🥰', keywords: ['smiling', 'face', 'hearts', 'love', 'adore'] },
  { emoji: '😘', keywords: ['face', 'blowing', 'kiss', 'love'] },
  { emoji: '😎', keywords: ['smiling', 'face', 'sunglasses', 'cool'] },
  { emoji: '🤓', keywords: ['nerd', 'face', 'glasses', 'geek', 'smart'] },
  { emoji: '🧐', keywords: ['face', 'monocle', 'curious', 'thinking'] },
  { emoji: '🤔', keywords: ['thinking', 'face', 'ponder', 'hmm', 'consider'] },
  { emoji: '🤨', keywords: ['face', 'raised', 'eyebrow', 'skeptical'] },
  { emoji: '😐', keywords: ['neutral', 'face', 'meh', 'expressionless'] },
  { emoji: '😑', keywords: ['expressionless', 'face', 'blank', 'meh'] },
  { emoji: '😶', keywords: ['face', 'without', 'mouth', 'silent', 'mute'] },
  { emoji: '🙄', keywords: ['face', 'rolling', 'eyes', 'annoyed'] },
  { emoji: '😏', keywords: ['smirking', 'face', 'smirk', 'sly'] },
  { emoji: '😬', keywords: ['grimacing', 'face', 'awkward', 'nervous'] },
  { emoji: '😌', keywords: ['relieved', 'face', 'calm', 'peaceful'] },
  { emoji: '😔', keywords: ['pensive', 'face', 'sad', 'thoughtful'] },
  { emoji: '😴', keywords: ['sleeping', 'face', 'tired', 'zzz', 'sleep'] },
  { emoji: '🤤', keywords: ['drooling', 'face', 'hungry', 'want'] },
  { emoji: '😷', keywords: ['face', 'mask', 'medical', 'sick', 'covid'] },
  { emoji: '🤒', keywords: ['face', 'thermometer', 'sick', 'fever'] },
  { emoji: '🤕', keywords: ['face', 'bandage', 'head', 'hurt', 'injured'] },
  { emoji: '🤢', keywords: ['nauseated', 'face', 'sick', 'green'] },
  { emoji: '🤮', keywords: ['face', 'vomiting', 'sick', 'gross'] },
  { emoji: '🤧', keywords: ['sneezing', 'face', 'sick', 'cold'] },
  { emoji: '🥵', keywords: ['hot', 'face', 'sweating', 'heat'] },
  { emoji: '🥶', keywords: ['cold', 'face', 'freezing', 'blue'] },
  { emoji: '😵', keywords: ['dizzy', 'face', 'knocked', 'out', 'dead'] },
  { emoji: '🤯', keywords: ['exploding', 'head', 'mind', 'blown', 'shocked'] },
  { emoji: '🤠', keywords: ['cowboy', 'hat', 'face', 'western'] },
  { emoji: '🥳', keywords: ['partying', 'face', 'party', 'celebration', 'birthday'] },
  { emoji: '😈', keywords: ['smiling', 'face', 'horns', 'devil', 'mischievous'] },
  { emoji: '👿', keywords: ['angry', 'face', 'horns', 'devil', 'imp'] },
  { emoji: '👹', keywords: ['ogre', 'monster', 'japanese', 'scary'] },
  { emoji: '👺', keywords: ['goblin', 'monster', 'japanese', 'tengu'] },
  { emoji: '💀', keywords: ['skull', 'dead', 'death', 'skeleton'] },
  { emoji: '☠️', keywords: ['skull', 'crossbones', 'death', 'danger', 'pirate'] },
  { emoji: '👻', keywords: ['ghost', 'halloween', 'spooky', 'scary', 'boo'] },
  { emoji: '👽', keywords: ['alien', 'extraterrestrial', 'ufo', 'space'] },
  { emoji: '👾', keywords: ['alien', 'monster', 'game', 'space', 'invader'] },
  { emoji: '🤖', keywords: ['robot', 'face', 'bot', 'machine', 'ai'] },
  { emoji: '💩', keywords: ['poop', 'poo', 'pile', 'funny', 'turd'] },
  { emoji: '😺', keywords: ['cat', 'grinning', 'face', 'happy'] },
  { emoji: '😸', keywords: ['cat', 'grinning', 'smile', 'face', 'happy'] },
  { emoji: '😹', keywords: ['cat', 'tears', 'joy', 'face', 'laughing'] },
  { emoji: '😻', keywords: ['cat', 'heart', 'eyes', 'face', 'love'] },
  { emoji: '😼', keywords: ['cat', 'wry', 'smile', 'face', 'smirk'] },
  { emoji: '😽', keywords: ['cat', 'kissing', 'face', 'love'] },
  { emoji: '🙀', keywords: ['cat', 'weary', 'face', 'surprised', 'shocked'] },
  { emoji: '😿', keywords: ['cat', 'crying', 'face', 'sad', 'tear'] },
  { emoji: '😾', keywords: ['cat', 'pouting', 'face', 'angry'] },
  { emoji: '👤', keywords: ['user', 'person', 'silhouette', 'bust', 'profile', 'account'] },
  { emoji: '👥', keywords: ['users', 'people', 'silhouettes', 'busts', 'group', 'team'] },
  { emoji: '👶', keywords: ['baby', 'child', 'infant', 'newborn'] },
  { emoji: '🧒', keywords: ['child', 'kid', 'young', 'person'] },
  { emoji: '👧', keywords: ['girl', 'child', 'female', 'young'] },
  { emoji: '👦', keywords: ['boy', 'child', 'male', 'young'] },
  { emoji: '🧑', keywords: ['person', 'adult', 'gender', 'neutral'] },
  { emoji: '👨', keywords: ['man', 'male', 'adult', 'guy'] },
  { emoji: '👩', keywords: ['woman', 'female', 'adult', 'girl'] },
  { emoji: '🧓', keywords: ['older', 'person', 'adult', 'senior'] },
  { emoji: '👴', keywords: ['old', 'man', 'grandpa', 'senior'] },
  { emoji: '👵', keywords: ['old', 'woman', 'grandma', 'senior'] },
  { emoji: '👨‍💻', keywords: ['man', 'technologist', 'coder', 'developer', 'programmer'] },
  { emoji: '👩‍💻', keywords: ['woman', 'technologist', 'coder', 'developer', 'programmer'] },
  { emoji: '🧑‍💻', keywords: ['technologist', 'coder', 'developer', 'programmer', 'person'] },
  { emoji: '👨‍🔧', keywords: ['man', 'mechanic', 'fix', 'repair', 'worker'] },
  { emoji: '👩‍🔧', keywords: ['woman', 'mechanic', 'fix', 'repair', 'worker'] },
  { emoji: '👨‍💼', keywords: ['man', 'office', 'worker', 'business', 'manager'] },
  { emoji: '👩‍💼', keywords: ['woman', 'office', 'worker', 'business', 'manager'] },
  { emoji: '🧑‍💼', keywords: ['office', 'worker', 'business', 'manager', 'person'] },
  { emoji: '👨‍🔬', keywords: ['man', 'scientist', 'research', 'lab', 'chemistry'] },
  { emoji: '👩‍🔬', keywords: ['woman', 'scientist', 'research', 'lab', 'chemistry'] },
  { emoji: '👨‍🎨', keywords: ['man', 'artist', 'painter', 'creative', 'art'] },
  { emoji: '👩‍🎨', keywords: ['woman', 'artist', 'painter', 'creative', 'art'] },
  { emoji: '👨‍🚀', keywords: ['man', 'astronaut', 'space', 'nasa'] },
  { emoji: '👩‍🚀', keywords: ['woman', 'astronaut', 'space', 'nasa'] },
  { emoji: '👨‍🏫', keywords: ['man', 'teacher', 'professor', 'instructor', 'education'] },
  { emoji: '👩‍🏫', keywords: ['woman', 'teacher', 'professor', 'instructor', 'education'] },
  { emoji: '👨‍⚕️', keywords: ['man', 'health', 'doctor', 'nurse', 'medical'] },
  { emoji: '👩‍⚕️', keywords: ['woman', 'health', 'doctor', 'nurse', 'medical'] },
  { emoji: '🤝', keywords: ['handshake', 'agreement', 'deal', 'meeting', 'partner'] },
  { emoji: '💬', keywords: ['speech', 'bubble', 'comment', 'talk', 'chat', 'message'] },
  { emoji: '💭', keywords: ['thought', 'bubble', 'think', 'idea'] },
  { emoji: '🗣️', keywords: ['speaking', 'head', 'talk', 'voice', 'loud'] },
  { emoji: '👁️', keywords: ['eye', 'look', 'see', 'watch', 'view'] },
  { emoji: '👀', keywords: ['eyes', 'look', 'see', 'watch', 'stare'] },
  { emoji: '🧠', keywords: ['brain', 'think', 'smart', 'intelligence', 'mind'] },
  { emoji: '🫀', keywords: ['heart', 'anatomical', 'organ', 'body'] },
  { emoji: '🫁', keywords: ['lungs', 'breathe', 'organ', 'body'] },
  { emoji: '🦴', keywords: ['bone', 'skeleton', 'body', 'anatomy'] },
  { emoji: '🦷', keywords: ['tooth', 'dental', 'teeth', 'dentist'] },
  { emoji: '👂', keywords: ['ear', 'hear', 'listen', 'sound'] },
  { emoji: '👃', keywords: ['nose', 'smell', 'sniff', 'face'] },
  { emoji: '👅', keywords: ['tongue', 'taste', 'lick', 'mouth'] },
  { emoji: '👄', keywords: ['mouth', 'lips', 'kiss', 'speak'] },
  { emoji: '❤️', keywords: ['heart', 'love', 'red', 'like', 'favorite'] },
  { emoji: '🧡', keywords: ['heart', 'orange', 'love', 'like'] },
  { emoji: '💛', keywords: ['heart', 'yellow', 'love', 'like'] },
  { emoji: '💚', keywords: ['heart', 'green', 'love', 'like'] },
  { emoji: '💙', keywords: ['heart', 'blue', 'love', 'like'] },
  { emoji: '💜', keywords: ['heart', 'purple', 'love', 'like'] },
  { emoji: '🖤', keywords: ['heart', 'black', 'love', 'like'] },
  { emoji: '🤍', keywords: ['heart', 'white', 'love', 'like'] },
  { emoji: '🤎', keywords: ['heart', 'brown', 'love', 'like'] },
  { emoji: '💔', keywords: ['broken', 'heart', 'sad', 'love'] },
  { emoji: '❣️', keywords: ['heart', 'exclamation', 'love', 'heavy'] },
  { emoji: '💕', keywords: ['hearts', 'two', 'love', 'couple'] },
  { emoji: '💞', keywords: ['hearts', 'revolving', 'love', 'affection'] },
  { emoji: '💓', keywords: ['heart', 'beating', 'love', 'pulse'] },
  { emoji: '💗', keywords: ['heart', 'growing', 'love', 'pink'] },
  { emoji: '💖', keywords: ['heart', 'sparkling', 'love', 'shiny'] },
  { emoji: '💘', keywords: ['heart', 'arrow', 'cupid', 'love'] },
  { emoji: '💝', keywords: ['heart', 'ribbon', 'gift', 'love'] },
  { emoji: '💟', keywords: ['heart', 'decoration', 'love', 'purple'] },
  { emoji: '💪', keywords: ['flexed', 'biceps', 'strong', 'muscle', 'arm', 'power'] },
  { emoji: '🙌', keywords: ['raising', 'hands', 'celebration', 'hooray', 'praise'] },
  { emoji: '👏', keywords: ['clapping', 'hands', 'applause', 'bravo'] },
  { emoji: '🙏', keywords: ['folded', 'hands', 'pray', 'please', 'thanks', 'hope'] },
  { emoji: '👍', keywords: ['thumbs', 'up', 'yes', 'good', 'approve', 'like'] },
  { emoji: '👎', keywords: ['thumbs', 'down', 'no', 'bad', 'disapprove', 'dislike'] },
  { emoji: '👌', keywords: ['ok', 'hand', 'perfect', 'good', 'nice'] },
  { emoji: '✌️', keywords: ['victory', 'hand', 'peace', 'two', 'sign'] },
  { emoji: '🤞', keywords: ['crossed', 'fingers', 'luck', 'hope', 'wish'] },
  { emoji: '🤙', keywords: ['call', 'me', 'hand', 'shaka', 'hang', 'loose'] },
  { emoji: '👋', keywords: ['waving', 'hand', 'hello', 'goodbye', 'hi', 'bye'] },
  { emoji: '✋', keywords: ['raised', 'hand', 'stop', 'high', 'five'] },
  { emoji: '🖐️', keywords: ['hand', 'splayed', 'fingers', 'five'] },
  { emoji: '🖖', keywords: ['vulcan', 'salute', 'spock', 'star', 'trek'] },
  { emoji: '👆', keywords: ['backhand', 'index', 'pointing', 'up'] },
  { emoji: '👇', keywords: ['backhand', 'index', 'pointing', 'down'] },
  { emoji: '👈', keywords: ['backhand', 'index', 'pointing', 'left'] },
  { emoji: '👉', keywords: ['backhand', 'index', 'pointing', 'right'] },
  { emoji: '☝️', keywords: ['index', 'pointing', 'up', 'one'] },
  { emoji: '✊', keywords: ['raised', 'fist', 'punch', 'power'] },
  { emoji: '👊', keywords: ['fist', 'oncoming', 'punch', 'bump'] },
  { emoji: '🤛', keywords: ['fist', 'left', 'bump', 'facing'] },
  { emoji: '🤜', keywords: ['fist', 'right', 'bump', 'facing'] },
  { emoji: '🤚', keywords: ['raised', 'back', 'hand', 'stop'] },
  { emoji: '👐', keywords: ['open', 'hands', 'hug', 'jazz'] },
  { emoji: '🤲', keywords: ['palms', 'up', 'together', 'cupped'] },
  { emoji: '🫶', keywords: ['heart', 'hands', 'love', 'care'] },

  // Travel & Places
  { emoji: '🚀', keywords: ['rocket', 'launch', 'space', 'ship', 'startup', 'fast'] },
  { emoji: '✈️', keywords: ['airplane', 'plane', 'travel', 'flight', 'fly'] },
  { emoji: '🛫', keywords: ['airplane', 'departure', 'takeoff', 'travel'] },
  { emoji: '🛬', keywords: ['airplane', 'arrival', 'landing', 'travel'] },
  { emoji: '🛩️', keywords: ['airplane', 'small', 'plane', 'private'] },
  { emoji: '💺', keywords: ['seat', 'airplane', 'sit', 'chair'] },
  { emoji: '🚁', keywords: ['helicopter', 'fly', 'aircraft', 'chopper'] },
  { emoji: '🚂', keywords: ['locomotive', 'train', 'steam', 'railway'] },
  { emoji: '🚃', keywords: ['railway', 'car', 'train', 'carriage'] },
  { emoji: '🚄', keywords: ['train', 'high', 'speed', 'bullet', 'fast'] },
  { emoji: '🚅', keywords: ['train', 'bullet', 'speed', 'shinkansen'] },
  { emoji: '🚆', keywords: ['train', 'railway', 'station', 'commute'] },
  { emoji: '🚇', keywords: ['metro', 'subway', 'train', 'underground'] },
  { emoji: '🚈', keywords: ['light', 'rail', 'train', 'tram'] },
  { emoji: '🚊', keywords: ['tram', 'trolley', 'streetcar', 'rail'] },
  { emoji: '🚗', keywords: ['car', 'automobile', 'vehicle', 'drive', 'red'] },
  { emoji: '🚕', keywords: ['taxi', 'cab', 'car', 'yellow', 'ride'] },
  { emoji: '🚌', keywords: ['bus', 'vehicle', 'transport', 'public'] },
  { emoji: '🚎', keywords: ['trolleybus', 'bus', 'electric', 'transport'] },
  { emoji: '🚐', keywords: ['minibus', 'van', 'vehicle', 'transport'] },
  { emoji: '🚑', keywords: ['ambulance', 'emergency', 'hospital', 'medical'] },
  { emoji: '🚒', keywords: ['fire', 'engine', 'truck', 'emergency'] },
  { emoji: '🚓', keywords: ['police', 'car', 'cop', 'patrol'] },
  { emoji: '🚔', keywords: ['police', 'car', 'oncoming', 'patrol'] },
  { emoji: '🚖', keywords: ['taxi', 'oncoming', 'cab', 'car'] },
  { emoji: '🚘', keywords: ['automobile', 'oncoming', 'car', 'vehicle'] },
  { emoji: '🚙', keywords: ['suv', 'sport', 'utility', 'vehicle', 'car'] },
  { emoji: '🚚', keywords: ['truck', 'delivery', 'shipping', 'vehicle'] },
  { emoji: '🚛', keywords: ['truck', 'articulated', 'lorry', 'semi'] },
  { emoji: '🚜', keywords: ['tractor', 'farm', 'vehicle', 'agriculture'] },
  { emoji: '🏎️', keywords: ['racing', 'car', 'formula', 'fast', 'speed'] },
  { emoji: '🏍️', keywords: ['motorcycle', 'racing', 'bike', 'speed'] },
  { emoji: '🛵', keywords: ['scooter', 'motor', 'vespa', 'vehicle'] },
  { emoji: '🚲', keywords: ['bicycle', 'bike', 'cycle', 'pedal'] },
  { emoji: '🛴', keywords: ['scooter', 'kick', 'ride', 'vehicle'] },
  { emoji: '🚏', keywords: ['bus', 'stop', 'station', 'transport'] },
  { emoji: '⛽', keywords: ['fuel', 'pump', 'gas', 'station', 'petrol'] },
  { emoji: '🚧', keywords: ['construction', 'barrier', 'road', 'work'] },
  { emoji: '🚦', keywords: ['traffic', 'light', 'vertical', 'signal'] },
  { emoji: '🚥', keywords: ['traffic', 'light', 'horizontal', 'signal'] },
  { emoji: '⚓', keywords: ['anchor', 'ship', 'boat', 'sea', 'nautical'] },
  { emoji: '⛵', keywords: ['sailboat', 'boat', 'sail', 'sea', 'yacht'] },
  { emoji: '🚤', keywords: ['speedboat', 'boat', 'fast', 'water'] },
  { emoji: '🛥️', keywords: ['motor', 'boat', 'speedboat', 'yacht'] },
  { emoji: '🛳️', keywords: ['passenger', 'ship', 'cruise', 'liner'] },
  { emoji: '⛴️', keywords: ['ferry', 'ship', 'boat', 'transport'] },
  { emoji: '🚢', keywords: ['ship', 'boat', 'cruise', 'ocean', 'liner'] },
  { emoji: '🏠', keywords: ['house', 'home', 'building', 'residence'] },
  { emoji: '🏡', keywords: ['house', 'garden', 'home', 'yard'] },
  { emoji: '🏢', keywords: ['office', 'building', 'work', 'company', 'business'] },
  { emoji: '🏣', keywords: ['post', 'office', 'japanese', 'building'] },
  { emoji: '🏤', keywords: ['post', 'office', 'european', 'building'] },
  { emoji: '🏥', keywords: ['hospital', 'building', 'medical', 'health'] },
  { emoji: '🏨', keywords: ['hotel', 'building', 'accommodation', 'travel'] },
  { emoji: '🏩', keywords: ['love', 'hotel', 'building', 'romance'] },
  { emoji: '🏪', keywords: ['convenience', 'store', 'shop', 'building'] },
  { emoji: '🏫', keywords: ['school', 'building', 'education', 'learn'] },
  { emoji: '🏬', keywords: ['department', 'store', 'mall', 'shopping'] },
  { emoji: '🏭', keywords: ['factory', 'building', 'industry', 'manufacturing'] },
  { emoji: '🏗️', keywords: ['construction', 'building', 'crane', 'work'] },
  { emoji: '🏛️', keywords: ['classical', 'building', 'museum', 'government'] },
  { emoji: '🏰', keywords: ['castle', 'european', 'building', 'medieval'] },
  { emoji: '🏯', keywords: ['castle', 'japanese', 'building', 'traditional'] },
  { emoji: '🗼', keywords: ['tower', 'tokyo', 'building', 'landmark'] },
  { emoji: '🗽', keywords: ['statue', 'liberty', 'landmark', 'new', 'york'] },
  { emoji: '⛪', keywords: ['church', 'building', 'religion', 'christian'] },
  { emoji: '🕌', keywords: ['mosque', 'building', 'religion', 'islam'] },
  { emoji: '🛕', keywords: ['hindu', 'temple', 'building', 'religion'] },
  { emoji: '🕍', keywords: ['synagogue', 'building', 'religion', 'jewish'] },
  { emoji: '⛩️', keywords: ['shinto', 'shrine', 'japan', 'building'] },
  { emoji: '🕋', keywords: ['kaaba', 'mecca', 'islam', 'building'] },
  { emoji: '⛲', keywords: ['fountain', 'park', 'water', 'decoration'] },
  { emoji: '⛺', keywords: ['tent', 'camping', 'outdoor', 'camp'] },
  { emoji: '🌁', keywords: ['foggy', 'bridge', 'san', 'francisco'] },
  { emoji: '🌃', keywords: ['night', 'stars', 'city', 'evening'] },
  { emoji: '🏙️', keywords: ['cityscape', 'city', 'skyline', 'urban'] },
  { emoji: '🌄', keywords: ['sunrise', 'mountains', 'morning', 'dawn'] },
  { emoji: '🌅', keywords: ['sunrise', 'morning', 'dawn', 'sun'] },
  { emoji: '🌆', keywords: ['cityscape', 'dusk', 'sunset', 'evening'] },
  { emoji: '🌇', keywords: ['sunset', 'city', 'evening', 'dusk'] },
  { emoji: '🌉', keywords: ['bridge', 'night', 'city', 'lights'] },
  { emoji: '🎠', keywords: ['carousel', 'horse', 'fair', 'ride'] },
  { emoji: '🎡', keywords: ['ferris', 'wheel', 'fair', 'ride', 'amusement'] },
  { emoji: '🎢', keywords: ['roller', 'coaster', 'fair', 'ride', 'amusement'] },
  { emoji: '🚣', keywords: ['rowboat', 'person', 'rowing', 'boat'] },
  { emoji: '🗺️', keywords: ['map', 'world', 'earth', 'geography', 'explore'] },
  { emoji: '🗻', keywords: ['mount', 'fuji', 'mountain', 'japan'] },
  { emoji: '🏔️', keywords: ['mountain', 'snow', 'capped', 'peak'] },
  { emoji: '⛰️', keywords: ['mountain', 'peak', 'rock', 'climb'] },
  { emoji: '🏕️', keywords: ['camping', 'tent', 'outdoor', 'nature'] },
  { emoji: '🏖️', keywords: ['beach', 'umbrella', 'sand', 'vacation'] },
  { emoji: '🏜️', keywords: ['desert', 'sand', 'hot', 'dry'] },
  { emoji: '🏝️', keywords: ['island', 'desert', 'tropical', 'palm'] },
  { emoji: '🏞️', keywords: ['national', 'park', 'nature', 'scenic'] },

  // Food & Drink
  { emoji: '🍎', keywords: ['apple', 'red', 'fruit', 'healthy'] },
  { emoji: '🍏', keywords: ['apple', 'green', 'fruit', 'healthy'] },
  { emoji: '🍐', keywords: ['pear', 'fruit', 'green', 'healthy'] },
  { emoji: '🍊', keywords: ['orange', 'tangerine', 'fruit', 'citrus'] },
  { emoji: '🍋', keywords: ['lemon', 'citrus', 'fruit', 'yellow', 'sour'] },
  { emoji: '🍌', keywords: ['banana', 'fruit', 'yellow', 'tropical'] },
  { emoji: '🍉', keywords: ['watermelon', 'fruit', 'summer', 'red'] },
  { emoji: '🍇', keywords: ['grapes', 'fruit', 'purple', 'vine'] },
  { emoji: '🍓', keywords: ['strawberry', 'fruit', 'berry', 'red'] },
  { emoji: '🫐', keywords: ['blueberries', 'fruit', 'berry', 'blue'] },
  { emoji: '🍈', keywords: ['melon', 'fruit', 'cantaloupe', 'honeydew'] },
  { emoji: '🍒', keywords: ['cherries', 'fruit', 'red', 'cherry'] },
  { emoji: '🍑', keywords: ['peach', 'fruit', 'orange', 'fuzzy'] },
  { emoji: '🥭', keywords: ['mango', 'fruit', 'tropical', 'orange'] },
  { emoji: '🍍', keywords: ['pineapple', 'fruit', 'tropical', 'yellow'] },
  { emoji: '🥥', keywords: ['coconut', 'fruit', 'tropical', 'palm'] },
  { emoji: '🥝', keywords: ['kiwi', 'fruit', 'green', 'fuzzy'] },
  { emoji: '🍅', keywords: ['tomato', 'vegetable', 'red', 'fruit'] },
  { emoji: '🍆', keywords: ['eggplant', 'aubergine', 'vegetable', 'purple'] },
  { emoji: '🥑', keywords: ['avocado', 'fruit', 'green', 'guacamole'] },
  { emoji: '🥦', keywords: ['broccoli', 'vegetable', 'green', 'healthy'] },
  { emoji: '🥬', keywords: ['leafy', 'green', 'vegetable', 'lettuce'] },
  { emoji: '🥒', keywords: ['cucumber', 'vegetable', 'green', 'pickle'] },
  { emoji: '🌶️', keywords: ['pepper', 'hot', 'chili', 'spicy', 'red'] },
  { emoji: '🫑', keywords: ['pepper', 'bell', 'vegetable', 'green'] },
  { emoji: '🌽', keywords: ['corn', 'ear', 'maize', 'vegetable'] },
  { emoji: '🥕', keywords: ['carrot', 'vegetable', 'orange', 'healthy'] },
  { emoji: '🫒', keywords: ['olive', 'food', 'green', 'oil'] },
  { emoji: '🧄', keywords: ['garlic', 'food', 'flavor', 'cooking'] },
  { emoji: '🧅', keywords: ['onion', 'food', 'flavor', 'cooking'] },
  { emoji: '🥔', keywords: ['potato', 'vegetable', 'food', 'starch'] },
  { emoji: '🍠', keywords: ['sweet', 'potato', 'roasted', 'vegetable'] },
  { emoji: '🥐', keywords: ['croissant', 'bread', 'pastry', 'french'] },
  { emoji: '🥯', keywords: ['bagel', 'bread', 'breakfast', 'food'] },
  { emoji: '🍞', keywords: ['bread', 'loaf', 'toast', 'food'] },
  { emoji: '🥖', keywords: ['baguette', 'bread', 'french', 'food'] },
  { emoji: '🥨', keywords: ['pretzel', 'snack', 'food', 'twisted'] },
  { emoji: '🧀', keywords: ['cheese', 'wedge', 'food', 'dairy'] },
  { emoji: '🥚', keywords: ['egg', 'food', 'breakfast', 'chicken'] },
  { emoji: '🍳', keywords: ['cooking', 'egg', 'frying', 'pan', 'breakfast'] },
  { emoji: '🧈', keywords: ['butter', 'food', 'dairy', 'spread'] },
  { emoji: '🥞', keywords: ['pancakes', 'breakfast', 'food', 'stack'] },
  { emoji: '🧇', keywords: ['waffle', 'breakfast', 'food', 'belgian'] },
  { emoji: '🥓', keywords: ['bacon', 'meat', 'breakfast', 'food'] },
  { emoji: '🥩', keywords: ['steak', 'meat', 'cut', 'beef'] },
  { emoji: '🍗', keywords: ['poultry', 'leg', 'chicken', 'meat'] },
  { emoji: '🍖', keywords: ['meat', 'bone', 'food', 'drumstick'] },
  { emoji: '🌭', keywords: ['hot', 'dog', 'frankfurter', 'food'] },
  { emoji: '🍔', keywords: ['hamburger', 'burger', 'food', 'beef', 'fast'] },
  { emoji: '🍟', keywords: ['fries', 'french', 'food', 'fast', 'potato'] },
  { emoji: '🍕', keywords: ['pizza', 'slice', 'food', 'italian'] },
  { emoji: '🥪', keywords: ['sandwich', 'bread', 'food', 'lunch'] },
  { emoji: '🥙', keywords: ['pita', 'stuffed', 'flatbread', 'food'] },
  { emoji: '🧆', keywords: ['falafel', 'food', 'middle', 'eastern'] },
  { emoji: '🌮', keywords: ['taco', 'mexican', 'food', 'shell'] },
  { emoji: '🌯', keywords: ['burrito', 'mexican', 'food', 'wrap'] },
  { emoji: '🫔', keywords: ['tamale', 'mexican', 'food', 'wrapped'] },
  { emoji: '🥗', keywords: ['salad', 'green', 'food', 'healthy'] },
  { emoji: '🥫', keywords: ['canned', 'food', 'tin', 'preserved'] },
  { emoji: '🍝', keywords: ['spaghetti', 'pasta', 'italian', 'noodles'] },
  { emoji: '🍜', keywords: ['noodles', 'steaming', 'bowl', 'ramen', 'asian'] },
  { emoji: '🍲', keywords: ['pot', 'food', 'stew', 'soup'] },
  { emoji: '🍛', keywords: ['curry', 'rice', 'indian', 'food'] },
  { emoji: '🍣', keywords: ['sushi', 'japanese', 'fish', 'rice'] },
  { emoji: '🍱', keywords: ['bento', 'box', 'japanese', 'lunch'] },
  { emoji: '🥟', keywords: ['dumpling', 'food', 'asian', 'gyoza'] },
  { emoji: '🦪', keywords: ['oyster', 'seafood', 'pearl', 'food'] },
  { emoji: '🍤', keywords: ['shrimp', 'fried', 'tempura', 'seafood'] },
  { emoji: '🍙', keywords: ['rice', 'ball', 'japanese', 'onigiri'] },
  { emoji: '🍚', keywords: ['rice', 'cooked', 'bowl', 'food'] },
  { emoji: '🍘', keywords: ['rice', 'cracker', 'japanese', 'snack'] },
  { emoji: '🍥', keywords: ['fish', 'cake', 'swirl', 'narutomaki'] },
  { emoji: '🥠', keywords: ['fortune', 'cookie', 'chinese', 'dessert'] },
  { emoji: '🥡', keywords: ['takeout', 'box', 'chinese', 'food'] },
  { emoji: '🍦', keywords: ['ice', 'cream', 'soft', 'serve', 'dessert'] },
  { emoji: '🍧', keywords: ['shaved', 'ice', 'dessert', 'sweet'] },
  { emoji: '🍨', keywords: ['ice', 'cream', 'dessert', 'sweet'] },
  { emoji: '🍩', keywords: ['doughnut', 'donut', 'dessert', 'sweet'] },
  { emoji: '🍪', keywords: ['cookie', 'sweet', 'dessert', 'biscuit'] },
  { emoji: '🎂', keywords: ['birthday', 'cake', 'celebration', 'party'] },
  { emoji: '🍰', keywords: ['cake', 'shortcake', 'slice', 'dessert'] },
  { emoji: '🧁', keywords: ['cupcake', 'dessert', 'sweet', 'muffin'] },
  { emoji: '🥧', keywords: ['pie', 'dessert', 'sweet', 'baked'] },
  { emoji: '🍫', keywords: ['chocolate', 'bar', 'sweet', 'candy'] },
  { emoji: '🍬', keywords: ['candy', 'sweet', 'wrapper', 'sugar'] },
  { emoji: '🍭', keywords: ['lollipop', 'candy', 'sweet', 'sugar'] },
  { emoji: '🍮', keywords: ['custard', 'pudding', 'dessert', 'flan'] },
  { emoji: '🍯', keywords: ['honey', 'pot', 'sweet', 'bee'] },
  { emoji: '🍼', keywords: ['baby', 'bottle', 'milk', 'infant'] },
  { emoji: '🥛', keywords: ['milk', 'glass', 'drink', 'dairy'] },
  { emoji: '☕', keywords: ['coffee', 'hot', 'beverage', 'cup', 'cafe'] },
  { emoji: '🫖', keywords: ['teapot', 'tea', 'drink', 'hot'] },
  { emoji: '🍵', keywords: ['tea', 'cup', 'green', 'hot', 'matcha'] },
  { emoji: '🧃', keywords: ['juice', 'beverage', 'box', 'drink'] },
  { emoji: '🥤', keywords: ['cup', 'straw', 'soda', 'drink', 'beverage'] },
  { emoji: '🧋', keywords: ['bubble', 'tea', 'boba', 'drink', 'milk'] },
  { emoji: '🍶', keywords: ['sake', 'bottle', 'cup', 'japanese', 'drink'] },
  { emoji: '🍺', keywords: ['beer', 'mug', 'drink', 'alcohol', 'bar'] },
  { emoji: '🍻', keywords: ['beers', 'clinking', 'mugs', 'cheers', 'toast'] },
  { emoji: '🥂', keywords: ['clinking', 'glasses', 'champagne', 'toast', 'celebrate'] },
  { emoji: '🍷', keywords: ['wine', 'glass', 'drink', 'red', 'alcohol'] },
  { emoji: '🥃', keywords: ['tumbler', 'glass', 'whiskey', 'drink', 'alcohol'] },
  { emoji: '🍸', keywords: ['cocktail', 'glass', 'drink', 'martini', 'alcohol'] },
  { emoji: '🍹', keywords: ['tropical', 'drink', 'cocktail', 'vacation'] },
  { emoji: '🧊', keywords: ['ice', 'cube', 'cold', 'frozen'] },
  { emoji: '🥄', keywords: ['spoon', 'utensil', 'eat', 'tableware'] },
  { emoji: '🍴', keywords: ['fork', 'knife', 'utensils', 'cutlery', 'eat'] },
  { emoji: '🍽️', keywords: ['plate', 'fork', 'knife', 'dinner', 'dining'] },
  { emoji: '🥢', keywords: ['chopsticks', 'utensils', 'asian', 'eat'] },
  { emoji: '🥣', keywords: ['bowl', 'spoon', 'cereal', 'breakfast'] },

  // Activities & Sports
  { emoji: '⚽', keywords: ['soccer', 'ball', 'football', 'sport', 'game'] },
  { emoji: '🏀', keywords: ['basketball', 'ball', 'sport', 'nba', 'game'] },
  { emoji: '🏈', keywords: ['football', 'american', 'ball', 'sport', 'nfl'] },
  { emoji: '⚾', keywords: ['baseball', 'ball', 'sport', 'mlb', 'game'] },
  { emoji: '🥎', keywords: ['softball', 'ball', 'sport', 'game'] },
  { emoji: '🎾', keywords: ['tennis', 'ball', 'sport', 'racket', 'game'] },
  { emoji: '🏐', keywords: ['volleyball', 'ball', 'sport', 'game'] },
  { emoji: '🏉', keywords: ['rugby', 'football', 'ball', 'sport'] },
  { emoji: '🥏', keywords: ['flying', 'disc', 'frisbee', 'sport'] },
  { emoji: '🎱', keywords: ['pool', 'ball', '8', 'billiards', 'game'] },
  { emoji: '🪀', keywords: ['yo-yo', 'toy', 'game', 'play'] },
  { emoji: '🏓', keywords: ['ping', 'pong', 'table', 'tennis', 'paddle'] },
  { emoji: '🏸', keywords: ['badminton', 'racket', 'shuttlecock', 'sport'] },
  { emoji: '🏒', keywords: ['hockey', 'ice', 'stick', 'puck', 'sport'] },
  { emoji: '🏑', keywords: ['hockey', 'field', 'stick', 'sport'] },
  { emoji: '🥍', keywords: ['lacrosse', 'stick', 'ball', 'sport'] },
  { emoji: '🏏', keywords: ['cricket', 'bat', 'ball', 'sport', 'game'] },
  { emoji: '🪃', keywords: ['boomerang', 'australia', 'throw', 'return'] },
  { emoji: '🥅', keywords: ['goal', 'net', 'sport', 'hockey'] },
  { emoji: '⛳', keywords: ['golf', 'flag', 'hole', 'sport', 'course'] },
  { emoji: '🪁', keywords: ['kite', 'fly', 'wind', 'toy'] },
  { emoji: '🏹', keywords: ['bow', 'arrow', 'archery', 'sport'] },
  { emoji: '🎣', keywords: ['fishing', 'pole', 'fish', 'hobby'] },
  { emoji: '🤿', keywords: ['diving', 'mask', 'snorkel', 'swim'] },
  { emoji: '🥊', keywords: ['boxing', 'glove', 'sport', 'fight'] },
  { emoji: '🥋', keywords: ['martial', 'arts', 'uniform', 'karate', 'judo'] },
  { emoji: '🎽', keywords: ['running', 'shirt', 'sash', 'sport'] },
  { emoji: '🛹', keywords: ['skateboard', 'sport', 'board', 'skate'] },
  { emoji: '🛼', keywords: ['roller', 'skate', 'sport', 'skating'] },
  { emoji: '🛷', keywords: ['sled', 'sledge', 'snow', 'winter'] },
  { emoji: '⛷️', keywords: ['skier', 'skiing', 'snow', 'winter', 'sport'] },
  { emoji: '🏂', keywords: ['snowboarder', 'snowboard', 'winter', 'sport'] },
  { emoji: '🏋️', keywords: ['weight', 'lifter', 'lifting', 'gym', 'sport'] },
  { emoji: '🤼', keywords: ['wrestling', 'wrestlers', 'sport', 'fight'] },
  { emoji: '🤸', keywords: ['cartwheel', 'person', 'gymnastics', 'sport'] },
  { emoji: '🤺', keywords: ['fencing', 'person', 'sport', 'sword'] },
  { emoji: '⛹️', keywords: ['basketball', 'person', 'bouncing', 'ball'] },
  { emoji: '🧘', keywords: ['yoga', 'person', 'lotus', 'meditation', 'zen'] },
  { emoji: '🏄', keywords: ['surfing', 'person', 'wave', 'sport', 'beach'] },
  { emoji: '🏊', keywords: ['swimming', 'person', 'pool', 'sport', 'water'] },
  { emoji: '🤽', keywords: ['water', 'polo', 'person', 'sport'] },
  { emoji: '🚴', keywords: ['cycling', 'person', 'biking', 'bicycle', 'sport'] },
  { emoji: '🚵', keywords: ['mountain', 'biking', 'person', 'cycling'] },
  { emoji: '🧗', keywords: ['climbing', 'person', 'rock', 'sport'] },
  { emoji: '🤾', keywords: ['handball', 'person', 'sport', 'ball'] },
  { emoji: '🏌️', keywords: ['golf', 'person', 'golfing', 'sport'] },
  { emoji: '🏇', keywords: ['horse', 'racing', 'jockey', 'sport'] },
  { emoji: '🎮', keywords: ['video', 'game', 'controller', 'gaming', 'play', 'console'] },
  { emoji: '🕹️', keywords: ['joystick', 'game', 'arcade', 'gaming', 'controller'] },
  { emoji: '🎲', keywords: ['dice', 'game', 'die', 'gambling', 'random', 'chance'] },
  { emoji: '♟️', keywords: ['chess', 'pawn', 'game', 'board', 'strategy'] },
  { emoji: '🎳', keywords: ['bowling', 'ball', 'pins', 'sport', 'game'] },
  { emoji: '🎰', keywords: ['slot', 'machine', 'gambling', 'casino', 'jackpot'] },
  { emoji: '🎨', keywords: ['art', 'palette', 'paint', 'artist', 'color', 'creative'] },
  { emoji: '🎭', keywords: ['performing', 'arts', 'theater', 'drama', 'masks'] },
  { emoji: '🎪', keywords: ['circus', 'tent', 'show', 'performance'] },
  { emoji: '🎤', keywords: ['microphone', 'karaoke', 'sing', 'music', 'voice'] },
  { emoji: '🎧', keywords: ['headphone', 'music', 'listen', 'audio', 'podcast'] },
  { emoji: '🎼', keywords: ['musical', 'score', 'music', 'notes', 'sheet'] },
  { emoji: '🎵', keywords: ['musical', 'note', 'music', 'sound', 'song'] },
  { emoji: '🎶', keywords: ['musical', 'notes', 'music', 'sound', 'melody'] },
  { emoji: '🎹', keywords: ['musical', 'keyboard', 'piano', 'music', 'keys'] },
  { emoji: '🥁', keywords: ['drum', 'music', 'drumsticks', 'percussion'] },
  { emoji: '🪘', keywords: ['drum', 'long', 'music', 'percussion'] },
  { emoji: '🎷', keywords: ['saxophone', 'music', 'instrument', 'jazz'] },
  { emoji: '🎺', keywords: ['trumpet', 'music', 'instrument', 'brass'] },
  { emoji: '🎸', keywords: ['guitar', 'music', 'instrument', 'rock'] },
  { emoji: '🪕', keywords: ['banjo', 'music', 'instrument', 'string'] },
  { emoji: '🎻', keywords: ['violin', 'music', 'instrument', 'orchestra'] },
  { emoji: '🪗', keywords: ['accordion', 'music', 'instrument', 'squeeze'] },
  { emoji: '🎬', keywords: ['clapper', 'board', 'movie', 'film', 'action', 'cinema'] },
  { emoji: '🎥', keywords: ['movie', 'camera', 'film', 'cinema', 'video'] },
  { emoji: '📹', keywords: ['video', 'camera', 'record', 'film'] },
  { emoji: '📷', keywords: ['camera', 'photo', 'picture', 'photograph'] },
  { emoji: '📸', keywords: ['camera', 'flash', 'photo', 'picture'] },
  { emoji: '📽️', keywords: ['film', 'projector', 'movie', 'cinema'] },
  { emoji: '🎞️', keywords: ['film', 'frames', 'movie', 'cinema'] },
  { emoji: '🎊', keywords: ['confetti', 'ball', 'celebration', 'party'] },
  { emoji: '🎉', keywords: ['party', 'popper', 'celebration', 'tada', 'congratulations'] },
  { emoji: '🎈', keywords: ['balloon', 'party', 'celebration', 'birthday'] },
  { emoji: '🪅', keywords: ['pinata', 'party', 'celebration', 'candy'] },
  { emoji: '🎄', keywords: ['christmas', 'tree', 'holiday', 'decoration'] },
  { emoji: '🎃', keywords: ['jack', 'lantern', 'halloween', 'pumpkin'] },
  { emoji: '🧨', keywords: ['firecracker', 'dynamite', 'explosive', 'boom'] },
  { emoji: '🎆', keywords: ['fireworks', 'celebration', 'night', 'festival'] },
  { emoji: '🎇', keywords: ['sparkler', 'fireworks', 'celebration', 'night'] },
  { emoji: '🧧', keywords: ['red', 'envelope', 'gift', 'chinese', 'lucky'] },
  { emoji: '🎐', keywords: ['wind', 'chime', 'bell', 'decoration'] },
  { emoji: '🎑', keywords: ['moon', 'viewing', 'ceremony', 'festival'] },
  { emoji: '🎋', keywords: ['tanabata', 'tree', 'banner', 'japanese'] },
  { emoji: '🎍', keywords: ['pine', 'decoration', 'bamboo', 'japanese'] },
  { emoji: '🎎', keywords: ['dolls', 'japanese', 'festival', 'hinamatsuri'] },
  { emoji: '🎏', keywords: ['carp', 'streamer', 'japanese', 'koinobori'] },
  { emoji: '🎟️', keywords: ['ticket', 'admission', 'event', 'pass'] },
  { emoji: '🎫', keywords: ['ticket', 'admission', 'event', 'pass'] },
];

// Helper to deduplicate emoji arrays
const uniqueEmojis = (emojis: string[]) => [...new Set(emojis)];

// Icon categories with better organization
const iconCategories = {
  suggested: {
    label: 'Suggested',
    icons: ['📄', '📝', '📋', '📁', '📚', '💡', '⚙️', '🚀', '✅', '📌', '🔗', '💻', '🎯', '⭐', '🔒']
  },
  documents: {
    label: 'Documents',
    icons: uniqueEmojis(emojiDatabase.filter(e =>
      e.keywords.some(k => ['document', 'file', 'book', 'note', 'paper', 'folder', 'mail', 'email', 'card', 'calendar'].includes(k))
    ).map(e => e.emoji))
  },
  objects: {
    label: 'Objects',
    icons: uniqueEmojis(emojiDatabase.filter(e =>
      e.keywords.some(k => ['tool', 'computer', 'phone', 'device', 'light', 'key', 'lock', 'bell', 'clock', 'battery'].includes(k))
    ).map(e => e.emoji))
  },
  symbols: {
    label: 'Symbols',
    icons: uniqueEmojis(emojiDatabase.filter(e =>
      e.keywords.some(k => ['check', 'cross', 'warning', 'question', 'exclamation', 'arrow', 'play', 'stop', 'plus', 'minus', 'star', 'heart'].includes(k))
    ).map(e => e.emoji))
  },
  nature: {
    label: 'Nature',
    icons: uniqueEmojis(emojiDatabase.filter(e =>
      e.keywords.some(k => ['plant', 'tree', 'flower', 'leaf', 'sun', 'moon', 'weather', 'cloud', 'rain', 'snow', 'earth', 'ocean', 'water'].includes(k))
    ).map(e => e.emoji))
  },
  animals: {
    label: 'Animals',
    icons: uniqueEmojis(emojiDatabase.filter(e =>
      e.keywords.some(k => ['dog', 'cat', 'bird', 'fish', 'animal', 'pet', 'bear', 'monkey', 'insect', 'bug'].includes(k))
    ).map(e => e.emoji))
  },
  people: {
    label: 'People',
    icons: uniqueEmojis(emojiDatabase.filter(e =>
      e.keywords.some(k => ['face', 'person', 'user', 'people', 'hand', 'heart', 'love', 'smile', 'happy', 'think'].includes(k))
    ).map(e => e.emoji))
  },
  travel: {
    label: 'Travel',
    icons: uniqueEmojis(emojiDatabase.filter(e =>
      e.keywords.some(k => ['car', 'plane', 'train', 'ship', 'building', 'house', 'city', 'rocket', 'travel', 'transport'].includes(k))
    ).map(e => e.emoji))
  },
  food: {
    label: 'Food',
    icons: uniqueEmojis(emojiDatabase.filter(e =>
      e.keywords.some(k => ['food', 'fruit', 'vegetable', 'drink', 'coffee', 'eat', 'meal', 'dessert', 'sweet'].includes(k))
    ).map(e => e.emoji))
  },
  activities: {
    label: 'Activities',
    icons: uniqueEmojis(emojiDatabase.filter(e =>
      e.keywords.some(k => ['sport', 'game', 'music', 'art', 'party', 'celebration', 'play', 'ball', 'camera', 'movie'].includes(k))
    ).map(e => e.emoji))
  }
};

// Flatten all icons for random selection
const allIcons = computed(() => {
  return emojiDatabase.map(e => e.emoji);
});

// Filtered icons based on search
const filteredIcons = computed(() => {
  const query = searchQuery.value.trim().toLowerCase();

  if (!query) {
    return iconCategories[activeCategory.value as keyof typeof iconCategories]?.icons || [];
  }

  // Search through emoji database by keywords
  const matches = emojiDatabase.filter(e =>
    e.keywords.some(keyword => keyword.includes(query))
  );

  return matches.map(e => e.emoji);
});

// Size classes
const sizeClasses = computed(() => {
  switch (props.size) {
    case 'sm': return { button: 'w-7 h-7 text-base', icon: 'text-base' };
    case 'lg': return { button: 'w-12 h-12 text-3xl', icon: 'text-3xl' };
    default: return { button: 'w-9 h-9 text-xl', icon: 'text-xl' };
  }
});

// Handle icon selection
const selectIcon = (icon: string) => {
  currentIcon.value = icon;
  emit('update:icon', icon);
  showDropdown.value = false;
  searchQuery.value = '';
};

// Toggle dropdown
const toggleDropdown = () => {
  showDropdown.value = !showDropdown.value;
  if (showDropdown.value) {
    searchQuery.value = '';
    activeCategory.value = 'suggested';
  }
};

// Close dropdown when clicking outside (but not if we're dragging)
const handleClickOutside = (event: MouseEvent) => {
  // Don't close if we were dragging
  if (hasDragged.value) return;

  if (
    dropdownRef.value &&
    triggerRef.value &&
    !dropdownRef.value.contains(event.target as Node) &&
    !triggerRef.value.contains(event.target as Node)
  ) {
    showDropdown.value = false;
  }
};

// Close on escape
const handleKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Escape') {
    showDropdown.value = false;
  }
};

// Track if actual dragging occurred (vs just a click)
const hasDragged = ref(false);

// Drag-to-scroll handlers for category tabs
const handleMouseDown = (e: MouseEvent) => {
  if (!categoryTabsRef.value) return;
  isDragging.value = true;
  hasDragged.value = false;
  startX.value = e.clientX;
  scrollLeft.value = categoryTabsRef.value.scrollLeft;
  categoryTabsRef.value.style.cursor = 'grabbing';

  // Add global listeners for mouseup outside container
  document.addEventListener('mouseup', handleGlobalMouseUp);
  document.addEventListener('mousemove', handleGlobalMouseMove);
};

const handleGlobalMouseUp = () => {
  if (isDragging.value) {
    isDragging.value = false;
    if (categoryTabsRef.value) {
      categoryTabsRef.value.style.cursor = 'grab';
    }
  }
  document.removeEventListener('mouseup', handleGlobalMouseUp);
  document.removeEventListener('mousemove', handleGlobalMouseMove);

  // Reset hasDragged after a short delay
  setTimeout(() => {
    hasDragged.value = false;
  }, 0);
};

const handleGlobalMouseMove = (e: MouseEvent) => {
  if (!isDragging.value || !categoryTabsRef.value) return;
  e.preventDefault();
  const walk = startX.value - e.clientX;
  // Only mark as dragged if moved more than a few pixels
  if (Math.abs(walk) > 3) {
    hasDragged.value = true;
  }
  categoryTabsRef.value.scrollLeft = scrollLeft.value + walk;
};

// Handle wheel scrolling on category tabs (both vertical and horizontal wheel)
const handleWheel = (e: WheelEvent) => {
  if (!categoryTabsRef.value || !isOverflowing.value) return;
  e.preventDefault();
  // Use deltaY for vertical scroll wheels, deltaX for horizontal (trackpads)
  const delta = e.deltaY !== 0 ? e.deltaY : e.deltaX;
  categoryTabsRef.value.scrollLeft += delta;
};

// Click on dot to scroll to position
const scrollToDot = (dotIndex: number) => {
  if (!categoryTabsRef.value) return;
  const { scrollWidth, clientWidth } = categoryTabsRef.value;
  const maxScroll = scrollWidth - clientWidth;
  const targetScroll = (dotIndex / (DOT_COUNT - 1)) * maxScroll;
  categoryTabsRef.value.scrollTo({ left: targetScroll, behavior: 'smooth' });
};

onMounted(() => {
  document.addEventListener('click', handleClickOutside);
  document.addEventListener('keydown', handleKeydown);
});

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside);
  document.removeEventListener('keydown', handleKeydown);
  document.removeEventListener('mouseup', handleGlobalMouseUp);
  document.removeEventListener('mousemove', handleGlobalMouseMove);
});
</script>

<template>
  <div class="relative inline-block">
    <!-- Trigger button -->
    <button
      ref="triggerRef"
      @click="toggleDropdown"
      class="flex items-center justify-center rounded-lg transition-all duration-150 hover:bg-surface-hover active:scale-95 focus:outline-none focus:ring-2 focus:ring-accent/50"
      :class="sizeClasses.button"
      aria-label="Select document icon"
      type="button"
    >
      <span class="select-none" :class="sizeClasses.icon">{{ currentIcon }}</span>
    </button>

    <!-- Dropdown panel -->
    <Transition
      enter-active-class="transition duration-150 ease-out"
      enter-from-class="opacity-0 scale-95 translate-y-1"
      enter-to-class="opacity-100 scale-100 translate-y-0"
      leave-active-class="transition duration-100 ease-in"
      leave-from-class="opacity-100 scale-100 translate-y-0"
      leave-to-class="opacity-0 scale-95 translate-y-1"
    >
      <div
        v-if="showDropdown"
        ref="dropdownRef"
        class="absolute left-0 top-full mt-2 z-50 w-80 bg-surface border border-default rounded-xl shadow-xl overflow-hidden"
      >
        <!-- Search input -->
        <div class="p-3 border-b border-default">
          <div class="relative">
            <svg class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
            </svg>
            <input
              v-model="searchQuery"
              type="text"
              placeholder="Search icons..."
              class="w-full pl-10 pr-4 py-2 text-sm bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent/50 focus:border-accent"
              @click.stop
            />
          </div>
        </div>

        <!-- Category tabs (hidden when searching) -->
        <div v-if="!searchQuery" class="relative border-b border-default">
          <!-- Left fade indicator -->
          <div
            class="absolute left-0 top-0 bottom-0 w-6 bg-gradient-to-r from-surface to-transparent pointer-events-none z-10 transition-opacity duration-200"
            :class="canScrollLeft ? 'opacity-100' : 'opacity-0'"
          />

          <div
            ref="categoryTabsRef"
            class="category-tabs flex gap-1 px-3 py-2 overflow-x-auto cursor-grab select-none"
            @mousedown="handleMouseDown"
            @wheel="handleWheel"
          >
            <button
              v-for="(category, key) in iconCategories"
              :key="key"
              @click.stop="!hasDragged && (activeCategory = key)"
              class="px-3 py-1.5 text-xs font-medium rounded-md whitespace-nowrap transition-colors flex-shrink-0"
              :class="activeCategory === key
                ? 'bg-accent text-white'
                : 'text-secondary hover:text-primary hover:bg-surface-hover'"
            >
              {{ category.label }}
            </button>
          </div>

          <!-- Right fade indicator -->
          <div
            class="absolute right-0 top-0 bottom-0 w-6 bg-gradient-to-l from-surface to-transparent pointer-events-none z-10 transition-opacity duration-200"
            :class="canScrollRight ? 'opacity-100' : 'opacity-0'"
          />

          <!-- Scroll hint dots -->
          <div v-if="isOverflowing" class="flex justify-center gap-1.5 py-1.5 bg-surface-alt">
            <button
              v-for="i in DOT_COUNT"
              :key="i"
              type="button"
              class="w-1.5 h-1.5 p-0 border-0 rounded-full bg-tertiary transition-all duration-200 cursor-pointer hover:scale-125"
              :class="(i - 1) === activeDotIndex ? 'opacity-100' : 'opacity-30 hover:opacity-60'"
              @click.stop="scrollToDot(i - 1)"
              :aria-label="`Scroll to section ${i}`"
            />
          </div>
        </div>

        <!-- Icons grid -->
        <div class="p-3 max-h-64 overflow-y-auto">
          <div v-if="searchQuery && filteredIcons.length === 0" class="py-8 text-center text-tertiary text-sm">
            No icons found
          </div>
          <div v-else class="grid grid-cols-8 gap-1">
            <button
              v-for="icon in filteredIcons"
              :key="icon"
              @click.stop="selectIcon(icon)"
              class="flex items-center justify-center w-8 h-8 text-xl rounded-md transition-all duration-100 hover:bg-surface-hover hover:scale-110 active:scale-95"
              :class="currentIcon === icon ? 'bg-accent/20 ring-2 ring-accent' : ''"
            >
              <span class="select-none">{{ icon }}</span>
            </button>
          </div>
        </div>

        <!-- Footer with random button -->
        <div class="px-3 py-2 border-t border-default bg-surface-alt flex items-center justify-between">
          <span class="text-xs text-tertiary">Click an icon to select</span>
          <button
            @click.stop="selectIcon(allIcons[Math.floor(Math.random() * allIcons.length)])"
            class="px-2 py-1 text-xs font-medium text-secondary hover:text-primary hover:bg-surface-hover rounded transition-colors"
          >
            Random
          </button>
        </div>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.scrollbar-hide {
  -ms-overflow-style: none;
  scrollbar-width: none;
}
.scrollbar-hide::-webkit-scrollbar {
  display: none;
}

/* Hide scrollbar on category tabs */
.category-tabs {
  -ms-overflow-style: none;
  scrollbar-width: none;
}
.category-tabs::-webkit-scrollbar {
  display: none;
}

/* Buttons inside category tabs should show pointer cursor */
.category-tabs button {
  cursor: pointer;
}
</style>
