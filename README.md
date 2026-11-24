# go-with-me

**One terminal for all your machines, Claude accounts, VMs and local models.**

Your Mac Studio, your work laptop, an office box and a Linux VM become tabs in one window. A live board shows what every machine and agent is doing, and a coordinator Claude can summarise everything and hand work to the right machine.

![go-with-me demo](docs/demo.gif)

```bash
git clone https://github.com/Vayras/go-with-me && cd go-with-me && ./go-with-me demo
```
☝️ **Live demo, no setup.** It opens the real hub with pretend machines and agents. No SSH, no logins, and nothing on your computer is touched. (Needs [Zellij](https://zellij.dev): `brew install zellij`.)

---

## What you get

<table>
<tr>
<td width="50%"><img src="docs/hub-overview.png" alt="Sidebar and coordinator"></td>
<td width="50%"><img src="docs/session-tab.png" alt="A session"></td>
</tr>
<tr>
<td><b>Just a sidebar and a terminal.</b> <b>This Mac</b> gets a full card: CPU / GPU / RAM bars, <b>every loaded model</b> and its size, VMs, which other devices are using it, and its running sessions. <b>Other machines</b> get compact cards with their models and sessions. Only sessions that are <i>running</i> are listed; saved ones collapse to one line. Tab 1 is the <b>coordinator</b>: a terminal where typing <code>claude</code> gives you an assistant that can see and steer every machine.</td>
<td><b>Click a session to jump in</b> (a stopped one starts). Each session runs on its own machine and <b>keeps running</b> when you close your laptop. <b>Alt+1…9</b> also jumps; <b>n</b> new, <b>x</b> close. Select text with the mouse to copy it.</td>
</tr>
</table>

A **session** is *machine + engine + account + folder*:

| | |
|---|---|
| **Machines** | Any Mac (or Linux box), connected over [Tailscale](https://tailscale.com) (works anywhere, no Remote Login) or plain SSH with Remote Login |
| **Terminals, not auto-launched apps** | Every tab is a plain terminal; **nothing starts on its own**. A session's engine (`claude`, `opencode` with local [Ollama](https://ollama.com) models, or `shell`) just labels what it's for and shows a hint; you type `claude` / `opencode` when you want it |
| **Accounts** | Several Claude logins side by side: in a `work` tab, typing `claude` uses your work login, and in a `personal` tab your personal one |
| **Web** | The same hub in a browser on any of your Tailscale devices (`gwm web`): same sidebar, same terminals, same keys. Tailnet-only HTTPS plus a login token |
| **VMs** | `gwm vm create sandbox --on studio` makes a Linux VM ([Lima](https://lima-vm.io)) on your strongest machine; it shows up as a machine with its own sessions |

<img src="docs/status.png" alt="gwm status" width="100%">

Outside the hub, `gwm status` prints the same information as a table, and `gwm who` lists which devices are connected to which machine.

## Quick start

```bash
brew install zellij tmux
git clone https://github.com/Vayras/go-with-me ~/go-with-me && ~/go-with-me/install.sh   # adds `gwm`
gwm tailscale   # optional, recommended: connect your Macs from anywhere, no Remote Login needed
gwm init        # name this Mac
gwm add         # add your other Macs (Tailscale or Remote Login): sets up access both ways, installs everything there
gwm             # open the hub
```

**Two ways to connect your Macs:** **Tailscale** (works anywhere, no Remote Login, no SSH keys) or **Remote Login** (plain SSH on the same Wi-Fi). Mix and match. **📖 Full guide and troubleshooting: [docs/SETUP.md](docs/SETUP.md)**

## Everyday use

| Do this | Command |
|---|---|
| Open the hub (only running sessions get a tab; nothing new is started) | `gwm` |
| Start a new session (asks machine / engine / account / folder) | `gwm new` |
| Open a tab for a session that isn't running | `gwm open <session>` |
| Close a session and its tab (its screen is saved to `~/.config/go-with-me/archive/` first; Claude chats are kept) | `gwm close <session>` |
| Stop it but keep the tab (Enter in the tab restarts it) | `gwm stop <session>` |
| See everything at a glance / who's using what | `gwm status` · `gwm who` |
| Read or type into a session from anywhere | `gwm peek <session>` · `gwm send <session> "text"` |
| Read a session's whole Claude conversation (the coordinator does this for every chat by default) | `gwm history <session>` · `gwm history --all` |
| Add / remove a machine | `gwm add [--via tailscale\|ssh]` · `gwm remove <machine>` |
| Set up Tailscale on this Mac | `gwm tailscale` |
| Run a command on a machine | `gwm run <machine> "git status"` |
| Sign a Claude account in on a machine | `gwm login work --on studio` |
| Linux VMs | `gwm vm create <name> --on <machine> [--cpus 6 --memory 16]` · `gwm vm list` |
| Local models | `gwm models --on studio` · `gwm pull <model>` · `gwm opencode-setup --on studio` |
| Copy text | Select it with the mouse; it's copied automatically. Paste with Cmd+V |
| Open the hub in a browser (your tailnet only) | `gwm web` · `gwm web stop` · `gwm web token` |
| Leave the hub (everything keeps running) | **⌥Q**, or close the window |

**Shortcuts** (⌥ = Option; in Terminal.app run `gwm keys --setup` once so ⌥ works):

| | |
|---|---|
| **⌥1…9** · **⌥← ⌥→** | jump to a tab · previous / next |
| **⌥S** | switch between sidebar and terminal |
| **⌥N** · **⌥X** · **⌥Z** | new pane · close pane · zoom pane |
| **⌥O** | open or create a session |
| **⌥Q** | leave (everything keeps running) |
| **⌥/** | show all shortcuts (`gwm keys`) |

Click anything in the sidebar too. Select text to copy it.

## Web access

```bash
gwm web            # prints https://<this-mac>.<tailnet>.ts.net:8443/gwm and, the first time, a login token
gwm web stop       # stop sharing (your hub and sessions keep running)
```

It's the same hub (sidebar + terminals) through [Zellij's web client](https://zellij.dev), with four layers of protection:

1. **Local only.** The web server listens on `127.0.0.1`, so nothing on your Wi-Fi or the internet can connect to it. `gwm web` refuses to continue if it's ever bound wider.
2. **Your tailnet only.** [Tailscale Serve](https://tailscale.com/kb/1312/serve) publishes it over HTTPS with a real `*.ts.net` certificate to the devices your tailnet's access rules allow to reach this Mac (just yours, in a personal tailnet; check your rules if you share nodes or have teammates). It's Serve, not Funnel, so it's never on the public internet. `gwm web stop` always removes the route; it would otherwise survive reboots.
3. **Login token.** Every browser logs in with a token (`gwm web token`). Use `--read-only` for watch-only access, and `gwm web revoke <name>` revokes one; revoking also logs out browsers already using it.
4. **Only the hub is shared.** Web sharing is enabled for the go-with-me hub, not for your other terminal sessions.

A token is a terminal on that Mac, so treat it like a password. One-time setup: turn on **HTTPS Certificates** in your [Tailscale DNS settings](https://login.tailscale.com/admin/dns).

## Config

`~/.config/go-with-me/hosts.conf`
```
# name   | ssh target         | role (the coordinator reads this)
studio   | you@my-mac-studio  | Home Mac Studio, 64GB. Always on. Local models, VMs.
macbook  | you@my-macbook     | Work laptop; may be asleep.
sandbox  | vm:studio/sandbox  | Linux VM on studio
```
`~/.config/go-with-me/sessions.conf`
```
# name        | machine | engine   | account | folder
site          | studio  | claude   | default | ~/code/site
work-api      | studio  | claude   | work    | ~/work/api
local         | studio  | opencode | -       | ~/code
```
`gwm init`, `gwm add`, `gwm new` and `gwm close` edit these for you, and `gwm add` copies them to your other machines.

## How it works

Plain bash, no daemon or server:

- **Zellij** draws the hub on the machine you're sitting at: a Python **sidebar** (curses) beside each session's terminal.
- Each session lives in a **hidden tmux** on its own machine (no status bar, no prefix key; it's only there so sessions survive disconnects). Tabs reconnect to it over **SSH**.
- The sidebar's data comes from one shared SSH sweep of every machine (cached a few seconds): CPU/RAM, Ollama's loaded models, Lima VMs, tmux sessions, and incoming SSH/Tailscale connections (that's **in use**).
- **Accounts** = a separate `CLAUDE_CONFIG_DIR` per label on each machine.
- The **coordinator** tab is a terminal in `coordinator/`; when you run `claude` there it reads `CLAUDE.md` and uses `gwm status / peek / send / run`. It asks before sending to a session or changing anything.

## Similar projects

The pieces of go-with-me exist elsewhere; the combination is what's different. Worth a look:

| Project | Overlap |
|---|---|
| [herdr](https://github.com/ogulcancelik/herdr) | The closest: a terminal multiplexer for agents, with remote machines over SSH and agent status per pane. Much more mature. |
| [AI Maestro](https://github.com/23blocks-OS/ai-maestro), [Codeman](https://github.com/Ark0N/Codeman), [ADHDev](https://github.com/vilmire/adhdev), [Agent Deck](https://github.com/asheshgoplani/agent-deck) | Multi-machine agent dashboards (mostly web UIs) |
| [Claude Code Remote Control](https://code.claude.com/docs/en/remote-control) | Anthropic's way to reach your sessions from claude.ai and your phone |
| [ccswap](https://github.com/vvinhas/ccswap), [claude-switch](https://github.com/hoangvu12/claude-switch) | Claude multi-account switching on one machine |

**go-with-me's angle:** a *machine-level* board (resources, local models, VMs, who's connected), **per-session Claude accounts** mixed across machines, **local models and Lima VMs as first-class citizens**, and a coordinator that works over your own SSH. It's all in the terminal and simple enough to read in one sitting.

**Not there yet:** phone and web access, "needs your input" alerts, git worktree isolation, cost tracking, and restoring layouts after a reboot. PRs welcome.

## Security notes

- **The coordinator reads every Claude chat** on every machine and account (including work accounts) by default, and it can run commands everywhere. Text it reads could try to steer it (prompt injection), so `coordinator/.claude/settings.json` makes Claude **always ask you** before `gwm run`, `gwm send`, `ssh`, or anything that creates, closes or deletes. Keep it that way, and avoid auto mode in the coordinator.
- `gwm send` / `gwm run` execute on your machines over SSH. Treat the coordinator like any agent with shell access.
- Lima VMs mount your home folder **read-only** by default, which makes them good sandboxes.
- Check your employer's policy before joining work machines or work accounts.

## Status

Early, and built for macOS (Linux works for VMs and remote machines). The VM commands are new and lightly tested. Issues and PRs welcome.

## License

MIT
