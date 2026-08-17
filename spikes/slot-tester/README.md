# slot-tester

A dev plugin that renders a distinct panel on every UI extension point, so slot
regressions show up without a real integration. One `mount` switches on
`context.component.slot`.

Covers the ordinary slots (ticket sidebar, asset info, settings page, dashboard
widget, header action, bulk action, nav item) and the awkward cases: a panel
that renders nothing, one whose mount never settles, one that fills in late, and
one that declares `chrome: "none"` and draws its own frame.

```bash
# debug builds only, needs NOSDESK_DEV_MODE=1 in .env
nosdesk-plugin sign --dev --in . --out ../../plugins/slot-tester-1.0.0.zip
```

The backend provisions `plugins/*.zip` on startup. Zips there are gitignored;
rebuild from this directory rather than committing one.
