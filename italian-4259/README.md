# Italian catalog update for Aurora Viewer

[Issue #4259](https://github.com/Aurora-Viewer/Aurora-Viewer/issues/4259) contains the full Italian catalog contributed by [00JackWild00](https://github.com/00JackWild00). The source message IDs in the attachment match the English catalog at [`c382aea0`](https://github.com/Aurora-Viewer/Aurora-Viewer/commit/c382aea0). The update targets [`3fe6a320`](https://github.com/Aurora-Viewer/Aurora-Viewer/commit/3fe6a320).

- `it-delta-3fe6a320.ftl` adds 282 new messages and updates 11 existing messages. The additions cover language import, game controls, Weather, World filters, audio, the second camera and the avatar attachment inspector.
- `removed-keys.txt` lists the 244 IDs retired upstream. This includes the previous chat and script-info IDs and retired region weather and environment controls.
- Apply the delta by message ID to the issue attachment's `it.ftl` and delete the retired IDs. Keep the resulting filename as `it.ftl` for Viewer import.
- The resulting 15,044 active messages passed Fluent rendering checks. The delta follows the attachment's terminology and address form. The project's formal-address guidance still needs to be reconciled with the original catalog during Italian review.

The delta is available to support the Italian contribution described in #4259. The Spanish review continues in [draft #4335](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/4335).
