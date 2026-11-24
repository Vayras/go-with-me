# go-with-me

**One terminal for all your machines.** A tmux-based hub that joins your Macs, Linux VMs, multiple Claude accounts and local models into a single window:

```
 go-with-me  0 overview  1 studio  2 studio-work  3 studio-local  4 macbook      click a tab · Alt+0-9 · Ctrl+a d = leave
┌ STATUS BOARD ─────────────────────────────────────────────────────────────────────────────────────┐
│ HOST      STATUS        CPU  RAM     MODELS LOADED     VMs                                        │
│ studio    ● on (here)    6%  13/64G  inkling-2-mini    sandbox:Running                            │
│ macbook   ● on           9%  14/48G  -                                                            │
│ sandbox   ● vm           1%   1/8G   -                                                            │
│                                                                                                   │
│ SESSION       HOST     ENGINE    ACCOUNT   STATE        CURRENT TASK                              │
│ studio        studio   claude    default   working      Landing page hero section                 │
│ studio-work   studio   claude    work      idle 4m *    Fix flaky API tests                       │
│ studio-local  studio   opencode  -         idle 12m     OpenCode                                  │
│ macbook       macbook  claude    default   idle 1h                                                │
├ COORDINATOR — ask about or delegate to any device ────────────────────────────────────────────────┤
│ > summarise everything                                                                            │
│ > when studio-work's tests pass, tell the sandbox session to run the load test                    │
└───────────────────────────────────────────────────────────────────────────────────────────────────┘
```

- **Tab 0, overview:** a live board of every host and session, plus a **coordinator** Claude that can read any session's screen, summarise everything, and hand work to the right machine.
- **One tab per session.** A session is **host + engine + account + folder**:
  - **engine:** `claude` (Claude Code), `opencode` (local models via Ollama) or `shell`
  - **account:** separate Claude logins side by side (e.g. personal + work), each fully isolated
- **Sessions persist.** They live in tmux *on their host*, so closing your laptop doesn't stop them. Run `gwm` again to pick up where you left off.
- **VMs:** create Linux VMs (via [Lima](https://lima-vm.io)) on your strongest machine; they appear as hosts with their own sessions.
- **Local models:** see what's loaded where, pull models, and point opencode at any host's Ollama.

It's plain bash + tmux + ssh. There's no server and no daemon.

## Requirements

- macOS (hosts can also be Linux, e.g. VMs), `tmux`, `ssh` between machines (key-based)
- [Tailscale](https://tailscale.com) recommended, so hosts reach each other from anywhere
- Optional: [Claude Code](https://claude.com/claude-code), [opencode](https://opencode.ai), [Ollama](https://ollama.com), [Lima](https://lima-vm.io)

## Install

```bash
git clone https://github.com/<you>/go-with-me ~/go-with-me
cd ~/go-with-me && ./install.sh          # links `go-with-me` and `gwm` into ~/.local/bin
gwm init studio                          # say which host this machine is (creates ~/.config/go-with-me)
gwm edit                                 # list your hosts and sessions
gwm doctor                               # check every host is reachable
gwm                                      # open the hub
```

Each host needs: **Remote Login** on (System Settings → General → Sharing), this machine's SSH key in its `~/.ssh/authorized_keys`, and `tmux`.

## Config

`~/.config/go-with-me/hosts.conf`
```
# name   | ssh target          | role
studio   | you@my-mac-studio   | Home Mac Studio, 64GB. Always on. Local models, VMs.
macbook  | you@my-macbook-pro  | Work laptop; may be asleep.
sandbox  | vm:studio/sandbox   | Linux VM on studio
```

`~/.config/go-with-me/sessions.conf`
```
# name        | host    | engine   | account | folder
studio        | studio  | claude   | default | ~/code
studio-work   | studio  | claude   | work    | ~/work
studio-local  | studio  | opencode | -       | ~/code
macbook       | macbook | claude   | default | ~
```

## Commands

| Command | What it does |
|---|---|
| `gwm` | open the hub (or re-attach) |
| `gwm status` / `gwm board` | status table once / live |
| `gwm peek <session> [n]` | show a session's screen |
| `gwm send <session> "text"` | type into a session + Enter |
| `gwm restart <session>` | stop a session (restarts on reconnect) |
| `gwm run <host> "cmd"` | run a command on a host or VM |
| `gwm accounts [--on host]` | which Claude accounts are signed in |
| `gwm login <account> [--on host]` | sign an account in (stored in `~/.config/go-with-me/accounts/<account>`) |
| `gwm vm create <name> [--on host] [--cpus N --memory GB --disk GB]` | new Linux VM with tmux + Claude Code, added as a host and session |
| `gwm vm list\|start\|stop\|delete` | manage VMs |
| `gwm models [--on host]` / `gwm pull <model>` | local models (Ollama) |
| `gwm opencode-setup [--on host] [--model m]` | point opencode at that host's Ollama |
| `gwm sync <host>` | copy hosts/sessions config from another host |
| `gwm doctor` | check reachability and installed tools on every host |
| `gwm kill` | close the hub (sessions keep running) |

### In the hub
- **Click** a tab, or press **Alt+0…9** (Terminal.app: Settings → Profiles → Keyboard → *Use Option as Meta key*)
- **Ctrl+a d**: leave the hub; everything keeps running
- The hub's prefix is **Ctrl+a**, so **Ctrl+b** still reaches tmux inside a session

## How accounts work

Claude Code keeps its login in a config directory. `go-with-me` gives each account label its own directory (`CLAUDE_CONFIG_DIR=~/.config/go-with-me/accounts/<label>`) on the host where the session runs, so `default` (your normal login) and e.g. `work` stay separate, including history and settings.

## Security notes

- `gwm send` / `gwm run` execute on your machines over SSH. The coordinator is instructed to ask before sending to sessions or running anything that changes state, but treat it like any agent with shell access.
- Lima VMs mount your home folder **read-only** by default, which makes them handy sandboxes.
- Check your employer's policy before joining work machines or work accounts.

## License

MIT
