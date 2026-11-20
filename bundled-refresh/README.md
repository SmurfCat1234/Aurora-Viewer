# Refreshes for five bundled languages

These optional import files target Aurora Viewer at [`2ca847e5`](https://github.com/Aurora-Viewer/Aurora-Viewer/commit/2ca847e5). They cover Simplified Chinese (`zh-Hans`), Traditional Chinese (`zh-Hant`), Russian (`ru`), German (`de`) and Brazilian Portuguese (`pt-BR`).

Each file supplies the 22 entries missing from the corresponding bundled catalog at that revision, including Weather, avatar-cache, audio, local texture sorting and World filters. Each has all 15,045 active English message IDs and passed the Fluent render check. The batch worklist reports zero missing or retired IDs against that English revision.

To try a refresh, select **Use a language file (.ftl)** in **Preferences → Language and updates**, choose the file and restart. Keep its original filename. See [draft PR #4335](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/4335) for the Windows offline captures and review status.
