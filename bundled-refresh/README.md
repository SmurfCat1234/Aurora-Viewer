# Refreshes for five bundled languages

These optional import files target Aurora Viewer at [`231c88a3`](https://github.com/Aurora-Viewer/Aurora-Viewer/commit/231c88a3). They cover Simplified Chinese (`zh-Hans`), Traditional Chinese (`zh-Hant`), Russian (`ru`), German (`de`) and Brazilian Portuguese (`pt-BR`).

Each file supplies the 22 entries missing from the corresponding bundled catalog at that revision, including Weather, avatar-cache, audio, local texture sorting and World filters. Each uses upstream's current labels for avatar information and inspecting all attachments. The obsolete chat and script-info IDs have been removed. Each file supplies 14,998 active message IDs and passed the Fluent render check. Aurora uses English for the 148 messages awaiting translation on this baseline.

To try a refresh, select **Use a language file (.ftl)** in **Preferences → Language and updates**, choose the file and restart. Keep its original filename. See [PR #4335](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/4335) for the Windows offline captures and review status.
