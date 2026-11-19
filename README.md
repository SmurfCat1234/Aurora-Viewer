# Aurora Viewer language review files

These optional Fluent catalogs are for reviewers of [Aurora Viewer PR #4220](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/4220). Spanish contains all 15,240 English message keys from [upstream commit `a3ca02f1`](https://github.com/Aurora-Viewer/Aurora-Viewer/commit/a3ca02f182efb54986d6a008cda05d592376c29f) during the 0.9.12 beta cycle. All 2,354 keys directly referenced by Preferences and all 101 directly referenced by the new Aurora Weather window have been reviewed in Spanish. The other import drafts come from [`cd6a0aae`](https://github.com/Aurora-Viewer/Aurora-Viewer/commit/cd6a0aaee2d140de34ec816f6a32086e86bb4aba) and use English fallback for 47 new camera and weather messages. The importer ignores their obsolete Preferences weather heading.

To try one, download the raw `.ftl` file, keep its filename, and choose it under **Preferences → Language and updates → Use a language file (.ftl)**. Restart Aurora. Clear the checkbox and restart to return to the built-in language selection. The existing importer and English fallback are described in [#4187](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/4187) and [#4196](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/4196).

These machine-assisted drafts are at different stages of proofreading. The PR table records which languages are bundled and which screens have been checked. If you find a problem, please include the filename, Viewer version, screen or menu path, and suggested wording. An offline demo screenshot can help demonstrate a layout issue.

The Arabic and Hebrew files are available for testing. In Windows offline Preferences, some text still appears in the wrong reading order and the window retains its left-to-right layout. The existing [bidirectional text work in #1191](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/1191) covers chat and some other text surfaces. The interface still needs a separate RTL UI pass.

These translations are distributed under the project's [GNU GPL version 3](LICENSE).
