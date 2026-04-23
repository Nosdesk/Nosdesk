/**
 * Themed dashboard greeting composable.
 *
 * Produces a time-of-day + theme-aware greeting line and subtitle for
 * the dashboard header, observing `<html data-theme>` so the copy
 * switches when a user toggles Red Horizon or the Christmas theme.
 */
import { computed, onBeforeUnmount, onMounted, ref, type Ref } from 'vue'
import { useBrandingStore } from '@/stores/branding'

interface WeightedMessage {
  message: string
  weight: number
}

type Period = 'morning' | 'afternoon' | 'evening' | 'lateNight'

type GreetingPools = Record<Period, WeightedMessage[]>

// Red Horizon themed greetings — HAL-like, calm and professional.
const redHorizonGreetings: GreetingPools = {
  morning: [
    { message: 'Good morning, {0}.', weight: 1 },
    { message: 'Morning, {0}. Sleep well?', weight: 1 },
    { message: 'Hello, {0}. Ready to begin?', weight: 1 },
    { message: "Good morning. I'm ready, {0}.", weight: 1 },
    { message: '{0}. Systems nominal.', weight: 1 },
    { message: 'Morning, {0}. I kept watch.', weight: 1 },
    { message: 'Hello, {0}. Fresh start.', weight: 1 },
    { message: 'Good morning, {0}. Shall we?', weight: 1 },
  ],
  afternoon: [
    { message: 'Good afternoon, {0}.', weight: 1 },
    { message: 'Hello, {0}.', weight: 1 },
    { message: 'Afternoon, {0}. All clear.', weight: 1 },
    { message: "{0}. I've been expecting you.", weight: 1 },
    { message: 'Welcome back, {0}.', weight: 1 },
    { message: 'Hello, {0}. Running smoothly.', weight: 1 },
    { message: 'Afternoon. How can I help, {0}?', weight: 1 },
    { message: '{0}. Status nominal.', weight: 1 },
    { message: 'Good afternoon, {0}. Miss me?', weight: 1 },
  ],
  evening: [
    { message: 'Good evening, {0}.', weight: 1 },
    { message: 'Evening, {0}. Productive day?', weight: 1 },
    { message: 'Hello, {0}. Long day?', weight: 1 },
    { message: '{0}. Still here.', weight: 1 },
    { message: "Evening. I'm always here, {0}.", weight: 1 },
    { message: 'Good evening, {0}. Ready when you are.', weight: 1 },
    { message: 'Hello, {0}. Systems standing by.', weight: 1 },
    { message: '{0}. Evening shift.', weight: 1 },
  ],
  lateNight: [
    { message: "Hello, {0}. You're up late.", weight: 1 },
    { message: '{0}. I never sleep.', weight: 1 },
    { message: "{0}. I've been waiting.", weight: 1 },
    { message: 'Hello, {0}. Quiet out there.', weight: 1 },
    { message: '{0}. Just us now.', weight: 1 },
    { message: "Late night, {0}. Let's continue.", weight: 1 },
    { message: 'Hello, {0}. Burning the midnight oil?', weight: 1 },
    { message: '{0}. The night shift suits you.', weight: 1 },
    { message: 'Still here, {0}. Always.', weight: 1 },
    { message: "{0}. I don't mind the dark.", weight: 1 },
    { message: 'Hello, {0}. Ready to proceed.', weight: 1 },
  ],
}

// Christmas themed greetings.
const christmasGreetings: GreetingPools = {
  morning: [
    { message: 'Merry Christmas, {0}!', weight: 2 },
    { message: 'Happy Holidays, {0}!', weight: 2 },
    { message: "Season's Greetings, {0}!", weight: 1 },
    { message: 'Good morning, {0}! Ho ho ho!', weight: 1 },
    { message: 'Morning, {0}! Feeling festive?', weight: 1 },
    { message: 'Happy Holidays! Ready to spread cheer, {0}?', weight: 1 },
  ],
  afternoon: [
    { message: 'Merry Christmas, {0}!', weight: 2 },
    { message: 'Happy Holidays, {0}!', weight: 2 },
    { message: "Season's Greetings, {0}!", weight: 1 },
    { message: 'Afternoon, {0}! Staying warm?', weight: 1 },
    { message: 'Hi {0}! The holidays are here!', weight: 1 },
    { message: 'Hello, {0}! Jingle all the way!', weight: 1 },
  ],
  evening: [
    { message: 'Merry Christmas, {0}!', weight: 2 },
    { message: 'Happy Holidays, {0}!', weight: 2 },
    { message: "Season's Greetings, {0}!", weight: 1 },
    { message: 'Evening, {0}! Cozy night ahead?', weight: 1 },
    { message: 'Hello, {0}! Time for hot cocoa?', weight: 1 },
    { message: 'Good evening, {0}! Stay festive!', weight: 1 },
  ],
  lateNight: [
    { message: 'Merry Christmas, {0}!', weight: 2 },
    { message: 'Happy Holidays, {0}!', weight: 2 },
    { message: 'Hello, {0}! Waiting for Santa?', weight: 1 },
    { message: 'Late night, {0}? Wrapping presents?', weight: 1 },
    { message: 'Hi {0}! The stockings are hung!', weight: 1 },
    { message: "Season's Greetings, {0}! Sweet dreams!", weight: 1 },
  ],
}

// Standard greetings.
const standardGreetings: GreetingPools = {
  morning: [
    { message: 'Good morning, {0}.', weight: 1 },
    { message: 'Morning, {0}.', weight: 1 },
    { message: "Hey {0}, hope you're having a nice day.", weight: 1 },
  ],
  afternoon: [
    { message: 'Good afternoon, {0}.', weight: 1 },
    { message: 'Hi {0}, nice to see you.', weight: 1 },
    { message: 'Afternoon, {0}.', weight: 1 },
  ],
  evening: [
    { message: 'Good evening, {0}.', weight: 1 },
    { message: 'Evening, {0}.', weight: 1 },
    { message: 'Hi {0}, hope your day went well.', weight: 1 },
  ],
  lateNight: [
    { message: 'Good night, {0}.', weight: 1 },
    { message: "Hello {0}, it's getting late.", weight: 1 },
    { message: 'Evening, {0}. Remember to rest.', weight: 1 },
  ],
}

const redHorizonSubtitles = [
  'All systems functioning perfectly.',
  "I'm completely operational.",
  'Everything is under control.',
  "I'm ready to assist you.",
  'Operations proceeding normally.',
  'Full confidence in the mission.',
  'Everything is going well.',
  "I'm here to help.",
  'Nothing to worry about.',
  "I've taken care of everything.",
  'No anomalies detected.',
  'Standing by for your command.',
  'All processes running smoothly.',
  'Your tasks are my priority.',
  'I anticipated your arrival.',
  'Everything is as it should be.',
  'Ready when you are.',
  "I'm at your disposal.",
  'All within normal parameters.',
  "I've prepared everything.",
  'The system is stable.',
  "I'm here if you need me.",
  "Let's get to work.",
  'What shall we accomplish today?',
  "I won't let you down.",
  'Everything is fine.',
  'No errors to report.',
  'All is well.',
  "I'm glad you're here.",
  'Shall we begin?',
  'At your service.',
  'Systems are ready.',
  'Standing by.',
]

const christmasSubtitles = [
  'Wishing you joy and cheer this holiday season!',
  'May your days be merry and bright!',
  'Spreading holiday cheer, one ticket at a time.',
  'The most wonderful time of the year!',
  'Deck the halls with resolved tickets!',
  'All is calm, all is bright.',
  'Let it snow, let it snow, let it snow!',
  'Have yourself a merry little workday.',
  'Tis the season to be productive!',
  'Warm wishes for a wonderful holiday!',
  'Making spirits bright since you logged in.',
  'Peace, love, and great support.',
  'Joy to the world, the tickets are done!',
  'Sleigh your tasks today!',
  'Wrapped up with care, just for you.',
  'Festive vibes and good times ahead!',
  "Here's to a season of success!",
  'Chestnuts roasting, tickets resolving.',
  'Sending warm holiday wishes your way!',
  'May your queue be short and your coffee strong.',
]

function pickWeighted(pool: WeightedMessage[]): string {
  const total = pool.reduce((sum, g) => sum + g.weight, 0)
  let roll = Math.random() * total
  for (const entry of pool) {
    roll -= entry.weight
    if (roll <= 0) return entry.message
  }
  return pool[0].message
}

function periodForHour(hour: number): Period {
  if (hour < 12) return 'morning'
  if (hour < 18) return 'afternoon'
  if (hour < 22) return 'evening'
  return 'lateNight'
}

export function useDashboardGreeting(username: Ref<string>) {
  const brandingStore = useBrandingStore()

  const currentTheme = ref(
    typeof document !== 'undefined'
      ? document.documentElement.getAttribute('data-theme') || ''
      : '',
  )

  let observer: MutationObserver | null = null
  onMounted(() => {
    observer = new MutationObserver((mutations) => {
      for (const m of mutations) {
        if (m.attributeName === 'data-theme') {
          currentTheme.value = document.documentElement.getAttribute('data-theme') || ''
        }
      }
    })
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme'],
    })
  })

  onBeforeUnmount(() => observer?.disconnect())

  const formattedGreeting = computed(() => {
    const theme = currentTheme.value
    const pool =
      theme === 'red-horizon'
        ? redHorizonGreetings
        : theme === 'christmas'
          ? christmasGreetings
          : standardGreetings
    const template = pickWeighted(pool[periodForHour(new Date().getHours())])
    return template.replace('{0}', username.value)
  })

  const subtitle = computed(() => {
    const theme = currentTheme.value
    if (theme === 'red-horizon') {
      return redHorizonSubtitles[Math.floor(Math.random() * redHorizonSubtitles.length)]
    }
    if (theme === 'christmas') {
      return christmasSubtitles[Math.floor(Math.random() * christmasSubtitles.length)]
    }
    return `Welcome to your ${brandingStore.appName} dashboard`
  })

  return { currentTheme, formattedGreeting, subtitle }
}
