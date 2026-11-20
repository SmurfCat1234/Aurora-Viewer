# Aurora Viewer language review packs

These 26 opt-in Fluent catalogs target Aurora Viewer at `3fe6a320`. Each file passed parsing, key and variable checks, and formatting with representative type and selector values. Wording counts and pending text findings are recorded separately in `manifest.json` and the reports in the ZIP.

1. Choose a file named after your language, such as `es.ftl` or `ko.ftl`.
2. In Aurora Viewer, open **Preferences → Language and updates** and turn on **Use a language file (.ftl)**.
3. Select the file and restart the Viewer. Leave its filename unchanged so Fluent can select the right locale.

All of the current English message IDs are present. The Viewer uses the English message when a future key is missing or a translation cannot render. The copyright-report notice has draft translations that preserve the original links and reporting steps. Wording and right-to-left layout still need acceptance.

For the next release, run Aurora’s `catalog_delta` example against the old and new English files. That produces added, changed, and retired message IDs. Run the Fluent checks again after applying the delta. Use the audit reports and representative Windows offline screens to check the wording changes. [Aurora translator guide](https://github.com/Aurora-Viewer/Aurora-Viewer/blob/3fe6a320/docs/TRANSLATING.md)

The validation manifest has the English baseline, coverage and counts by language, plus SHA-256 values with LF line endings. The review packs and discussion are tracked in [draft #4335](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/4335). Italian is tracked separately in [#4259](https://github.com/Aurora-Viewer/Aurora-Viewer/issues/4259).

See [the draft PR screenshots](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/4335#screenshots-and-checks) for representative Windows offline checks.
