# ocpop

Waybar widget for [opencode](https://opencode.ai) sessions: watch what the
agents are doing and hop straight back into any of them.

## What it does

**`ocpop waybar`** — the bar module. Shows the session count; the tooltip
combines two things:

- **OpenCode Go usage** — 5h / 7d / 30d pace bars rendered with
  (refetched at most every 60s, cached in `/tmp/ocpop-usage.json`)
- **Session list** — sessions touched since the last boot, monospace for
  alignment, state shown as a colored dot:

| Color | State   | Meaning                                      |
| ----- | ------- | -------------------------------------------- |
| green | running | agent is working (pending turn / no done)    |
| pink  | waiting | needs your input (permission or fresh reply) |
| white | idle    | untouched for a while                        |

**`ocpop pick`** — the click action. A fuzzel menu of sessions with the same
state icons; picking one focuses the terminal window already running that
session (`niri msg action focus-window`), or opens a fresh alacritty resumed
into it (`opencode -s <id>` in the right directory). ESC cancels.

State is derived from the opencode sqlite db (read-only, via `opencode db`):
`session.permission` non-null, pending user turns, and unfinished assistant
messages.

## Install

```sh
cargo install --path .
```

## Wire into waybar

```jsonc
"custom/ocpop": {
  "exec": "~/.cargo/bin/ocpop waybar",
  "return-type": "json",
  "interval": 5,
  "on-click": "~/.cargo/bin/ocpop pick",
},
```

Optional styling

```css
#custom-ocpop.oc-waiting {
  color: @pink;
}
#custom-ocpop.oc-running {
  color: @green;
}
#custom-ocpop.error {
  color: @red;
}
```

## Debug

```
ocpop list    # what the widget sees, one session per line
ocpop waybar  # raw waybar JSON
```
