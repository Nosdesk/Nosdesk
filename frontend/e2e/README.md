# End-to-end tests

Cover what jsdom cannot: touch gesture arbitration, scroll position, and real
layout measurement. Several of these behaviours failed silently in production
code before being caught by hand — a board whose drag is stolen by the scroll
container still renders perfectly.

## Running

The suite drives the dev stack rather than starting its own. Spinning up a
multi-container app with a database per run would be slower and flakier than
pointing at the one already running.

```bash
make dev          # backend + postgres + frontend watch
make seed-demo    # demo users, projects and tickets the specs navigate to
pnpm --filter nosdesk-frontend run test:e2e
```

Point elsewhere with `E2E_BASE_URL=https://host pnpm ... run test:e2e`.

Two projects run: `phone` (390x844, touch) and `desktop` (1680x1000). Specs opt
in with `test.skip(({ hasTouch }) => ...)`, so touch and pointer behaviour are
asserted on the surface each belongs to.

## Why this is not in CI

It needs the full stack and seeded data. Wiring that into CI is worthwhile but
is its own piece of work; until then this runs on demand, and the unit tests
(`pnpm --filter nosdesk-frontend run test`) cover the pure logic in CI.

## Notes for editing these specs

- **They mutate demo data.** The drag specs move tickets between columns and do
  not restore them, so repeated runs drift the board. Re-run `make seed-demo` to
  reset. Assertions therefore check that a card *changed* column, never that it
  landed in a named one.
- **Select on `data-card-id`, not geometry.** An earlier version matched cards by
  size and silently found nothing once the cards moved and the incidental
  wrapper sizes changed.
- **Never wait for `networkidle`.** The app holds an SSE connection open, so the
  network never goes idle and the wait always times out.
- **The sync pool fills in after mount.** `gotoAndSettle` waits for it; asserting
  immediately reads an empty board.
- **Touch goes through CDP** (`Input.dispatchTouchEvent`), which is Chromium
  only. The `phone` project is spelled out rather than using an `iPhone` preset,
  because those imply WebKit and would fail to launch.
