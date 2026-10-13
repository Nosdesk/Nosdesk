-- Plugin storage compare-and-set: each write to a plugin_data row bumps its
-- version, and a plugin may make a write conditional on the version it read.
-- A constant default adds the column without rewriting rows, so the audit
-- trigger on plugin_data does not fire.
ALTER TABLE plugin_data ADD COLUMN version bigint NOT NULL DEFAULT 1;
