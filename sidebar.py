#!/usr/bin/env python3
"""go-with-me sidebar: the left column in every hub tab, and the hub's only navigation.

Layout, grouped by what belongs together:
  header
  coordinator                       (the overview tab)
  MACHINES — one card per machine:  status · CPU/RAM · models · VMs · who's using it,
                                    then that machine's sessions
  actions                           new · close · refresh · help
  details of the highlighted session
Click a line (or ↑/↓ + Enter) to jump; opening a stopped session starts it. Keys: n new · x close · r refresh · ? help.
State comes from `gwm _data` (one shared, cached SSH sweep); tabs are driven with `zellij action`.
"""
import curses, json, os, subprocess, sys, threading, time

GWM = sys.argv[1] if len(sys.argv) > 1 else "gwm"
ICON = {"claude": "✻", "opencode": "◇", "shell": "$"}
state = {"self": "", "hosts": [], "sessions": [], "probe": {}, "used": {}, "ok": False}
tabs = {"list": [], "active": None}
lock = threading.Lock()

# colour pairs
ACCENT, GREEN, YELLOW, RED, GREY, BLUE, MAGENTA = range(1, 8)


def run(*args, timeout=30):
    try:
        return subprocess.run(args, capture_output=True, text=True, timeout=timeout).stdout
    except Exception:
        return ""


# ---------------------------------------------------------------- data
def load_data():
    out = run(GWM, "_data", timeout=45)
    hosts, sessions, used, probe, me = [], [], {}, {}, ""
    for line in out.splitlines():
        if line.startswith("#SELF\t"):
            me = line.split("\t", 1)[1]
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
                p += [""] * 6
                d.update(up=True, cpu=p[2], ram=p[3], models=p[4], vms=p[5])
            elif kind == "S" and len(p) >= 6:
                d["sessions"][p[2]] = {"idle": int(p[3] or 0), "att": int(p[4] or 0), "title": p[5]}
    with lock:
        state.update(self=me, hosts=hosts, sessions=sessions, used=used, probe=probe, ok=bool(hosts))


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


def poller():
    n = 0
    while True:
        load_tabs()
        if n % 4 == 0:
            load_data()
        n += 1
        time.sleep(1.5)


def session_state(s):
    """→ (label, colour, current task)"""
    with lock:
        hp = state["probe"].get(s["host"], {})
    if not hp.get("up"):
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
def build():
    """A list of lines. Each line = (segments, target). segments: [(x, text, attr-key)] ; target: ('tab', name) / ('act', key) / None.
    x < 0 means right-aligned that many columns from the edge."""
    with lock:
        hosts, sessions, me = list(state["hosts"]), list(state["sessions"]), state["self"]
        probe, used, ok = dict(state["probe"]), dict(state["used"]), state["ok"]
        active = tabs["active"]
        num = {t["name"]: t["pos"] + 1 for t in tabs["list"]}
    L = []
    add = lambda segs=(), target=None: L.append((list(segs), target))

    add([(1, "go-with-me", "title"), (-1, "" if ok else "loading…", "dim")])
    add()
    add([(1, "▌" if active == "overview" else " ", "mark"), (3, "⌂  coordinator", "strong" if active == "overview" else "normal"),
         (-1, str(num.get("overview", "")), "dim")], ("tab", "overview"))
    add([(6, "ask · summarise · delegate", "dim")])
    add()
    add()
    add([(1, "MACHINES", "heading")])

    for h in hosts:
        hp = probe.get(h["name"], {})
        up = hp.get("up")
        add()
        add([(1, "●" if up else "○", "ok" if up else "bad"),
             (3, h["name"], "strong"), (4 + len(h["name"]), "· here" if h["name"] == me else "", "dim"),
             (-1, "" if up else "offline", "bad")])
        if up:
            add([(3, f"{hp.get('cpu', '')} cpu · {hp.get('ram', '').replace('G', ' GB')}", "dim")])
            m = short_models(hp.get("models"))
            if m:
                add([(3, "models ", "dim"), (10, m, "normal")])
            if hp.get("vms") and hp["vms"] != "-":
                add([(3, "vms    ", "dim"), (10, hp["vms"].replace(":Running", " ▶").replace(":Stopped", " ■"), "normal")])
            if used.get(h["name"]):
                add([(3, "in use ", "dim"), (10, used[h["name"]], "used")])
        mine = [s for s in sessions if s["host"] == h["name"]]
        if mine:
            add()
        for s in mine:
            label, col, _ = session_state(s)
            is_active = active == s["name"]
            off = label in ("off", "offline")
            n = num.get(s["name"])
            st = {"off": "○ off", "offline": "—"}.get(label, ("● " if label == "working" else "· ") + label)
            add([(1, "▌" if is_active else " ", "mark"),
                 (3, f"{ICON.get(s['engine'], '·')} {s['name']}", "strong" if is_active else ("dim" if off else "normal")),
                 (-4, st, col), (-1, str(n) if n and n < 10 else "", "dim")], ("tab", s["name"]))

    if not hosts and ok:
        add([(3, "no machines yet — gwm add", "dim")])
    add()
    add()
    add([(1, "+  new session", "normal"), (-1, "n", "dim")], ("act", "new"))
    add([(1, "×  close this session", "normal"), (-1, "x", "dim")], ("act", "close"))
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
         "ok": curses.color_pair(GREEN), "bad": curses.color_pair(RED), "used": curses.color_pair(MAGENTA)}.get(key)
    if a is None:
        a = curses.color_pair(key) if isinstance(key, int) else 0
    return a | (curses.A_REVERSE if sel else 0)


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
    tgt = L[cursor][1] if 0 <= cursor < len(L) else None
    if not tgt or tgt[0] != "tab":
        with lock:
            tgt = ("tab", tabs["active"]) if tabs["active"] else None
    det = details(tgt)
    room = H - footer_h - (len(det) + 2 if det else 0)
    for y, (segs, target) in enumerate(L[: max(room, 0)]):
        sel = (y == cursor and target is not None)
        if sel:
            put(y, 0, " " * W, curses.A_REVERSE)
        for x, text, key in segs:
            put(y, x, text, attr(key, sel))
    if det and room > 0:
        y = H - footer_h - len(det) - 1
        put(y - 1, 1, "─" * (W - 2), attr("dim"))
        for text, key in det:
            while text and y < H - footer_h:
                put(y, 1, text[: W - 2], attr(key)); text = text[W - 2:]; y += 1
    put(H - 1, 1, "Ctrl+g window keys · ? help", attr("dim"))
    scr.refresh()


# ---------------------------------------------------------------- actions
def zellij(*a):
    run("zellij", "action", *a, timeout=10)


def floating(cmd, name):
    subprocess.Popen(["zellij", "run", "--floating", "--close-on-exit", "--name", name, "--"] + cmd,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


HELP_CMD = r'''cat <<'EOF'
 go-with-me — keys & tips

 Sidebar        click a session to jump to it (opening a stopped one starts it)
                ↑/↓ or j/k then Enter · n new · x close current · r refresh
 Switch tabs    Alt+1…9 (numbers shown in the sidebar) · Alt+←/→
                Terminal.app: Settings → Profiles → Keyboard → "Use Option as Meta key"
 Typing         all keys go to the session you're in (Claude, opencode…)
 Copy text      select with the mouse — it's copied. Paste with Cmd+V
 Window keys    Ctrl+g, then  p ←/→  move between sidebar and session  ·  p f  fullscreen
                              s  scroll back  ·  Enter  back to typing
 Leave          Ctrl+g → o → d  (everything keeps running) — or just close the window
 From anywhere  gwm status · gwm who · gwm peek <session> · gwm send <session> "text"

 press any key to close
EOF
read -rsn1'''


def activate(target):
    if not target:
        return
    kind, key = target
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
            floating([GWM, "new"], "new session")
        elif key == "close":
            with lock:
                cur = tabs["active"]
            if cur and cur != "overview":
                floating(["bash", "-c", f'{GWM} close "{cur}"; sleep 1'], "close session")
        elif key == "refresh":
            threading.Thread(target=load_data, daemon=True).start()
        elif key == "help":
            floating(["bash", "-c", HELP_CMD], "keys & tips")


def main(scr):
    curses.curs_set(0)
    curses.use_default_colors()
    for pair, col in ((ACCENT, 209), (GREEN, 2), (YELLOW, 3), (RED, 1), (GREY, 8), (BLUE, 110), (MAGENTA, 176)):
        try:
            curses.init_pair(pair, col, -1)
        except curses.error:
            curses.init_pair(pair, curses.COLOR_WHITE, -1)
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
            if my in clickable and b & (curses.BUTTON1_CLICKED | curses.BUTTON1_RELEASED | curses.BUTTON1_PRESSED):
                cursor = my
                if b & (curses.BUTTON1_CLICKED | curses.BUTTON1_RELEASED):
                    activate(L[my][1]); cursor = -1
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
            activate(("act", "close"))
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
