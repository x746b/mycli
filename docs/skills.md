# Skills in MyCLI 1.9.7

## Using skills

- `/skill`: pick a skill; Enter invokes it, Esc cancels. An argument hint prompts for arguments.
- `/skill <name> [arguments]`: invoke directly, including aliases and case-insensitive names.
- `/skill list`: list user-invocable skills with their source.
- `/skill paths`: display the internal catalog and discovery roots.
- `/skill reload`: reload internal definitions, external metadata, and configured paths.
- `mycli '/skill debug failing tests'`: invoke a named skill in a single-shot session.

Slash invocation sends the expanded instructions as the next user turn, preserving
conversation and normal cancellation handling. It works on every tier, using only
that tier's tools. On `full`, the model can also call `Skill` with `skill="list"` or
a name and `args`. That tool shares the active catalog, including successful reloads.

## Internal catalog

Lookup: `MYCLI_SKILLS` → `$XDG_CONFIG_HOME/mycli/skills-internal.toml` (default
`~/.config/mycli/skills-internal.toml`) → legacy `~/.mycli/skills-internal.toml` →
embedded defaults. An explicit override is authoritative. Invalid startup content
warns and uses embedded defaults; invalid reloads preserve the previous catalog.

The file replaces the internal catalog, so deleting a table removes that skill.
An empty `[skills]` table disables all internal definitions. Example:

```toml
version = 1

[skills.review]
description = "Review the requested code for correctness"
aliases = ["inspect"]
argument_hint = "<files or topic>"
user_invocable = true
model_invocable = true
enabled = true
prompt = '''
Review $ARGUMENTS. Identify concrete defects and cite the relevant code.
Explain missing evidence and propose focused verification.
'''
```

Names/aliases are unique, case-insensitive identifiers containing letters, digits,
`-`, or `_`. `list`, `reload`, `path`, and `paths` are reserved. A description and
nonempty prompt are required. `enabled`, `user_invocable`, and `model_invocable`
default to true. Unknown fields, duplicate aliases, and unsupported versions fail
validation. `allowed_tools` is optional descriptive metadata, not permission enforcement.

## External discovery

Precedence: internal catalog, project roots, user roots, then configured roots in
file order. Within each root paths are sorted. The first matching name/alias wins.
Searches include nested directories up to eight levels and follow symlinks with loop
protection. Supporting Markdown beneath a `SKILL.md` folder is not listed separately.

Standard roots are `.config/mycli/skills`, `.claude/commands`, `.claude/skills`, and
`.agents/skills`. The user `.config` root honors `XDG_CONFIG_HOME`.

Add collections, individual skill folders, or a `SKILL.md` path in `config.toml`:

```toml
skill_paths = ["~/my-skills", "./team/skills"]
```

Relative paths use the session working directory; `~/` expands to the home directory.
A project `skill_paths` list replaces the global list, including an explicit empty list.
Run `/skill reload` after editing paths or skill metadata. External content is read
from its discovered path when invoked; frontmatter names need not match folders.
Malformed YAML is reported and skipped during discovery.

Example external file, `~/.config/mycli/skills/review/SKILL.md`:

```markdown
---
name: review-code
description: Review requested code for concrete defects
argument-hint: "<files or topic>"
---
Review $ARGUMENTS. Cite evidence and suggest focused checks.
```

Then run `/skill reload` followed by `/skill review-code src/main.rs`.

Supported Claude-style metadata: `name`, quoted/multiline `description`,
`argument-hint`, `allowed-tools`, `user-invocable`, and `disable-model-invocation`.
`aliases` is additionally supported as a comma-separated string or YAML list.
Nested command files without a declared name use names such as `team:review`.

Templates expand `$ARGUMENTS`, `$ARGUMENTS_SUFFIX`, `$0`, `$1`, `$ARGUMENTS[0]`, and
`${CLAUDE_SKILL_DIR}`. Positional arguments split on whitespace; full arguments retain
their original text. The model receives the external skill directory to resolve
references. Scripts/references are loaded or executed only through ordinary model
tool calls. Shell substitutions, Claude hooks, forked contexts, model routing, and
automatic permission grants are not implemented. `allowed-tools` is metadata only.

## Content review and upstream

The internal commit workflow is adapted from the principles in the
[official OpenAI customization example](https://learn.chatgpt.com/docs/customization/overview):
explicit staging, focused commits, and messages matching the change. MyCLI adds
review of both diffs, preservation of unrelated staging, appropriate checks, and
honest reporting. It does not assume an OpenAI identity or push automatically.
The other templates clarify verification, persistence, or available capabilities;
`loop` no longer assumes an unavailable scheduler exists.

Upstream Cersei revision `708c5055845ba6c682d92960cec99e3adbcca3e1` already supplies
[skill discovery](https://github.com/pacifio/cersei/blob/708c5055845ba6c682d92960cec99e3adbcca3e1/crates/cersei-tools/src/skills/discovery.rs)
and the Skill tool, but not this editable TOML catalog or MyCLI slash-command wiring.
These changes extend the vendored modules without upgrading the whole SDK.
