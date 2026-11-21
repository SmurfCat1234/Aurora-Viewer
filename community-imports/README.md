# Aurora Viewer language review packs

These 26 opt-in Fluent catalogs target Aurora Viewer at `e0e1c6c7`. Each file passed parsing, key and variable checks, and formatting with representative type and selector values. Wording counts and pending text findings are recorded separately in `manifest.json` and the reports in the ZIP.

1. Choose a file named after your language, such as `es.ftl` or `ko.ftl`.
2. In Aurora Viewer, open **Preferences → Language and updates** and turn on **Use a language file (.ftl)**.
3. Select the file and restart the Viewer. Leave its filename unchanged so Fluent can select the right locale.

Across these files, 1836 missing messages use the Viewer’s current English text. The copyright-report notice has draft translations that preserve the original links and reporting steps. Wording and right-to-left layout still need acceptance.

For the next release, run Aurora’s `catalog_delta` example against the old and new English files. That produces added, changed, and retired message IDs. Run the Fluent checks again after applying the delta. Use the audit reports and representative Windows offline screens to check the wording changes. [Aurora translator guide](https://github.com/Aurora-Viewer/Aurora-Viewer/blob/e0e1c6c7/docs/TRANSLATING.md)

The validation manifest has the English baseline, coverage and counts by language, plus SHA-256 values with LF line endings. 19 catalogs proposed for the next upstream build plus 7 optional copies from built-in catalogs and earlier reviews: `es`, `ko`, `nb`, `pl`, `pt-PT`, `tr`, `uk`. The copies correspond to languages built into the review build or upstream. Korean is built into the #4335 review build and is pending upstream approval. Italian’s original contribution is credited in [#4259](https://github.com/Aurora-Viewer/Aurora-Viewer/issues/4259).

The [wording worklist](quality/wording-priority.tsv) groups exact English matches by message across the previews. Use it with the source screen to decide which phrases need translation.

See [the screenshots](screenshots/README.md) for representative Windows offline checks.
