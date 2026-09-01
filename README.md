# ssh-paste

Paste your local clipboard — screenshots included — into remote SSH sessions. Built for Ctrl+V in Claude Code on headless servers.

## Install

```sh
cargo install --git https://github.com/k27dong/ssh-paste
```

## Setup

```sh
ssh-paste setup <ssh-alias>   # sets up the remote, verifies both paths, offers the ssh config lines
ssh-paste serve               # keep running
```

Reconnect your ssh session, then paste with Ctrl+V on the remote.

## Fallback

```sh
ssh-paste send [target]       # one-shot push when the tunnel is down
ssh-paste remove <target>     # uninstall a remote
```

## Notes

- While a session with the forward is open, that remote can read your clipboard on demand. Everything binds loopback on both machines.
- On shared remotes, pick a unique `--port`.
