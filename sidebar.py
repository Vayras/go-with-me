#!/usr/bin/env python3
"""go-with-me sidebar: the left column in every hub tab, and the hub's only navigation.

Layout, grouped by what belongs together:
  header
  coordinator                       (the overview tab)
  THIS MAC — CPU/GPU/RAM, every loaded model, VMs, who's using it, its sessions;  OTHER MACHINES — CPU/GPU/RAM when online,
                                    then that machine's sessions
  actions                           new · close · refresh · help
  details of the highlighted session
Click a line (or ↑/↓ + Enter) to jump; opening a stopped session starts it. Keys: n new · x close · r refresh · ? help.
State comes from `gwm _data` (one shared, cached SSH sweep); tabs are driven with `zellij action`.
"""
import curses, json, os, subprocess, sys, threading, time

GWM = sys.argv[1] if len(sys.argv) > 1 else "gwm"
ICON = {"claude": "✻", "opencode": "◇", "shell": "$", "kimi": "◆", "codex": "◎", "gemini": "✦"}
state = {"self": "", "hosts": [], "sessions": [], "probe": {}, "used": {}, "ok": False}
tabs = {"list": [], "active": None}
lock = threading.Lock()
offset = 0          # sidebar scroll position

# colour pairs
ACCENT, GREEN, YELLOW, RED, GREY, BLUE, MAGENTA, SEL = range(1, 9)


def run(*args, timeout=30):
    try:
        return subprocess.run(args, capture_output=True, text=True, timeout=timeout).stdout
    except Exception:
        return ""


# ---------------------------------------------------------------- data
def load_data():
    out = run(GWM, "_data", timeout=45)
    hosts, sessions, used, probe, me, coord = [], [], {}, {}, "", ""
    for line in out.splitlines():
        if line.startswith("#SELF\t"):
            me = line.split("\t", 1)[1]
        elif line.startswith("#COORD\t"):
            coord = line.split("\t", 1)[1]
        elif line.startswith("#HOST\t"):
            p = line.split("\t") + [""] * 4
            hosts.append({"name": p[1], "target": p[2], "role": p[3]})
        elif line.startswith("#SESS\t"):
            p = line.split("\t") + [""] * 6
            sessions.append({"name": p[1], "host": p[2], "engine": p[3], "account": p[4], "folder": p[5]})
        elif line.startswith("#USED\t"):
            p = line.split("\t") + [""]
            used[p[1]] = p[2]
        elif "|" in line:
            p = line.split("|")
            h, kind = p[0], (p[1] if len(p) > 1 else "")
            d = probe.setdefault(h, {"up": False, "sessions": {}})
            if kind == "H":
                p += [""] * 7
                d.update(up=True, cpu=p[2], ram=p[3], models=p[4], vms=p[5], gpu=p[6], mlist=d.get("mlist", []))
            elif kind == "A":                      # up, but Tailscale SSH wants the login re-approved
                d["approval"] = p[2] if len(p) > 2 else ""
            elif kind == "M" and len(p) >= 4:
                d.setdefault("mlist", []).append((p[2].replace(":latest", ""), p[3], p[4] if len(p) > 4 else ""))

            elif kind == "S" and len(p) >= 6:
                d["sessions"][p[2]] = {"idle": int(p[3] or 0), "att": int(p[4] or 0), "title": p[5]}
    with lock:
        state.update(self=me, coord=coord, hosts=hosts, sessions=sessions, used=used, probe=probe, ok=bool(hosts))


def load_tabs():
    try:
        data = json.loads(run("zellij", "action", "list-tabs", "-j", timeout=5) or "[]")
    except Exception:
        data = []
    lst = []
    for t in data:
        name = str(t.get("name", ""))
        base = name.split(" ", 1)[1] if name[:1].isdigit() and " " in name else name
        tid, pos = t.get("tab_id", t.get("id")), int(t.get("position", 0))
        lst.append({"id": tid, "pos": pos, "name": base, "active": t.get("active")})
        want = f"{pos + 1} {base}"              # keep "N name" in step with position, so Alt+N matches
        if name != want and tid is not None:
            run("zellij", "action", "rename-tab-by-id", str(tid), want, timeout=5)
    with lock:
        tabs["list"] = lst
        tabs["active"] = next((t["name"] for t in lst if t["active"]), None)


CFG = os.environ.get("GWM_CONFIG") or os.path.expanduser("~/.config/go-with-me")
HUB = os.environ.get("ZELLIJ_SESSION_NAME", "")
seen_running = None   # sessions that were running at the previous check (None = first look)


def is_leader():
    """Only one sidebar per hub keeps the tabs in step (every tab has its own sidebar)."""
    if not HUB:
        return False
    lock = os.path.join(CFG, f".sidebar-leader-{HUB}")
    try:
        other = int(open(lock).read().strip() or 0)
    except (OSError, ValueError):
        other = 0
    if other and other != os.getpid():
        try:
            os.kill(other, 0); return False            # another sidebar is in charge
        except OSError:
            pass                                        # it's gone: take over
    try:
        open(lock, "w").write(str(os.getpid()))
    except OSError:
        return False
    return True


def sync_tabs():
    """Close tabs of sessions that were ended anywhere; open tabs for saved sessions that started anywhere."""
    global seen_running
    with lock:
        if not state["ok"]:
            return
        saved = {s["name"] for s in state["sessions"]}
        running = {n for h in state["probe"].values() for n in h.get("sessions", {})} - {"coordinator"}
        up = {h for h, d in state["probe"].items() if d.get("up")}
        host_of = {s["name"]: s["host"] for s in state["sessions"]}
        tablist = list(tabs["list"])
    open_tabs = {t["name"] for t in tablist}
    for t in tablist:
        n = t["name"]
        if n == "overview" or t["id"] is None:
            continue
        gone = n not in saved and n not in running
        if gone:
            run("zellij", "action", "close-tab-by-id", str(t["id"]), timeout=5)
    now_running = {n for n in running if n in saved and host_of.get(n) in up}
    if seen_running is not None:
        for n in sorted(now_running - seen_running - open_tabs):   # newly started somewhere: give it a tab
            subprocess.Popen([GWM, "open", n], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    seen_running = now_running


def poller():
    n = 0
    while True:
        load_tabs()
        if n % 4 == 0:
            load_data()
            if is_leader():
                try:
                    sync_tabs()
                except Exception:
                    pass
        n += 1
        time.sleep(1.5)


def session_state(s):
    """→ (label, colour, current task)"""
    with lock:
        hp = state["probe"].get(s["host"], {})
    if not hp.get("up"):
        if "approval" in hp:
            return "approval", YELLOW, "Tailscale SSH needs re-approval"
        return "offline", RED, ""
    info = hp["sessions"].get(s["name"])
    if not info:
        return "off", GREY, ""
    idle = info["idle"]
    if idle < 8:
        return "working", YELLOW, info["title"].lstrip("✳✻ ").strip()
    label = f"{idle // 60}m" if idle < 3600 else f"{idle // 3600}h"
    return label, GREEN, info["title"].lstrip("✳✻ ").strip()


def short_models(m):
    names = [x.split(":")[0] for x in (m or "").split(",") if x and x != "-"]
    if not names:
        return ""
    return names[0] + (f" +{len(names) - 1}" if len(names) > 1 else "")


# ---------------------------------------------------------------- layout
def model_rows(mlist):
    """[(label, size)] biggest first; same-named models get their format (e.g. 'gguf') so they can be told apart."""
    names = [m[0] for m in mlist]
    out = []
    for name, sz, fmt in sorted(mlist, key=lambda m: -float(m[1])):
        fmt = {"safetensors": "mlx"}.get(fmt, fmt)
        out.append((name + (f" ·{fmt}" if names.count(name) > 1 and fmt else ""), float(sz)))
    return out


def pct(v):
    try:
        return max(0.0, min(100.0, float(str(v).rstrip("%"))))
    except ValueError:
        return None


def usage_rows(add, hp):
    """CPU / GPU / RAM bars for an online machine (GPU skipped if the machine doesn't report it)."""
    for label, val, shown in (("CPU", pct(hp.get("cpu")), hp.get("cpu", "")),
                              ("GPU", pct(hp.get("gpu")), hp.get("gpu", "")),
                              ("RAM", ram_pct(hp.get("ram", "")), hp.get("ram", "").replace("G", " GB"))):
        if val is None and label == "GPU":
            continue
        add([(3, label, "dim"), (8, bar(val), level(val)), (19, shown, "normal")])


def ram_pct(ram):
    try:
        used, tot = ram.rstrip("G").split("/")
        return 100.0 * float(used) / float(tot)
    except Exception:
        return None


def bar(p, width=10):
    if p is None:
        return ""
    full = int(round(p / 100 * width))
    return "▮" * full + "▯" * (width - full)


def level(p):
    return "dim" if p is None else (RED if p >= 85 else (YELLOW if p >= 60 else GREEN))


def session_lines(add, sessions, host, active, num):
    hidden = 0
    for s in sessions:
        if s["host"] != host:
            continue
        label, col, _ = session_state(s)
        if label in ("off", "offline", "approval") and s["name"] not in num:
            hidden += 1                      # saved but not running: don't clutter the list
            continue
        is_active = active == s["name"]
        off = label in ("off", "offline", "approval")
        n = num.get(s["name"])
        st = {"off": "○ off", "offline": "—", "approval": "◐ approve"}.get(label, ("● " if label == "working" else "· ") + label)
        add([(1, "▌" if is_active else " ", "mark"),
             (3, f"{ICON.get(s['engine'], '·')} {s['name']}", "strong" if is_active else ("dim" if off else "normal")),
             (-4, st, col), (-1, str(n) if n and n < 10 else "", "dim")], ("tab", s["name"]))
    with lock:
        running = state["probe"].get(host, {}).get("sessions", {})
    saved = {s["name"] for s in sessions}
    for name in sorted(running):
        if name in saved or name == "coordinator":
            continue                         # running, but no saved entry: show it so it can be ended
        add([(1, " ", "mark"), (3, f"· {name}", "dim"), (-4, "not saved", YELLOW)], ("orphan", name))
    if hidden:
        add([(3, f"{hidden} saved · not running", "dim")], ("act", "saved:" + host))


def build():
    """A list of lines. Each line = (segments, target). segments: [(x, text, style)] ; target: ('tab', name) / ('act', key) / None.
    x < 0 means right-aligned that many columns from the edge."""
    with lock:
        hosts, sessions, me = list(state["hosts"]), list(state["sessions"]), state["self"]
        probe, used, ok = dict(state["probe"]), dict(state["used"]), state["ok"]
        active = tabs["active"]
        num = {t["name"]: t["pos"] + 1 for t in tabs["list"]}
        coord = state.get("coord", "")
    L = []
    add = lambda segs=(), target=None: L.append((list(segs), target))

    add([(1, "go-with-me", "title"), (-1, "" if ok else "loading…", "dim")])
    add()
    add([(1, "▌" if active == "overview" else " ", "mark"), (3, "⌂  coordinator", "strong" if active == "overview" else "normal"),
         (-1, str(num.get("overview", "")), "dim")], ("tab", "overview"))
    add([(6, "shared · lives on " + (coord or "?"), "dim")])

    # ---- THIS MAC: everything about the machine you're sitting at
    here = next((h for h in hosts if h["name"] == me), None)
    if here:
        hp = probe.get(me, {})
        add(); add()
        add([(1, "THIS MAC", "heading"), (10, "· " + me, "dim")])
        add()
        if hp.get("up"):
            usage_rows(add, hp)
            mlist = hp.get("mlist") or []
            add()
            if mlist:
                total = sum(float(m[1]) for m in mlist)
                add([(3, f"models loaded · {len(mlist)}", "dim"), (-1, f"{total:.1f} GB", "dim")])
                for name, sz in model_rows(mlist):
                    add([(5, name[:22], "normal"), (-1, f"{sz:.1f} GB", "dim")])
            else:
                add([(3, "no models loaded", "dim")])
            if hp.get("vms") and hp["vms"] != "-":
                add()
                add([(3, "vms", "dim"), (8, hp["vms"].replace(":Running", " ▶").replace(":Stopped", " ■"), "normal")])
            if used.get(me):
                add()
                add([(3, "in use by", "dim"), (13, used[me], "used")])
        else:
            add([(3, "status unavailable", "bad")])
        if any(s["host"] == me for s in sessions):
            add()
            add([(3, "sessions", "dim")])
            before = len(L)
            session_lines(add, sessions, me, active, num)
            if len(L) == before:
                L.pop(); L.pop()

    # ---- OTHER MACHINES: compact cards
    others = [h for h in hosts if h["name"] != me]
    if others:
        add(); add()
        add([(1, "OTHER MACHINES", "heading")])
        for h in others:
            hp = probe.get(h["name"], {})
            up = hp.get("up")
            ask = not up and "approval" in hp
            add()
            add([(1, "●" if up else "◐" if ask else "○", "ok" if up else "warn" if ask else "bad"), (3, h["name"], "strong"),
                 (-1, "" if up else "approval needed" if ask else "offline", "warn" if ask else "bad")])
            if ask:                                 # Tailscale SSH "check": up, but the login waits for a browser approval
                add([(3, "ssh there once, or open the", "dim")])
                add([(3, "link in: gwm status", "dim")])
            if up:                                  # usage only for machines that are switched on
                usage_rows(add, hp)
                for name, sz in model_rows(hp.get("mlist") or []):
                    add([(3, "◆ " + name[:20], "normal"), (-1, f"{sz:.1f} GB", "dim")])
                if hp.get("vms") and hp["vms"] != "-":
                    add([(3, "vms " + hp["vms"].replace(":Running", " ▶").replace(":Stopped", " ■"), "normal")])
                if used.get(h["name"]):
                    add([(3, "in use by " + used[h["name"]], "used")])
            session_lines(add, sessions, h["name"], active, num)

    if not hosts and ok:
        add([(3, "no machines yet — gwm add", "dim")])
    add(); add()
    add([(1, "+  new session", "normal"), (-1, "n", "dim")], ("act", "new"))
    add([(1, "×  end a session", "normal"), (-1, "x", "dim")], ("act", "close"))
    add([(1, "⟳  refresh", "normal"), (-1, "r", "dim")], ("act", "refresh"))
    return L


def details(target):
    """Lines describing the highlighted (or current) session/machine, shown near the bottom."""
    if not target or target[0] != "tab" or target[1] == "overview":
        return []
    with lock:
        s = next((x for x in state["sessions"] if x["name"] == target[1]), None)
    if not s:
        return []
    label, col, task = session_state(s)
    out = [(s["name"], "strong"),
           (f"{s['engine']} on {s['host']}" + (f" · {s['account']}" if s["engine"] == "claude" else ""), "dim"),
           (f"in {s['folder']}", "dim")]
    if task:
        out.append((task, "normal"))
    elif label == "off":
        out.append(("not running — click to start", "dim"))
    return out


# ---------------------------------------------------------------- drawing
def attr(key, sel=False):
    a = {"title": curses.color_pair(ACCENT) | curses.A_BOLD, "heading": curses.color_pair(GREY) | curses.A_BOLD,
         "strong": curses.A_BOLD, "normal": 0, "dim": curses.color_pair(GREY), "mark": curses.color_pair(BLUE) | curses.A_BOLD,
         "ok": curses.color_pair(GREEN), "bad": curses.color_pair(RED), "warn": curses.color_pair(YELLOW), "used": curses.color_pair(MAGENTA)}.get(key)
    if a is None:
        a = curses.color_pair(key) if isinstance(key, int) else 0
    return (curses.color_pair(SEL) | curses.A_BOLD) if sel else a


def draw(scr, L, cursor):
    scr.erase()
    H, W0 = scr.getmaxyx()
    W = max(W0 - 1, 12)                      # Zellij draws the divider

    def put(y, x, text, a=0):
        if not text or not (0 <= y < H):
            return
        if x < 0:
            x = W - len(text) - (-x) + 1
        x = max(0, x)
        if x >= W:
            return
        try:
            scr.addnstr(y, x, text, max(0, W - x), a)
        except curses.error:
            pass

    footer_h = 2
    global offset
    tgt = L[cursor][1] if 0 <= cursor < len(L) else None
    if not tgt or tgt[0] != "tab":
        with lock:
            tgt = ("tab", tabs["active"]) if tabs["active"] else None
    det = details(tgt)
    room = H - footer_h - (len(det) + 2 if det else 0)
    if room < len(L) and det:                # not enough space: drop the details panel first
        det, room = [], H - footer_h
    room = max(room, 1)
    offset = max(0, min(offset, len(L) - room))
    if 0 <= cursor < len(L):                 # keep the highlighted row on screen
        offset = min(max(offset, cursor - room + 1), cursor)
    for y, (segs, target) in enumerate(L[offset: offset + room]):
        i = y + offset
        sel = (i == cursor and target is not None)
        if sel:
            put(y, 0, " " * W, curses.color_pair(SEL))
        for x, text, key in segs:
            put(y, x, text, attr(key, sel))
    if offset > 0:
        put(0, W - 3, "▲", attr("dim"))
    if offset + room < len(L):
        put(room - 1, W - 3, "▼", attr("dim"))
    if det and room > 0:
        y = H - footer_h - len(det) - 1
        put(y - 1, 1, "─" * (W - 2), attr("dim"))
        for text, key in det:
            while text and y < H - footer_h:
                put(y, 1, text[: W - 2], attr(key)); text = text[W - 2:]; y += 1
    put(H - 1, 1, "⌥S sidebar · ⌥/ shortcuts", attr("dim"))
    scr.refresh()


# ---------------------------------------------------------------- actions
def zellij(*a):
    run("zellij", "action", *a, timeout=10)


def floating(cmd, name):
    subprocess.Popen(["zellij", "run", "--floating", "--close-on-exit", "--name", name, "--"] + cmd,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


HELP_CMD = f'{GWM} keys'



def activate(target):
    if not target:
        return
    kind, key = target
    if kind == "orphan":
        floating(["bash", "-c", 'echo "\"$1\" is still running, but it is not in your saved sessions, so it can'"'"'t be opened as a tab."; echo; "$0" close "$1"; echo; read -rsn1 -p "Press any key to close this window"', GWM, key], "end session")
        return
    if kind == "tab":
        with lock:
            t = next((t for t in tabs["list"] if t["name"] == key), None)
        if t and t["id"] is not None:
            zellij("go-to-tab-by-id", str(t["id"]))
            if key != "overview":
                time.sleep(0.15); zellij("move-focus", "right")
        elif key != "overview":
            subprocess.Popen([GWM, "open", key], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    elif kind == "act":
        if key == "new":
            floating(["bash", "-c", f'{GWM} new || {{ echo; read -rp "Press Enter to close "; }}'], "new session")
        elif key == "close" or key.startswith("close:"):
            with lock:
                cur = key.split(":", 1)[1] if ":" in key else tabs["active"]
            if cur and cur != "overview":
                floating(["bash", "-c", '"$0" close "$1"; echo; read -rsn1 -p "Press any key to close this window"', GWM, cur], "end session")
        elif key == "refresh":
            threading.Thread(target=load_data, daemon=True).start()
        elif key.startswith("saved:"):
            host = key.split(":", 1)[1]
            floating(["bash", "-c", '''echo "Saved sessions on $1 (not running):"; echo
"$0" sessions | awk -v h="$1" 'NR==1 || $2==h'; echo
read -rp "Start which one? (name, or Enter to cancel) " n; [ -n "$n" ] && "$0" open "$n"''', GWM, host], "saved sessions")
        elif key == "help":
            floating(["bash", "-c", HELP_CMD], "shortcuts")


def main(scr):
    curses.curs_set(0)
    curses.use_default_colors()
    for pair, col in ((ACCENT, 209), (GREEN, 2), (YELLOW, 3), (RED, 1), (GREY, 8), (BLUE, 110), (MAGENTA, 176)):
        try:
            curses.init_pair(pair, col, -1)
        except curses.error:
            curses.init_pair(pair, curses.COLOR_WHITE, -1)
    try:
        curses.init_pair(SEL, -1, 237)          # selected row: a soft grey band, not inverse video
    except curses.error:
        curses.init_pair(SEL, curses.COLOR_WHITE, curses.COLOR_BLACK)
    curses.mousemask(curses.ALL_MOUSE_EVENTS | curses.REPORT_MOUSE_POSITION)
    curses.mouseinterval(0)
    scr.timeout(400)
    threading.Thread(target=poller, daemon=True).start()
    cursor = -1
    while True:
        L = build()
        clickable = [i for i, (_, t) in enumerate(L) if t]
        draw(scr, L, cursor)
        ch = scr.getch()
        if ch == -1:
            continue
        if ch == curses.KEY_MOUSE:
            try:
                _, mx, my, _, b = curses.getmouse()
            except curses.error:
                continue
            global offset
            if b & getattr(curses, "BUTTON4_PRESSED", 0):
                offset = max(0, offset - 3); continue
            if b & getattr(curses, "BUTTON5_PRESSED", 0):
                offset += 3; continue
            row = my + offset
            if row in clickable and b & (curses.BUTTON1_CLICKED | curses.BUTTON1_RELEASED | curses.BUTTON1_PRESSED):
                cursor = row
                if b & (curses.BUTTON1_CLICKED | curses.BUTTON1_RELEASED):
                    activate(L[row][1]); cursor = -1
            continue
        if ch in (curses.KEY_DOWN, ord("j")):
            nxt = [i for i in clickable if i > cursor]; cursor = nxt[0] if nxt else cursor
        elif ch in (curses.KEY_UP, ord("k")):
            prv = [i for i in clickable if i < cursor]; cursor = prv[-1] if prv else cursor
        elif ch in (10, 13, curses.KEY_ENTER) and 0 <= cursor < len(L):
            activate(L[cursor][1]); cursor = -1   # the ▌ marker shows where you are; drop the selection
        elif ch == ord("n"):
            activate(("act", "new"))
        elif ch == ord("x"):
            row = L[cursor][1] if 0 <= cursor < len(L) else None
            if row and row[0] in ("tab", "orphan") and row[1] != "overview":
                activate(("act", "close:" + row[1])); cursor = -1     # end the highlighted session
            else:
                activate(("act", "close"))                            # or the tab you're in
        elif ch == ord("r"):
            activate(("act", "refresh"))
        elif ch == ord("?"):
            activate(("act", "help"))


if __name__ == "__main__":
    os.environ.setdefault("ESCDELAY", "25")
    try:
        curses.wrapper(main)
    except KeyboardInterrupt:
        pass
