# Email quote-split fixture corpus

Sample inbound email bodies covering the canonical client
variations. Each directory contains three files:

  - `input.{txt,html}`        — the body as it would arrive (plain
                                text or HTML; the harness picks the
                                splitter by file extension).
  - `expected_new.{txt,html}` — what the splitter should return as
                                `new_content`.
  - `expected_quoted.{txt,html}` — what the splitter should return
                                as `quoted_content`. Omit the file
                                entirely when the expected value is
                                `None`.

The harness lives at `backend/tests/email_quote_corpus.rs` and
iterates this directory, asserting one split per fixture. When a
real-world email arrives that misfires, drop it in as a new
fixture rather than reproducing the issue inline in unit tests:
the directory is the de-facto regression-test net for this module.

## Adding a fixture

1. `mkdir backend/tests/fixtures/email_quote/<descriptive_name>`
2. Save the raw body as `input.txt` (or `input.html`).
3. Save the expected split as `expected_new.{ext}` and, if any,
   `expected_quoted.{ext}` — same extension as the input.
4. `cargo test --test email_quote_corpus` to verify.

## Why a corpus and not just unit tests

In-source unit tests cover the canonical client variations once
each. The corpus exists for the long tail: every customer email
that misfires gets captured here so the same pattern can never
regress without a failing test. Mature reply-parsing projects
(Discourse, Mailgun's `talon`, GitHub's `email_reply_parser`) all
maintain corpora because reply-quoting is heuristic, and the only
way to keep heuristics honest is to grow the test set every time
they're wrong in practice.
