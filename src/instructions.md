Tools for jj (Jujutsu), a version control system layered on git. Repositories
are colocated: `.jj` and `.git` side by side, git as storage and transport.

Every tool takes `repo`: the absolute path of the repository or any
directory inside it. Pass it on every call.

How jj differs from git:

- There is no staging area. The working copy is a commit, `@`, and every
  file that is not ignored is part of it as soon as it is written. Check
  `status` before describing or closing a commit; a stray file leaves with
  `file_untrack` after adding it to `.gitignore`.
- A commit has a change id (stable across rewrites) and a commit id (changes
  on every rewrite). Refer to commits by change id.
- Making a commit: edit files, `describe` with the message, then `new` to
  start the next one. `commit` does both, optionally for some paths only.
- Rewriting is normal and cheap: `squash`, `split` (by paths), `rebase`,
  `describe` on any mutable revision. Descendants are rebased automatically.
  Commits marked `immutable` in `log` cannot be rewritten.
- Bookmarks are git branches. They do not move when you commit; move them
  with `bookmark_set` before pushing.
- Conflicts do not stop operations. They are recorded in commits (`conflict`
  in `log`, listed by `status`) and resolved by editing the files.
- Every operation is recorded. `undo` reverts the last one; `op_log` shows
  the history.

Publishing: point a bookmark at the finished commit (`bookmark_set` with
`revision: "@-"` after `commit`), then `git_push` with that bookmark. Every
pushed commit is signed and each signature may ask the user to touch a
security key: run `git_push` with `dry_run: true` first and tell the user
how many commits will be signed (`commits to sign: N`).

For a jj command no tool covers (`bookmark delete`, `bookmark track`,
`duplicate`, `file track`, ...), use `run` with the arguments as a list,
for example `["bookmark", "delete", "old"]`. Interactive commands fail there.

Revisions use jj's revset language: `@` (working copy), `@-` (its parent),
`main`, `feat@origin`, `root()..@`, `trunk()`. Paths in `paths` are taken
literally, relative to `repo`.
