# Setting up go-with-me

This takes about 10 minutes for two Macs. Want to look around first? `./go-with-me demo` needs no setup.

- [1. Install (on every Mac)](#1-install-on-every-mac)
- [2. Name your first Mac](#2-name-your-first-mac)
- [3. Add your other Macs](#3-add-your-other-macs)
- [4. Open the hub](#4-open-the-hub)
- [5. Use it away from home (Tailscale)](#5-use-it-away-from-home-tailscale)
- [6. Optional extras](#6-optional-extras): work + personal accounts, local models, VMs, always-on server
- [Troubleshooting](#troubleshooting)

---

## 1. Install (on every Mac)

You only need to do this on the Mac you start from. `gwm add` installs go-with-me on the others for you, but they need **Homebrew** and **Remote Login**.

```bash
# Homebrew, if you don't have it: https://brew.sh
brew install zellij tmux
git clone https://github.com/Vayras/go-with-me ~/go-with-me && ~/go-with-me/install.sh
```

Open a new terminal window afterwards so `gwm` is on your PATH.

**On every Mac you want to reach**, turn on **Remote Login**:
System Settings → General → Sharing → **Remote Login** → on.
Click **ⓘ** and make sure *Allow access for* includes your user (or *All users*).

You'll also want [Claude Code](https://claude.com/claude-code) on the Macs where you'll run Claude sessions.

## 2. Name your first Mac

```bash
gwm init
```

It asks for a short name (e.g. `studio`), works out this Mac's address, creates `~/.config/go-with-me/`, makes an SSH key if you don't have one, and adds a Claude tab for this Mac.

## 3. Add your other Macs

```bash
gwm add
```

The wizard:

1. **Finds your Macs.** It lists Tailscale machines and Macs on your Wi-Fi with Remote Login on. Pick a number, or type an address.
2. **Checks it can connect.** If not, it tells you to turn on Remote Login there.
3. **Asks your username on that Mac.** It can differ between Macs! Run `whoami` on that Mac if unsure.
4. **Sets up SSH keys.** The first time, it asks for that Mac's password **once**, then never again.
5. **Sets up the other direction too**, so the other Mac can reach this one.
6. **Installs tmux** there if it's missing (via Homebrew).
7. **Installs go-with-me** there with the same config, so you can open the hub from either Mac.
8. **Adds a Claude tab** for it.

```
$ gwm add
Looking for machines (Tailscale + this Wi-Fi)…
  1) Alexs-MacBook-Pro.local              same Wi-Fi · Alex's MacBook Pro
Pick a number, or type an address [1]:
✓ Alexs-MacBook-Pro.local is reachable
Your username on that Mac [alex]: alexk
✓ Logged in as alexk
✓ Alexs-MacBook-Pro · 26.6.2 · 24GB · tmux ✓ claude ✓ opencode ✗ ollama ✓
Short name for it [macbook]:
✓ Saved as 'macbook' → alexk@Alexs-MacBook-Pro.local
✓ 'macbook' can reach this Mac too
✓ go-with-me installed on 'macbook' (run gwm there)
✓ Added session 'macbook'
```

Repeat for every Mac. Check everything with:

```bash
gwm doctor
```

## 4. Open the hub

```bash
gwm
```

- **Tab 1 "overview"**: the board (every machine and session) plus the coordinator Claude
- **Tabs 2, 3, …**: one per running session, numbered by position

| | |
|---|---|
| Switch tabs | click them, or **Alt+1…9** / **Alt+←→** (Terminal.app: Settings → Profiles → Keyboard → *Use Option as Meta key*) |
| Zellij's own keys | **Ctrl+g** first, then follow the bar at the bottom. Until then, all keys go to Claude |
| Leave (everything keeps running) | **Ctrl+g → o → d**, or just close the window |
| Start / open / close sessions | `gwm new` · `gwm open <name>` · `gwm close <name>` |

The first time a Claude tab opens, Claude asks whether to trust the folder. Pick **Yes**.

## 5. Use it away from home (Tailscale)

`.local` names only work on the same Wi-Fi. [Tailscale](https://tailscale.com) gives every Mac a name that works from anywhere, encrypted and free for personal use.

**On each Mac:**
```bash
brew install --cask tailscale-app     # then open Tailscale and sign in with the same account everywhere
```
(For an always-on server without the app: `brew install tailscale && sudo brew services start tailscale && sudo tailscale up`.)

**Then re-add your Macs**, and pick the Tailscale entries this time (they show as `Tailscale · macOS · online`):
```bash
gwm add macbook          # same name → replaces the old address
```
`gwm init` and `gwm add` prefer Tailscale names automatically once Tailscale is running.

## 6. Optional extras

**Work + personal Claude accounts.** Each account label gets its own login:
```bash
gwm new work-api --on studio --engine claude --account work --folder ~/work
gwm login work --on studio          # or just sign in inside the new tab
```

**Local models (no cloud tokens).** Use Ollama and opencode on your strongest Mac:
```bash
gwm run studio "brew install ollama opencode && brew services start ollama"
gwm pull qwen3.8-27b --on studio
gwm opencode-setup --on studio
gwm new local --on studio --engine opencode
```

**Linux VMs (sandboxes)** on your strongest Mac, via Lima:
```bash
gwm run studio "brew install lima"
gwm vm create sandbox --on studio --cpus 6 --memory 16
```

**Keep a server Mac always reachable:**
```bash
sudo pmset -a sleep 0          # never sleep (display can still sleep)
sudo pmset -a autorestart 1    # power back on after an outage
```

## Troubleshooting

`gwm doctor` checks every machine and says what's missing. Common errors:

| You see | Fix |
|---|---|
| `isn't accepting SSH connections` / `Connection refused` | Turn on **Remote Login** on that Mac |
| `Permission denied (publickey…)` | Wrong username (run `whoami` there), or Remote Login only allows some users (Sharing → Remote Login → ⓘ). Re-run `gwm add` |
| `Host key verification failed` | That Mac was reinstalled or its address now points at a different machine. Remove the old line from `~/.ssh/known_hosts` (`ssh-keygen -R <address>`) |
| A machine shows **offline** when you're away | It's using a `.local` address. Set up Tailscale (step 5) |
| A machine goes **offline** after a few minutes | It's asleep: `sudo pmset -a sleep 0` on that Mac |
| `claude: command not found` in a tab | Install Claude Code on that Mac. go-with-me already looks in `~/.local/bin` and Homebrew |
| A tab says *Disconnected* | Press **Enter** to reconnect. The session kept running on its Mac |
| Something's odd with the hub | `gwm kill` then `gwm`. Sessions are not affected |
