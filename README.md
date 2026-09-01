# ssh-paste

Copy on your machine, press Ctrl+V in a program on a remote machine — including
Claude Code on a headless server, where Ctrl+V attaches your screenshot
natively. No manual step in between.

## How it works

`ssh-paste serve` runs on your local machine and answers one question over a
loopback-only port: "what is on the clipboard right now?" One `RemoteForward`
line in your ssh config relays that port onto the remote's loopback through
the ssh session you are already inside. On the remote, two tiny generated
shims named `xclip` and `wl-paste` — exactly what Claude Code invokes to read
a clipboard on Linux — fetch from it the moment Ctrl+V fires.

If the tunnel is down (no session, `serve` not running), the shims fall back
to a spool you can fill explicitly with `ssh-paste send` — the same tool,
push instead of pull.

## Install (local machine)

    cargo install --git https://github.com/k27dong/ssh-paste

Requires `ssh` on PATH (stock on macOS, Linux, Windows 10+). The remote needs
only POSIX sh and `curl`.

## Set up a remote

    ssh-paste setup <ssh-alias>

One command: installs the shims, verifies PATH resolution, live-tests BOTH
paths (a pull round-trip through a real reverse tunnel, and a push round-trip
through the spool), registers the target, and prints the exact ssh config
line to add:

    Host <ssh-alias>
      RemoteForward 7717 127.0.0.1:7717

Add it, reconnect, and keep the server running locally:

    ssh-paste serve

To keep `serve` running automatically: macOS — a Login Item or a
launchd user agent running `ssh-paste serve`; Linux — a systemd user unit;
Windows — a Startup shortcut. It is a plain foreground process; anything that
starts it works.

## Use

Copy anything — screenshot (Cmd+Shift+4) or text — then press Ctrl+V in the
remote program. That's it.

Fallback / one-shot push (works with no tunnel and no serve):

    ssh-paste send            # default target
    ssh-paste send pod2       # named target

## Config

Your OS's standard config directory — `~/.config/ssh-paste/config.toml` on
Linux, `~/Library/Application Support/ssh-paste/config.toml` on macOS,
`%APPDATA%\ssh-paste\config.toml` on Windows:

    default_target = "pod"

    [targets.pod]
    host = "hermes-pod"
    # spool_dir = "~/.cache/ssh-paste"
    # shim_dir  = "~/.local/bin"
    # pull_port = 7717

Setup flags: `--name` registers under a different target name, `--force`
overrides conflicts with existing clipboard tools (each override is printed),
`--port` picks a different pull port. `SSH_PASTE_SSH` overrides which ssh
binary is used. After hand-editing `spool_dir`, `shim_dir`, or `pull_port`,
re-run `ssh-paste setup` — the shims bake those values in.

## Security notes

- While a session with the forward is open, that remote can read your local
  clipboard on demand — that is what "nothing in between" costs. Only hosts
  you add the `RemoteForward` line for get this; everything binds loopback
  on both machines, nothing is reachable from any network.
- Nothing is cached: `serve` reads the clipboard only when asked. The spool
  holds one explicitly sent item (0600) until your next send or
  `ssh-paste remove <target>` — remember that before `send`ing secrets.
- Transport is your own ssh: same keys, agent, host verification.

## Uninstall a remote

    ssh-paste remove <target>
