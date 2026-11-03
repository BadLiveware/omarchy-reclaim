# reclaim

Reclaims disk space from things that pile up during development and are hard to judge by hand.

| What | Deleted when |
|---|---|
| Cargo `target/`, .NET `bin/` and `obj/`, `node_modules/`, any directory with a `CACHEDIR.TAG` | Not touched for 14 days and no process holds a file in it |
| Git worktrees (manual, `.worktrees/`, `.claude/worktrees/`, Cursor) | Idle for 7 days, not locked, no process in it, no uncommitted or untracked changes, only rebuildable ignored files, and every commit is on a remote or in a merged or closed pull request |
| Treehouse pool worktrees | Treehouse itself decides (`treehouse prune --all --yes`) |
| Docker | Unused volumes at once; unused images and build cache after 7 days |

A worktree that is not safe to delete is kept.
After 30 idle days it is reported as needing a decision.

A build output counts as used when a file in it was written or read, or when its project folder was edited.
Reads come from file access times: with the `relatime` mount option the kernel records a read at most once a day, which is enough to see `npm run dev` or a running binary.

"In use" means a process has its working directory, its executable or a memory-mapped file inside the path.
Idle time comes from modification times: of the build output tree, and of the worktree directory and its git `HEAD`, index and reflog.
Git updates these on almost every command.

## Use

```sh
make install          # binary, systemd user units and default config
reclaim status        # dry run: what would be deleted, what needs a decision, and why
reclaim scan          # dry run that only refreshes the snapshot the bar widget shows
reclaim status --all  # also list everything that is kept
make enable           # daily run, plus a check every 15 minutes for low free space
```

`reclaim run` deletes. The timers run it with idle CPU and IO priority.
It sends a desktop notification when it freed space, when worktrees need a decision, or when disk space stays low.
Logs: `journalctl --user -u reclaim -u reclaim-low-disk`.

Configuration: `~/.config/reclaim/config.toml`, see `config.example.toml`.

## Omarchy bar widget

```sh
make install-widget
omarchy plugin enable fl.reclaim --section right
```

Hover shows free space, reclaimable space and open decisions.
Click opens a panel with disk usage, the last scan and run, reclaimable space per kind, and the worktrees that need a decision; clicking one opens a terminal in it.
The panel buttons start `reclaim-scan.service` and `reclaim.service`, and the switch enables or disables the timers.

The widget never scans by itself.
It reads `plan.json` and `last-run.json` in `$XDG_STATE_HOME/reclaim/`, which `reclaim scan` and `reclaim run` write.
`make install-widget` restarts the shell: its plugin hot reload can keep old compiled QML and scripts.

Claude Code session logs are not handled here: Claude Code deletes them itself after `cleanupPeriodDays` (default 30).

## Test

```sh
make test
```

The end-to-end test builds real git repositories, worktrees and a remote, holds one worktree with a running process, and checks what `reclaim run` deletes.
