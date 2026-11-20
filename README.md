# Aurora Viewer language review files

These optional Fluent catalogs support the [community localization draft, #4335](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/4335), continuing the work recorded in [#4220](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/4220). German, Brazilian Portuguese, Russian and both Chinese catalogs are now built into upstream Aurora through [#4315](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/4315).

The Spanish file covers all 15,032 active English messages at [upstream commit `afc7d8c4`](https://github.com/Aurora-Viewer/Aurora-Viewer/commit/afc7d8c4d189fe3ac57d39844cd1e9296b44ee11), including 5,310 individually reviewed messages. This pass adds Appearance, the World Map, the toolbar editor and its command list, and the audio listening position. The other new-language drafts reflect an earlier 0.9.12 beta baseline. Missing strings in those drafts use English.

The [bundled refresh files](bundled-refresh) are importable copies of the five upstream community catalogs. Each adds nine current weather, avatar-cache and audio messages to the corresponding upstream catalog. These files are for release review.

To try one, download the raw `.ftl` file, keep its filename, and choose it under **Preferences → Language and updates → Use a language file (.ftl)**. Restart Aurora. Clear the checkbox and restart to return to the built-in language selection. The existing importer and English fallback are described in [#4187](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/4187) and [#4196](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/4196).

These machine-assisted drafts are at different stages of proofreading. The PR table records current coverage and review status. If you find a problem, please include the filename, Viewer version, screen or menu path, and suggested wording. An offline demo screenshot can help demonstrate a layout issue.

The Arabic and Hebrew files are available for testing. In Windows offline Preferences, some text still appears in the wrong reading order and the window retains its left-to-right layout. The existing [bidirectional text work in #1191](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/1191) covers chat and some other text surfaces. The interface still needs a separate RTL UI pass.

These translations are distributed under the project's [GNU GPL version 3](LICENSE).
