#!/usr/bin/env python3
"""go-with-me sidebar: the left column in every hub tab.

Lists your machines and their sessions with live status. Click (or arrow keys + Enter) to jump to a session;
opening one that isn't running starts it. n = new session, x = close current session, r = refresh, ? = help.
Reads state from `gwm _data` (one shared SSH sweep, cached), and drives Zellij with `zellij action`.
"""
import curses, json, os, subprocess, sys, threading, time

GWM = sys.argv[1] if len(sys.argv) > 1 else "gwm"
ENGINE_ICON = {"claude": "✻", "opencode": "◇", "shell": "$"}
state = {"self": "", "hosts": [], "sessions": [], "probe": {}, "used": {}, "ok": False, "at": 0}
tabs = {"list": [], "active": None}
lock = threading.Lock()


def run(*args, timeout=30):
    try:
        return subprocess.run(args, capture_output=True, text=True, timeout=timeout).stdout
    except Exception:
        return ""


def load_data():
    out = run(GWM, "_data", timeout=45)
    hosts, sessions, used, probe, me = [], [], {}, {}, ""
    for line in out.splitlines():
        if line.startswith("#SELF\t"):
            me = line.split("\t", 1)[1]
        elif line.startswith("#HOST\t"):
            p = line.split("\t"); hosts.append({"name": p[1], "target": p[2], "role": p[3] if len(p) > 3 else ""})
        elif line.startswith("#SESS\t"):
            p = line.split("\t") + [""] * 6
            sessions.append({"name": p[1], "host": p[2], "engine": p[3], "account": p[4], "folder": p[5]})
        elif line.startswith("#USED\t"):
            p = line.split("\t") + [""]; used[p[1]] = p[2]
        elif "|" in line:
            p = line.split("|")
            h, kind = p[0], p[1] if len(p) > 1 else ""
            d = probe.setdefault(h, {"up": False, "sessions": {}})
            if kind == "H":
                d.update(up=True, cpu=p[2], ram=p[3], models=p[4] if len(p) > 4 else "-", vms=p[5] if len(p) > 5 else "")
            elif kind == "S" and len(p) >= 6:
                d["sessions"][p[2]] = {"idle": int(p[3] or 0), "att": int(p[4] or 0), "title": p[5]}
    with lock:
        state.update(self=me, hosts=hosts, sessions=sessions, used=used, probe=probe, ok=bool(hosts), at=time.time())


def load_tabs():
    try:
        data = json.loads(run("zellij", "action", "list-tabs", "-j", timeout=5) or "[]")
    except Exception:
        data = []
    lst = []
    for t in data:
        name = str(t.get("name", ""))
        base = name.split(" ", 1)[1] if name[:1].isdigit() and " " in name else name
        lst.append({"id": t.get("tab_id", t.get("id")), "pos": t.get("position", 0), "name": base, "active": t.get("active")})
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
    with lock:
        hp = state["probe"].get(s["host"], {})
    if not hp.get("up"):
        return "offline", 4, ""
    info = hp["sessions"].get(s["name"])
    if not info:
        return "off", 5, ""
    idle = info["idle"]
    label = "working" if idle < 8 else (f"{idle // 60}m" if idle < 3600 else f"{idle // 3600}h")
    return label, (3 if idle < 8 else 2), info["title"].lstrip("✳✻ ").strip()


def build_rows():
    """Each row: (kind, key, text-parts). kind in title/sep/overview/host/session/action/blank."""
    rows = [("title", None, None), ("sep", None, None), ("overview", "overview", None), ("blank", None, None)]
    with lock:
        hosts, sessions = list(state["hosts"]), list(state["sessions"])
    for h in hosts:
        rows.append(("host", h["name"], h))
        for s in sessions:
            if s["host"] == h["name"]:
                rows.append(("session", s["name"], s))
    if not hosts:
        rows.append(("blank", None, None))
    rows += [("blank", None, None), ("sep", None, None),
             ("action", "new", "+  new session        n"),
             ("action", "close", "×  close this session x"),
             ("action", "refresh", "⟳  refresh            r"),
             ("action", "help", "?  keys & tips        ?")]
    return rows


def draw(scr, rows, cursor):
    scr.erase()
    H, W = scr.getmaxyx()
    W = max(W, 10)
    with lock:
        active, me = tabs["active"], state["self"]
        probe, used, ok = state["probe"], state["used"], state["ok"]
    open_names = {t["name"] for t in tabs["list"]}

    def put(y, x, text, attr=0):
        if 0 <= y < H and x < W:
            try:
                scr.addnstr(y, x, text, max(0, W - x - 1), attr)
            except curses.error:
                pass

    for y, (kind, key, data) in enumerate(rows[:H]):
        sel = (y == cursor)
        base = curses.A_REVERSE if sel else 0
        if kind == "title":
            put(y, 1, "go-with-me", curses.color_pair(1) | curses.A_BOLD)
            put(y, 12, "" if ok else "loading…", curses.A_DIM)
        elif kind == "sep":
            put(y, 0, "─" * (W - 1), curses.A_DIM)
        elif kind == "overview":
            a = curses.A_BOLD | (curses.color_pair(6) if active == "overview" else 0)
            put(y, 1, ("▌" if active == "overview" else " ") + "⌂ overview", a | base)
            put(y, W - 7, "Alt+1", curses.A_DIM)
        elif kind == "host":
            hp = probe.get(key, {})
            up = hp.get("up")
            dot, col = ("●", 2) if up else ("○", 4)
            put(y, 1, dot, curses.color_pair(col) | base)
            put(y, 3, key + (" (here)" if key == me else ""), curses.A_BOLD | base)
            if up:
                put(y + 0, max(3 + len(key) + (8 if key == me else 1), W - 13), f"{hp.get('cpu','')} {hp.get('ram','')}".rjust(12), curses.A_DIM)
        elif kind == "session":
            label, col, title = session_state(data)
            is_active = (active == key)
            marker = "▌" if is_active else " "
            icon = ENGINE_ICON.get(data["engine"], "·")
            put(y, 1, marker, curses.color_pair(6) | curses.A_BOLD)
            put(y, 3, f"{icon} {key}", (curses.A_BOLD if is_active else 0) | (curses.A_DIM if label in ("off", "offline") and key not in open_names else 0) | base)
            st = {"off": "○ off", "offline": "offline"}.get(label, ("● " if label == "working" else "· ") + label)
            put(y, W - len(st) - 2, st, curses.color_pair(col))
        elif kind == "action":
            put(y, 1, data, curses.A_DIM | base if not sel else base)

    # footer: details for the highlighted / active session
    detail = None
    if 0 <= cursor < len(rows) and rows[cursor][0] in ("session", "host"):
        detail = rows[cursor]
    elif active:
        detail = next((r for r in rows if r[0] == "session" and r[1] == active), None)
    fy = len(rows) + 1
    if detail and fy < H - 1:
        kind, key, data = detail
        put(fy, 0, "─" * (W - 1), curses.A_DIM)
        if kind == "session":
            label, col, title = session_state(data)
            lines = [f"{key}", f"{data['engine']} on {data['host']}" + (f" · {data['account']}" if data['engine'] == 'claude' else ""),
                     f"in {data['folder']}"]
            if title:
                lines.append(title)
        else:
            hp = probe.get(key, {})
            lines = [key, (data.get("role") or "")[:200]]
            if hp.get("models") and hp["models"] != "-":
                lines.append("models: " + hp["models"])
            if hp.get("vms"):
                lines.append("VMs: " + hp["vms"])
            if used.get(key):
                lines.append("used by: " + used[key])
        y = fy + 1
        for i, ln in enumerate(lines):
            while ln and y < H - 1:
                put(y, 1, ln[: W - 3], (curses.A_BOLD if i == 0 else curses.A_DIM)); ln = ln[W - 3:]; y += 1
    scr.refresh()


def zellij(*a):
    run("zellij", "action", *a, timeout=10)


def floating(cmd, name):
    subprocess.Popen(["zellij", "run", "--floating", "--close-on-exit", "--name", name, "--"] + cmd,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def activate(row):
    kind, key, data = row
    if kind == "overview" or kind == "session":
        with lock:
            t = next((t for t in tabs["list"] if t["name"] == key), None)
        if t and t["id"] is not None:
            zellij("go-to-tab-by-id", str(t["id"]))
        elif kind == "session":
            subprocess.Popen([GWM, "open", key], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if kind == "session":
            time.sleep(0.2); zellij("move-focus", "right")
    elif kind == "action":
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


HELP_CMD = r'''cat <<'EOF'
 go-with-me — keys & tips

 Sidebar        click a session to jump to it (opening a stopped one starts it)
                ↑/↓ or j/k then Enter · n new · x close current · r refresh
 Switch tabs    Alt+1…9  ·  Alt+←/→  (Terminal.app: enable "Use Option as Meta key")
 Copy text      select with the mouse — it's copied automatically. Paste with Cmd+V.
 Zellij keys    Ctrl+g first, then follow the bar at the bottom (keys go to Claude until then)
 Leave          Ctrl+g → o → d   (everything keeps running) — or just close the window
 From anywhere  gwm status · gwm who · gwm peek <session> · gwm send <session> "text"

 press any key to close
EOF
read -rsn1'''


def main(scr):
    curses.curs_set(0)
    curses.use_default_colors()
    for i, c in enumerate([209, 2, 3, 1, 8, 110], start=1):  # 1 accent 2 green 3 yellow 4 red 5 grey 6 blue
        try:
            curses.init_pair(i, c, -1)
        except curses.error:
            curses.init_pair(i, curses.COLOR_WHITE, -1)
    curses.mousemask(curses.ALL_MOUSE_EVENTS | curses.REPORT_MOUSE_POSITION)
    curses.mouseinterval(0)
    scr.timeout(400)
    threading.Thread(target=poller, daemon=True).start()
    cursor = -1
    while True:
        rows = build_rows()
        selectable = [i for i, r in enumerate(rows) if r[0] in ("overview", "session", "host", "action")]
        draw(scr, rows, cursor)
        ch = scr.getch()
        if ch == -1:
            continue
        if ch == curses.KEY_MOUSE:
            try:
                _, mx, my, _, bstate = curses.getmouse()
            except curses.error:
                continue
            if bstate & (curses.BUTTON1_CLICKED | curses.BUTTON1_PRESSED | curses.BUTTON1_RELEASED):
                if 0 <= my < len(rows) and my in selectable:
                    cursor = my
                    if bstate & (curses.BUTTON1_CLICKED | curses.BUTTON1_RELEASED):
                        activate(rows[my])
            continue
        if ch in (curses.KEY_DOWN, ord("j")):
            nxt = [i for i in selectable if i > cursor]; cursor = nxt[0] if nxt else cursor
        elif ch in (curses.KEY_UP, ord("k")):
            prv = [i for i in selectable if i < cursor]; cursor = prv[-1] if prv else cursor
        elif ch in (10, 13, curses.KEY_ENTER) and 0 <= cursor < len(rows):
            activate(rows[cursor])
        elif ch == ord("n"):
            activate(("action", "new", None))
        elif ch == ord("x"):
            activate(("action", "close", None))
        elif ch == ord("r"):
            activate(("action", "refresh", None))
        elif ch == ord("?"):
            activate(("action", "help", None))


if __name__ == "__main__":
    os.environ.setdefault("ESCDELAY", "25")
    try:
        curses.wrapper(main)
    except KeyboardInterrupt:
        pass
