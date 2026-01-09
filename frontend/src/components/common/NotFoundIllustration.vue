<script setup lang="ts">
/**
 * NotFoundIllustration - A randomized, humorous "not found" display
 * Shows a different fun illustration and message each time
 */
import { computed } from 'vue';

const props = withDefaults(defineProps<{
  seed?: number;
}>(), {
  seed: undefined,
});

interface NotFoundOption {
  id: string;
  title: string;
  message: string;
  svg: string;
}

const options: NotFoundOption[] = [
  {
    id: 'picture-frame',
    title: 'Nothing to See Here',
    message: 'The frame is empty. The art has departed.',
    svg: `<svg class="w-32 h-32" viewBox="0 0 100 100" fill="none" xmlns="http://www.w3.org/2000/svg">
      <g stroke="currentColor" stroke-width="2" stroke-linecap="round">
        <!-- Outer frame -->
        <rect x="15" y="25" width="70" height="50" rx="3" opacity="0.5"/>
        <!-- Inner frame (empty space) -->
        <rect x="23" y="33" width="54" height="34" rx="2" opacity="0.2" stroke-dasharray="6 4"/>
        <!-- Hanging wire -->
        <path d="M35 25 L50 15 L65 25" opacity="0.3" fill="none"/>
        <!-- Nail -->
        <circle cx="50" cy="12" r="2.5" fill="currentColor" opacity="0.25"/>
      </g>
    </svg>`,
  },
  {
    id: 'cat',
    title: 'Knocked Off the Table',
    message: 'Something pushed this ticket over the edge.',
    svg: `<svg class="w-32 h-32" viewBox="0 0 100 100" fill="none" xmlns="http://www.w3.org/2000/svg">
      <g stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <!-- Table surface -->
        <path d="M5 50 H65" opacity="0.4"/>
        <path d="M65 50 V62" opacity="0.4"/>
        <!-- Cat body sitting on table -->
        <g opacity="0.35">
          <!-- Body -->
          <ellipse cx="35" cy="42" rx="12" ry="8"/>
          <!-- Head -->
          <circle cx="50" cy="34" r="8"/>
          <!-- Ears - short and wide triangles -->
          <path d="M44 27 L42 22 L48 27 Z" fill="currentColor" stroke="none"/>
          <path d="M52 27 L58 22 L56 27 Z" fill="currentColor" stroke="none"/>
          <!-- Tail curving down -->
          <path d="M23 42 Q15 45 18 55 Q20 60 15 62" fill="none" stroke-width="3"/>
        </g>
        <!-- Motion lines from above ticket -->
        <path d="M68 52 L72 58" opacity="0.15" stroke-dasharray="2 3"/>
        <path d="M72 50 L78 56" opacity="0.12" stroke-dasharray="2 3"/>
        <path d="M76 48 L82 54" opacity="0.1" stroke-dasharray="2 3"/>
        <!-- Falling document -->
        <g transform="rotate(25 78 72)">
          <rect x="70" y="60" width="16" height="22" rx="2" opacity="0.25" stroke-dasharray="4 3"/>
          <path d="M74 67 H82 M74 72 H80 M74 77 H78" opacity="0.15"/>
        </g>
      </g>
    </svg>`,
  },
  {
    id: 'another-castle',
    title: 'Thank You Mario!',
    message: 'But our ticket is in another castle.',
    svg: `<svg class="w-32 h-32" viewBox="0 0 100 100" fill="none" xmlns="http://www.w3.org/2000/svg">
      <g stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <!-- Castle base -->
        <rect x="20" y="55" width="60" height="30" opacity="0.3"/>
        <!-- Castle gate -->
        <path d="M42 85 V70 Q50 62 58 70 V85" opacity="0.25"/>
        <!-- Left tower -->
        <rect x="15" y="40" width="18" height="45" opacity="0.25"/>
        <!-- Left battlements -->
        <path d="M15 40 H18 V35 H21 V40 H27 V35 H30 V40 H33" opacity="0.2" fill="none"/>
        <!-- Right tower -->
        <rect x="67" y="40" width="18" height="45" opacity="0.25"/>
        <!-- Right battlements -->
        <path d="M67 40 H70 V35 H73 V40 H79 V35 H82 V40 H85" opacity="0.2" fill="none"/>
        <!-- Center tower -->
        <rect x="40" y="28" width="20" height="27" opacity="0.35"/>
        <!-- Center battlements -->
        <path d="M40 28 H43 V23 H47 V28 H53 V23 H57 V28 H60" opacity="0.25" fill="none"/>
        <!-- Flag pole -->
        <line x1="50" y1="23" x2="50" y2="12" opacity="0.3"/>
        <!-- Flag -->
        <path d="M50 12 L60 16 L50 20" opacity="0.25" fill="currentColor" stroke="none"/>
      </g>
    </svg>`,
  },
  {
    id: 'spinning-chair',
    title: 'Nobody Here',
    message: 'Just an empty chair. No ticket in sight.',
    svg: `<svg class="w-32 h-32" viewBox="0 0 100 100" fill="none" xmlns="http://www.w3.org/2000/svg">
      <g stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <!-- Chair back -->
        <rect x="35" y="20" width="30" height="28" rx="3" opacity="0.3"/>
        <!-- Seat -->
        <ellipse cx="50" cy="52" rx="20" ry="6" opacity="0.35"/>
        <!-- Center pole -->
        <line x1="50" y1="58" x2="50" y2="72" opacity="0.3" stroke-width="3"/>
        <!-- Wheel base (3 visible legs) -->
        <g opacity="0.25">
          <line x1="50" y1="72" x2="35" y2="88"/>
          <line x1="50" y1="72" x2="65" y2="88"/>
          <line x1="50" y1="72" x2="50" y2="90"/>
          <!-- Wheels -->
          <circle cx="35" cy="88" r="3" fill="currentColor"/>
          <circle cx="65" cy="88" r="3" fill="currentColor"/>
          <circle cx="50" cy="90" r="3" fill="currentColor"/>
        </g>
      </g>
    </svg>`,
  },
  {
    id: 'ufo',
    title: 'Abducted',
    message: 'This ticket was taken. You saw nothing.',
    svg: `<svg class="w-32 h-32" viewBox="0 0 100 100" fill="none" xmlns="http://www.w3.org/2000/svg">
      <g stroke="currentColor" stroke-width="2" stroke-linecap="round">
        <!-- Stars -->
        <circle cx="12" cy="18" r="1.5" fill="currentColor" opacity="0.2"/>
        <circle cx="88" cy="12" r="1" fill="currentColor" opacity="0.15"/>
        <circle cx="22" cy="8" r="1" fill="currentColor" opacity="0.1"/>
        <circle cx="82" cy="28" r="1.5" fill="currentColor" opacity="0.12"/>
        <!-- UFO body (saucer) -->
        <ellipse cx="50" cy="30" rx="30" ry="8" opacity="0.4"/>
        <!-- UFO dome -->
        <path d="M35 30 Q35 18 50 16 Q65 18 65 30" opacity="0.3"/>
        <!-- UFO lights -->
        <circle cx="35" cy="30" r="2.5" fill="currentColor" opacity="0.2"/>
        <circle cx="50" cy="32" r="2.5" fill="currentColor" opacity="0.25"/>
        <circle cx="65" cy="30" r="2.5" fill="currentColor" opacity="0.2"/>
        <!-- Tractor beam -->
        <path d="M38 38 L30 85" opacity="0.12"/>
        <path d="M62 38 L70 85" opacity="0.12"/>
        <!-- Beam glow -->
        <path d="M30 85 L70 85" opacity="0.08"/>
        <!-- Document being lifted -->
        <g transform="translate(42 60)" opacity="0.2">
          <rect width="16" height="20" rx="2" stroke-dasharray="3 2"/>
          <path d="M4 6 H12 M4 10 H10 M4 14 H8" opacity="0.5" stroke-width="1"/>
        </g>
      </g>
    </svg>`,
  },
  {
    id: 'existential',
    title: 'Does Anything Exist?',
    message: 'If a ticket is not observed, was it ever really there?',
    svg: `<svg class="w-32 h-32" viewBox="0 0 100 100" fill="none" xmlns="http://www.w3.org/2000/svg">
      <g stroke="currentColor" stroke-linecap="round">
        <!-- Expanding ripples -->
        <circle cx="50" cy="50" r="42" stroke-width="1" stroke-dasharray="3 8" opacity="0.1"/>
        <circle cx="50" cy="50" r="34" stroke-width="1" stroke-dasharray="3 7" opacity="0.15"/>
        <circle cx="50" cy="50" r="26" stroke-width="1.5" stroke-dasharray="3 6" opacity="0.2"/>
        <circle cx="50" cy="50" r="18" stroke-width="1.5" stroke-dasharray="2 5" opacity="0.25"/>
        <circle cx="50" cy="50" r="10" stroke-width="2" stroke-dasharray="2 4" opacity="0.3"/>
        <circle cx="50" cy="50" r="4" stroke-width="2" opacity="0.35"/>
        <!-- Center void -->
        <circle cx="50" cy="50" r="1.5" fill="currentColor" opacity="0.5"/>
      </g>
    </svg>`,
  },
  {
    id: 'gone-fishing',
    title: 'Gone Fishing',
    message: 'Out of office. Try the lake.',
    svg: `<svg class="w-32 h-32" viewBox="0 0 100 100" fill="none" xmlns="http://www.w3.org/2000/svg">
      <g stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <!-- Rod handle (cork grip) -->
        <path d="M12 82 L18 72" opacity="0.45" stroke-width="5"/>
        <!-- Curved rod body -->
        <path d="M18 72 Q30 55 45 38 Q60 22 82 12" opacity="0.35" fill="none" stroke-width="2"/>
        <!-- Fishing line to bobber -->
        <path d="M82 12 Q75 35 65 55" opacity="0.18" fill="none" stroke-width="1"/>
        <!-- Bobber -->
        <ellipse cx="65" cy="60" rx="4" ry="6" fill="currentColor" opacity="0.3"/>
        <line x1="65" y1="54" x2="65" y2="50" opacity="0.2" stroke-width="1"/>
        <!-- Water ripples -->
        <circle cx="65" cy="65" r="8" opacity="0.15" fill="none"/>
        <circle cx="65" cy="65" r="14" opacity="0.1" fill="none"/>
        <circle cx="65" cy="65" r="20" opacity="0.06" fill="none"/>
      </g>
    </svg>`,
  },
  {
    id: 'touch-grass',
    title: 'Go Outside',
    message: 'This ticket is not here. Have you tried looking outside?',
    svg: `<svg class="w-32 h-32" viewBox="0 0 100 100" fill="none" xmlns="http://www.w3.org/2000/svg">
      <g stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <!-- Ground -->
        <line x1="10" y1="85" x2="90" y2="85" opacity="0.2"/>
        <!-- Left tree -->
        <line x1="25" y1="85" x2="25" y2="65" opacity="0.3" stroke-width="3"/>
        <path d="M25 30 L15 55 L20 55 L12 70 L38 70 L30 55 L35 55 Z" opacity="0.25" fill="currentColor" stroke="none"/>
        <!-- Center tree (taller) -->
        <line x1="50" y1="85" x2="50" y2="55" opacity="0.35" stroke-width="4"/>
        <path d="M50 18 L38 45 L44 45 L35 60 L65 60 L56 45 L62 45 Z" opacity="0.3" fill="currentColor" stroke="none"/>
        <!-- Right tree -->
        <line x1="75" y1="85" x2="75" y2="68" opacity="0.3" stroke-width="3"/>
        <path d="M75 38 L67 58 L71 58 L65 72 L85 72 L79 58 L83 58 Z" opacity="0.25" fill="currentColor" stroke="none"/>
      </g>
    </svg>`,
  },
  {
    id: 'void',
    title: 'Into the Void',
    message: 'This ticket gazed into the abyss. The abyss won.',
    svg: `<svg class="w-32 h-32" viewBox="0 0 100 100" fill="none" xmlns="http://www.w3.org/2000/svg">
      <g stroke="currentColor" stroke-linecap="round">
        <!-- Black hole rings -->
        <circle cx="50" cy="50" r="40" stroke-width="1" opacity="0.08"/>
        <circle cx="50" cy="50" r="32" stroke-width="1.5" opacity="0.12"/>
        <circle cx="50" cy="50" r="24" stroke-width="2" opacity="0.18"/>
        <circle cx="50" cy="50" r="16" stroke-width="2" opacity="0.25"/>
        <circle cx="50" cy="50" r="8" stroke-width="2" opacity="0.35"/>
        <!-- Event horizon -->
        <circle cx="50" cy="50" r="3" fill="currentColor" opacity="0.6"/>
        <!-- Document being pulled in -->
        <g transform="rotate(25 72 28)">
          <rect x="65" y="18" width="14" height="18" rx="2" stroke-width="1.5" opacity="0.2" stroke-dasharray="3 2"/>
        </g>
        <!-- Spiral trajectory -->
        <path d="M72 32 Q65 38 60 44 Q56 48 52 50" stroke-width="1" stroke-dasharray="2 4" opacity="0.12"/>
      </g>
    </svg>`,
  },
  {
    id: 'wind',
    title: 'Blown Away',
    message: 'The wind took this one. It happens.',
    svg: `<svg class="w-32 h-32" viewBox="0 0 100 100" fill="none" xmlns="http://www.w3.org/2000/svg">
      <g stroke="currentColor" stroke-width="2" stroke-linecap="round">
        <!-- Wind swooshes -->
        <path d="M8 30 Q25 30 35 26 Q45 22 42 30 Q40 36 50 34" opacity="0.25"/>
        <path d="M5 48 Q30 48 45 42 Q58 36 55 46 Q52 54 65 50" opacity="0.3"/>
        <path d="M12 65 Q35 65 48 60 Q60 55 58 63 Q56 70 70 66" opacity="0.22"/>
        <!-- Dust particles -->
        <g fill="currentColor" stroke="none">
          <circle cx="72" cy="35" r="1.5" opacity="0.12"/>
          <circle cx="80" cy="45" r="2" opacity="0.18"/>
          <circle cx="75" cy="55" r="1" opacity="0.1"/>
          <circle cx="85" cy="52" r="1.5" opacity="0.15"/>
          <circle cx="78" cy="68" r="1" opacity="0.08"/>
        </g>
        <!-- Document blowing away -->
        <g transform="rotate(-20 82 58)">
          <rect x="75" y="48" width="14" height="18" rx="2" opacity="0.2" stroke-dasharray="4 3"/>
          <path d="M79 54 H85 M79 58 H84 M79 62 H82" opacity="0.1" stroke-width="1"/>
        </g>
      </g>
    </svg>`,
  },
];

// Pick a random option (or use seed for consistency)
const selectedOption = computed(() => {
  const index = props.seed !== undefined
    ? props.seed % options.length
    : Math.floor(Math.random() * options.length);
  return options[index];
});
</script>

<template>
  <div class="flex flex-col items-center gap-4">
    <div v-html="selectedOption.svg"></div>
    <h2 class="text-xl font-semibold text-primary">{{ selectedOption.title }}</h2>
    <p class="text-secondary text-center max-w-sm">{{ selectedOption.message }}</p>
  </div>
</template>
