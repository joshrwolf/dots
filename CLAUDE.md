# dots

GNU stow-managed dotfiles. Each top-level directory is a stow package whose
contents mirror `$HOME`, so `claude/.claude/CLAUDE.md` here *is*
`~/.claude/CLAUDE.md` — edits to it are global to every project, not local to
this one. Repo-specific rules belong in this file instead.

`PROJECTS` in the Makefile lists the directories that are **not** packages
(`tools`, `herdr-plugins`). A root-level `.stow-local-ignore` cannot exclude
them — stow only reads that file from inside a package.

Run `make`, never `stow` directly. It builds what needs building, restows every
package, and then links the herdr plugins, in that order.

## This repo is public

Never commit an employer name, an internal repository or project name, or an
internal hostname or URL — not in code, comments, docs, config, or test
fixtures. (`joshrwolf/...` is my own public namespace and is fine.)

The trap is captured output. Never commit a live capture of anything that
observed this machine — an API snapshot, a process listing, a log file, shell
history. Read the real thing to get its shape right, then hand-author the
fixture with neutral names. That also covers the cases the live session
happened not to contain, which a capture never does.
