# Session history and global memory — MyCLI 2.0.0

## Storage

All paths use `$XDG_CONFIG_HOME/mycli`, defaulting to `~/.config/mycli` on Linux and macOS:

```text
mycli/
├── MEMORY.md
├── memory/                    # Optional Markdown topics
└── sessions/
    └── <UUID>/
        ├── transcript.jsonl   # Append-only event journal
        └── context.json       # Atomic active-context checkpoint
```

Every new conversational invocation creates a UUID session. Read-only command-line
catalog listings do not create sessions. Journal entries contain message additions,
context replacements, settings metadata, and cumulative reported usage. Checkpoints
contain the current message list, session name/ID, project directory, model/provider,
persona, tool tier, reasoning/thinking settings, and usage.

Configuration credentials are not copied into metadata. The journal contains the
conversation itself, including user input and model-visible tool results, so its
contents can include anything discussed in that conversation. Session directories
are mode 0700 and files mode 0600 on Unix. The journal schema is MyCLI version 1;
it is not an importer for existing Claude session files.

## Saving and compaction

The agent checkpoints user input before inference, complete model responses, and each
completed tool result. Errors and ordinary cancellation checkpoint remaining context.
Available interrupted text is saved with an interruption marker; partial reasoning
is archived as metadata rather than replayed as an incomplete signed thinking block.
An abrupt kill retains the last committed checkpoint, not necessarily the last token
that appeared on screen.

Compaction appends a context-replacement event. It does not rewrite or erase previous
transcript entries. `context.json` is written through a same-directory temporary file
and atomic rename; the journal is synced first and is authoritative for recovery.
On resume, replay recovers the latest context and regenerates the checkpoint. A torn
final journal line is discarded; a corrupt complete record fails visibly. An exclusive
advisory lock prevents concurrent writers or resuming a session already open elsewhere.

Only model-visible tool output is part of the conversation journal. Large raw output
artifacts remain subject to the existing temporary-output retention policy. Tool
execution is never automatically replayed during resume. Unanswered calls receive
an interruption result stating that their outcome is unknown and should be checked.

## Commands

| Command | Behavior |
| --- | --- |
| `/sessions` | Pick another saved session to resume |
| `/sessions list` | List IDs, names, models, and active-context message counts |
| `/sessions path` | Show the current session directory |
| `/sessions rename <name>` | Rename the current session; spaces are allowed |
| `/sessions rename` | Prompt for a name |
| `/sessions rename <full-id> <name>` | Rename another saved, unlocked session |
| `/resume <id/name/prefix>` | Resume an unambiguous saved session |
| `/resume latest` | Resume the most recently updated other session |
| `/resume` | Open the session picker |
| `mycli --resume <id/name>` | Resume directly at startup |
| `mycli --resume` | Resume the latest saved session |
| `mycli '/sessions list'` | List sessions without connecting to a provider |

Names are display labels, not directory names. Duplicate names are allowed; ambiguous
selection requires the full ID. Renaming never changes a UUID or moves files.
Switching to a saved session keeps the session being left. Existing model/provider,
persona, tier, or prompt-reload operations still reset active context as before, but
their earlier messages remain archived in the same session journal.

Resume restores the saved project, model and settings; credentials and endpoints are
resolved from current global/project profiles and environment variables. Removed
profiles or project directories produce an error instead of silently changing them.
Direct one-off endpoint overrides should be made into named profiles for reliable
resumption. Current serving-window metadata is resolved anew; a saved large window
cannot override a smaller deployment. Current lower output limits also take precedence.
Resume restores model context, not the old terminal screen; `/sessions path` locates
the transcript for inspection. Global memory and project instructions are read afresh
rather than restored from an old system prompt. Saved history is compacted as needed before the next request.

## Exit behavior

- `/quit`, `/exit`, and `/q` ask `Keep this session archive and resumable context? [Y/n]`.
- `Y`, `yes`, or Enter keeps it. `N` or `no` deletes only the current UUID directory.
- Ctrl+C, Ctrl+D/EOF, or an interrupted keep/delete prompt preserves it.
- During generation, the first Ctrl+C cancels work and returns to the prompt; the
  usual second/idle Ctrl+C exits. Cancellation never opts into deletion.
- Single-shot invocations keep their sessions automatically.

Deletion does not undo code changes, remove other sessions, delete global memory,
or clear the separate input-history file. To discard an older session, resume it,
then `/quit` and answer N. Journal files from sessions started before this feature
cannot be reconstructed from the old input-only history.

## Durable Markdown memory

`MEMORY.md` holds concise user-wide preferences and durable facts. Project-specific
facts should name their project path. Detailed topics can live in `memory/*.md`, with
links in the index. Only the bounded index is added automatically; topic files are
read through normal tools when relevant. This does not scan skills or session archives
and does not enable the Grafeo database or automatic fact extraction.

- `/memory` or `/memory show`: display the memory index.
- `/memory path`: show index and topic-directory paths.
- `/memory topics`: list Markdown topic files.
- `/memory reload`: apply file edits to the current system prompt without discarding chat.
- `/remember <fact>`: explicitly append a fact, avoiding exact duplicate entries, and reload it.
- `/skill remember <fact>`: use the model-assisted memory workflow; it knows the global path.

New sessions load current memory. External/model-assisted edits can be applied within
the current conversation using `/memory reload`. The injected index is capped at 200
lines and 8 KiB, further reduced for small context budgets; truncation is UTF-8 safe
and explicitly marked. `/remember` preserves the complete file rather than writing
back the excerpt. A hidden `.memory.lock` coordinates direct CLI append operations.

This uses the existing Cersei Memory interface with a new journal backend and targeted
checkpoint hooks. It avoids the simple JSONL backend's whole-file overwrite behavior
and implements Markdown writes directly rather than relying on graph-only `store_memory`.
