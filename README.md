# Aurora Viewer language review files

These optional Fluent catalogs continue the localization work recorded in [Aurora Viewer PR #4220](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/4220). German, Brazilian Portuguese, Russian and both Chinese catalogs are now built into upstream Aurora through [#4315](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/4315).

The Spanish file covers all 15,030 active English messages at [upstream commit `64246c75`](https://github.com/Aurora-Viewer/Aurora-Viewer/commit/64246c7501b1c8b071d7dbe478530d74431ea232), including 5,003 individually reviewed messages. This pass covers People, resident profiles, Places, Search and Conversations. The remaining drafts reflect an earlier 0.9.12 beta baseline. Missing strings in those drafts use English, and their coverage of current features will be checked during each language's own review.

To try one, download the raw `.ftl` file, keep its filename, and choose it under **Preferences → Language and updates → Use a language file (.ftl)**. Restart Aurora. Clear the checkbox and restart to return to the built-in language selection. The existing importer and English fallback are described in [#4187](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/4187) and [#4196](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/4196).

These machine-assisted drafts are at different stages of proofreading. The PR table records current coverage and review status. If you find a problem, please include the filename, Viewer version, screen or menu path, and suggested wording. An offline demo screenshot can help demonstrate a layout issue.

The Arabic and Hebrew files are available for testing. In Windows offline Preferences, some text still appears in the wrong reading order and the window retains its left-to-right layout. The existing [bidirectional text work in #1191](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/1191) covers chat and some other text surfaces. The interface still needs a separate RTL UI pass.

These translations are distributed under the project's [GNU GPL version 3](LICENSE).
