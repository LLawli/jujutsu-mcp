# Tools

Every tool takes `repo`: the absolute path of the repository or of any
directory inside it. Revisions use jj's revset language (`@`, `@-`, `main`,
`feat@origin`, `root()..@`). Paths in `paths` are relative to `repo` and taken
literally: no globs or fileset syntax, so spaces, parentheses and `|` in file
names are safe.

Results:

- Text tools return jj's output, then a second block with what jj reported on
  stderr ("Working copy now at", "Rebased 3 commits", warnings).
- JSON tools (`log`, `bookmark_list`, `op_log`) return `structuredContent`
  matching their `outputSchema`, plus the same JSON as text.
- A failure is a result with `isError: true`. A jj failure carries the exact
  command, the exit code and jj's stderr; an invalid parameter says which one.

The hints column is what MCP clients use for permissions: **R** read-only,
**D** destructive, **O** open world (talks to the network or runs anything).

## Read

| Tool | Hints | Parameters |
|---|---|---|
| `status` | R | none besides `repo` |
| `log` | R | `revisions` (default: jj's log revset), `limit` (default 50) |
| `show` | R | `revision` (default `@`), `format` |
| `diff` | R | `revisions` (default `@`) or `from`/`to`; `paths`; `format` |
| `bookmark_list` | R | `all_remotes`, `names` (jj string patterns) |
| `op_log` | R | `limit` (default 20) |

`format` is one of `git`, `stat`, `summary`, `name_only`; without it jj uses
its configured format.

`log` returns `{commits, truncated}`. Each commit has `change_id`,
`commit_id`, `parents` (commit ids), `description`, `author` and `committer`
(`name`, `email`, `timestamp`), `bookmarks`, `remote_bookmarks`
(`name@remote`, without the colocated `git` remote), and the flags
`working_copy`, `empty`, `conflict`, `immutable`, `divergent`. `truncated` is
true when more commits matched than `limit`.

`bookmark_list` returns `{bookmarks}` with `name`, `remote` (null for local),
`target` (commit ids; several when conflicted), `conflict` and `tracked`.

`op_log` returns `{operations, truncated}` with `id` (12 hex digits),
`description`, `time`, `args` (the command line, when jj recorded it) and
`snapshot` (an automatic working-copy snapshot).

## Local write

All of them are undone with `undo`.

| Tool | Hints | Parameters |
|---|---|---|
| `describe` | | `message` (empty clears it), `revision` (default `@`) |
| `new` | | `parents` (default `@`; several make a merge), `message` |
| `commit` | | `message`, `paths` (default: everything) |
| `squash` | | `revision`, or `from`/`into` (default: `@` into `@-`); `paths`; `message` or `use_destination_message`; `keep_emptied` |
| `split` | | `paths` (go into the first commit), `message` (of the first commit), `revision` (default `@`), `parallel` |
| `edit` | | `revision` |
| `rebase` | | at most one of `revisions`, `source`, `branch` (default `branch: "@"`); exactly one of `onto`, `insert_after`, `insert_before`; `skip_emptied` |
| `restore` | D | `from`/`into`, or `changes_in`; `paths`; `restore_descendants` |
| `abandon` | D | `revisions`, `retain_bookmarks`, `restore_descendants` |
| `undo` | | none besides `repo` |
| `file_untrack` | | `paths` (must already be ignored) |
| `bookmark_set` | | `name`, `revision` (default `@`), `allow_backwards`; creates the bookmark if needed |

`squash` with descriptions on both sides needs `message` or
`use_destination_message`: jj would otherwise open an editor, which the server
never allows.

## Network

| Tool | Hints | Parameters |
|---|---|---|
| `git_fetch` | O | `remote` or `all_remotes`; `branches` |
| `git_push` | D, O | `bookmarks`, `changes`, `remote`, `dry_run` |

`git_push` first runs a dry run and counts the commits that will be signed;
its result opens with `commits to sign: N`. With `dry_run: true` it stops
there, so an agent can warn the user before a push that asks for one
security-key touch per commit. Pushing a bookmark the remote lacks creates it
there; naming a bookmark that does not exist locally is an error before
anything is pushed.

Both send MCP progress notifications while jj runs, when the client asks for
them, and a client cancellation kills the jj process.

## Anything else

| Tool | Hints | Parameters |
|---|---|---|
| `run` | D, O | `args`: jj arguments after `jj`, as a list |

`run` covers what has no dedicated tool (`bookmark delete`, `bookmark track`,
`duplicate`, `file track`, `git remote`, `workspace add`). It runs without a
shell, under the same rules as every other tool, and always takes the write
queue because it cannot tell a read from a write. Prefer the dedicated tools:
they validate parameters and return structured results.
