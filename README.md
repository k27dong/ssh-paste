# ssh-paste

Paste your local clipboard — screenshots included — into programs on a remote
machine over plain ssh. Built so Ctrl+V in Claude Code on a headless server
attaches what you copied locally.

## How it works

`ssh-paste send` reads your clipboard (image wins over text), streams it over
your own `ssh` into a one-item spool on the remote
(`~/.cache/ssh-paste/clip.png` or `clip.txt`). On the remote, two tiny
generated shims named `xclip` and `wl-paste` answer clipboard reads from that
spool — which is exactly how Claude Code looks for clipboard content on
Linux. No daemons, no ports, no tunnels; every transfer is an explicit send.

## Install (local machine)

    cargo install --git https://github.com/k27dong/ssh-paste

Requires `ssh` on PATH (stock on macOS, Linux, and Windows 10+).

## Set up a remote

    ssh-paste setup <ssh-alias>

One command: installs the shims into `~/.local/bin` on the remote, verifies
PATH, runs a live probe, and registers the target in your OS's standard
config directory — `~/.config/ssh-paste/config.toml` on Linux,
`~/Library/Application Support/ssh-paste/config.toml` on macOS, or
`%APPDATA%\ssh-paste\config.toml` on Windows. The first target becomes the
default.

## Use

    ssh-paste send            # default target
    ssh-paste send pod2       # named target

Then press Ctrl+V in Claude Code on the remote.

## Hotkey recipes

macOS (skhd): `cmd + shift - v : ssh-paste send`
macOS (Raycast): script command running `ssh-paste send`
Linux (sxhkd): `super + shift + v` → `ssh-paste send`
Windows (AutoHotkey): `#+v::Run "ssh-paste send"`

## Security notes

- Nothing on your machine is remotely readable; there is no listener.
- Sent content sits in the remote spool (0600) until your next send or
  `ssh-paste remove <target>` — remember that before sending secrets.
- Transport is your own ssh config: same keys, same host verification.

## Uninstall a remote

    ssh-paste remove <target>
