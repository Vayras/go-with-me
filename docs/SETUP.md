# Setting up go-with-me

This takes about 10 minutes for two Macs. Want to look around first? `./go-with-me demo` needs no setup.

- [1. Install](#1-install)
- [2. Choose how your Macs connect](#2-choose-how-your-macs-connect): **Tailscale** (recommended) or **Remote Login**
- [3. Name your first Mac](#3-name-your-first-mac)
- [4. Add your other Macs](#4-add-your-other-macs)
- [5. Open the hub](#5-open-the-hub)
- [6. Optional extras](#6-optional-extras): work + personal accounts, local models, VMs, always-on server
- [Troubleshooting](#troubleshooting)

---

## 1. Install

On the Mac you start from (`gwm add` installs it on the others for you):

```bash
# Homebrew, if you don't have it: https://brew.sh
brew install zellij tmux
git clone https://github.com/Vayras/go-with-me ~/go-with-me && ~/go-with-me/install.sh
```

Open a new terminal window afterwards so `gwm` is on your PATH. You'll also want [Claude Code](https://claude.com/claude-code) on the Macs where you'll run Claude sessions.

## 2. Choose how your Macs connect

| | **A. Tailscale** (recommended) | **B. Remote Login** |
|---|---|---|
| Works away from home | ✅ anywhere | ❌ same Wi-Fi only (unless you set up your own VPN) |
| macOS Remote Login | not needed | must be on for every Mac |
| SSH keys | none; Tailscale vouches for you | set up for you by `gwm add` (password once) |
| Needs | a free [Tailscale](https://tailscale.com) account | nothing extra |

You can mix them: some Macs over Tailscale, some over Remote Login.

### A. Tailscale

**On this Mac:**
```bash
gwm tailscale
```
It installs the open-source Tailscale, starts it (asks for your Mac password), signs you in (opens a link), and turns on **Tailscale SSH**.

**On each other Mac** (go-with-me isn't there yet, so use the plain commands):
```bash
brew install tailscale
sudo brew services start tailscale
sudo tailscale up --ssh          # sign in with the SAME Tailscale account
```

> **Why the command-line Tailscale and not the app?** On a Mac, only the open-source `tailscaled` can *accept* SSH logins. The App Store and standalone apps can only connect *out*. If you have the app, quit it first.

**One-time policy tweak.** By default Tailscale asks you to re-login in a browser every 12 hours before each SSH ("check" mode), which would break the live board. In the [admin console → Access controls](https://login.tailscale.com/admin/acls), set the `ssh` rule to `accept` for your own devices:

```jsonc
"ssh": [
  {
    "action": "accept",                 // was "check"
    "src":    ["autogroup:member"],
    "dst":    ["autogroup:self"],
    "users":  ["autogroup:nonroot"]       // never "root": gwm doesn't need it
  }
]
```

### B. Remote Login

**On every Mac you want to reach:** System Settings → General → Sharing → **Remote Login** → on. Click **ⓘ** and make sure *Allow access for* includes your user (or *All users*). That's it; `gwm add` does the keys.

## 3. Name your first Mac

```bash
gwm init
```

It asks for a short name (e.g. `studio`), works out this Mac's address (the Tailscale name if Tailscale is on), creates `~/.config/go-with-me/` and makes an SSH key if needed. It doesn't start or save any sessions; you create those when you want them with `gwm new`.

## 4. Add your other Macs

```bash
gwm add                    # asks Tailscale or Remote Login
gwm add --via tailscale    # or choose up front
gwm add --via ssh
```

The wizard:

1. **Finds your Macs**: your Tailscale machines, or Macs on your Wi-Fi with Remote Login on. Pick a number, or type an address.
2. **Checks it can connect**, and tells you exactly what to turn on if not.
3. **Asks your username on that Mac.** It can differ between Macs! Run `whoami` there if unsure.
4. **Logs in.** Tailscale: no keys needed. Remote Login: copies this Mac's key (asks that Mac's password **once**).
5. **Checks the other direction**, so the other Mac can reach this one.
6. **Installs tmux** there if missing.
7. **Installs go-with-me** there with the same config, so you can open the hub from either Mac.
8. **Optionally saves a Claude session** on it (it asks; the default is no, since nothing is started unless you want it).

```
$ gwm add --via ssh
Looking for machines…
  1) Alexs-MacBook-Pro.local              same Wi-Fi · Alex's MacBook Pro
Pick a number, or type an address [1]:
✓ Alexs-MacBook-Pro.local is reachable (Remote Login)
Your username on that Mac [alex]: alexk
✓ Logged in as alexk
✓ Alexs-MacBook-Pro · 26.6.2 · 24GB · tmux ✓ claude ✓ opencode ✗ ollama ✓
Short name for it [macbook]:
✓ Saved as 'macbook' → alexk@Alexs-MacBook-Pro.local
✓ 'macbook' can reach this Mac too
✓ go-with-me installed on 'macbook' (run gwm there)
Also save a Claude Code session on 'macbook'? (y/n) [n]:
```

Switching a Mac from Remote Login to Tailscale later? Run `gwm add <same-name> --via tailscale`; it replaces the old address. Check everything with `gwm doctor`.

## 5. Open the hub

```bash
gwm
```

- **The sidebar** (left): *This Mac* in detail (CPU/GPU/RAM, every loaded model, VMs, who's connected, its sessions) and a compact card per other machine. Only running sessions are listed; click to jump
- **Tab 1, the coordinator**: a terminal. Type `claude` there for an assistant that can see and steer everything
- **Every tab is a plain terminal**: nothing is launched for you. A Claude-type tab shows a hint and has its account set up, so `claude` uses the right login
- **Tabs 2, 3, …**: one per running session, numbered by position (shown in the sidebar)

| Shortcut | Does |
|---|---|
| **⌥1…9**, **⌥← ⌥→** | jump to a tab, previous / next |
| **⌥S** | switch between sidebar and terminal |
| **⌥N** / **⌥X** / **⌥Z** | new pane / close pane / zoom pane |
| **⌥O** | open or create a session |
| **⌥Q** | leave the hub (everything keeps running) |
| **⌥/** | all shortcuts |

⌥ is the Option key. In **Terminal.app**, run `gwm keys --setup` once and open a new window (it turns on *Use Option as Meta key*). iTerm2: Profiles → Keys → Left Option: *Esc+*. Ghostty: `macos-option-as-alt = true`.

The first time you run `claude` in a folder, it asks whether to trust it. Pick **Yes**.

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

**Open the hub in a browser** (phone, iPad, another laptop on your tailnet):
```bash
gwm web            # first time: turn on "HTTPS Certificates" at https://login.tailscale.com/admin/dns
```
It prints a `https://…ts.net:8443/gwm` link and a login token (shown once, so save it). The server only listens on this Mac; Tailscale Serve publishes it to your tailnet only (never the public internet). Use `gwm web token --read-only` for watch-only access, `gwm web revoke <name>` to revoke a token, and `gwm web stop` to stop sharing.

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
| A machine shows **offline** when you're away | It's using a `.local` address (Remote Login). Switch it to Tailscale: `gwm add <name> --via tailscale` |
| `Tailscale SSH refused the login` | Set the tailnet `ssh` rule to `"action": "accept"` (step 2A), or check the username |
| Tailscale mode: `isn't accepting SSH connections` | Tailscale SSH is off on that Mac: `sudo tailscale set --ssh` there. The Tailscale *app* can't do this; use the command-line `tailscale` |
| A machine goes **offline** after a few minutes | It's asleep: `sudo pmset -a sleep 0` on that Mac |
| `claude: command not found` in a tab | Install Claude Code on that Mac. go-with-me already looks in `~/.local/bin` and Homebrew |
| A tab says *Disconnected* | Press **Enter** to reconnect. The session kept running on its Mac |
| Something's odd with the hub | `gwm kill` then `gwm`. Sessions are not affected |
