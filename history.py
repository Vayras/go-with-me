#!/usr/bin/env python3
"""Print a readable version of a go-with-me session's Claude Code conversation.

Runs on the machine that owns the session (gwm sends it there). Finds the conversation by:
  1. the `claude` process running inside tmux session gwm-<name> (its folder + account), else
  2. the session's folder/account from the config, using the newest conversation saved there.
Usage: history.py <session> <lines> <fallback-folder> <fallback-config-dir>
"""
import glob, json, os, re, subprocess, sys, time

name, lines = sys.argv[1], int(sys.argv[2])
fb_folder = os.path.expanduser(sys.argv[3] if len(sys.argv) > 3 else "~")
fb_config = os.path.expanduser(sys.argv[4] if len(sys.argv) > 4 and sys.argv[4] else "~/.claude")
explicit = os.path.expanduser(sys.argv[5]) if len(sys.argv) > 5 and sys.argv[5] else ""   # a saved conversation file
locate = lines < 0                                                                          # -1: just say which conversation


def sh(*a):
    try:
        return subprocess.run(a, capture_output=True, text=True, timeout=10).stdout.strip()
    except Exception:
        return ""


def find_live():
    """(folder, config dir) of a claude process running in tmux session gwm-<name>, if any."""
    pane = sh("tmux", "display", "-p", "-t", f"=gwm-{name}:", "#{pane_pid}")
    if not pane:
        return None
    procs = {}
    for ln in sh("ps", "-axo", "pid=,ppid=,comm=").splitlines():
        p = ln.split(None, 2)
        if len(p) == 3:
            procs[p[0]] = (p[1], os.path.basename(p[2]))
    for pid, (ppid, comm) in procs.items():
        if comm != "claude":
            continue
        q = ppid
        while q in procs and q != pane:
            q = procs[q][0]
        if q == pane:
            cwd = sh("lsof", "-a", "-p", pid, "-d", "cwd", "-Fn")
            cwd = next((l[1:] for l in cwd.splitlines() if l.startswith("n")), "")
            env = sh("tmux", "show-environment", "-t", f"=gwm-{name}", "CLAUDE_CONFIG_DIR")
            cfg = env.split("=", 1)[1] if env.startswith("CLAUDE_CONFIG_DIR=") else os.path.expanduser("~/.claude")
            return cwd or None, cfg
    return None


live = find_live()
folder, config = (live if live else (fb_folder, fb_config))
if live and not folder:
    folder = sh("tmux", "display", "-p", "-t", f"=gwm-{name}:", "#{pane_current_path}") or fb_folder
slug = re.sub(r"[^A-Za-z0-9]", "-", os.path.realpath(folder))
files = sorted(glob.glob(os.path.join(config, "projects", slug, "*.jsonl")), key=os.path.getmtime, reverse=True)
if locate and not live:
    # claude isn't running in the session: only accept a conversation that was active while this session existed,
    # never just "the newest one in the folder" (that may belong to another session)
    created = sh("tmux", "display", "-p", "-t", f"=gwm-{name}:", "#{session_created}")
    files = [f for f in files if created.isdigit() and os.path.getmtime(f) >= int(created)][:1]
if explicit and os.path.isfile(explicit):
    files = [explicit]
if not files:
    if locate:
        print("{}"); sys.exit(0)
    print(f"[{name}] no Claude conversation found (folder {folder}, account dir {config})")
    sys.exit(0)

path = files[0]
title, msgs = "", []
if locate:   # describe the conversation (for resuming later) without printing it
    for raw in open(path, errors="replace"):
        if '"ai-title"' in raw:
            try: title = json.loads(raw).get("aiTitle") or title
            except ValueError: pass
    print(json.dumps({"transcript": path, "session_id": os.path.basename(path)[:-6], "folder": folder,
                      "config": config, "title": title, "live": bool(live)}))
    sys.exit(0)


def clip(t, n=700):
    t = re.sub(r"\s+\n", "\n", t.strip())
    return t if len(t) <= n else t[:n] + " …"


for raw in open(path, errors="replace"):
    try:
        d = json.loads(raw)
    except ValueError:
        continue
    kind = d.get("type")
    if kind == "ai-title":
        title = d.get("aiTitle") or d.get("title") or title
        continue
    if kind not in ("user", "assistant"):
        continue
    c = (d.get("message") or {}).get("content")
    parts = []
    if isinstance(c, str):
        parts.append(c)
    elif isinstance(c, list):
        for b in c:
            t = b.get("type")
            if t == "text":
                parts.append(b.get("text", ""))
            elif t == "tool_use":
                inp = b.get("input") or {}
                brief = inp.get("command") or inp.get("file_path") or inp.get("description") or inp.get("pattern") or ""
                parts.append(f"→ {b.get('name')}: {str(brief)[:160]}")
    text = "\n".join(p for p in parts if p and p.strip())
    if not text or text.startswith("<"):          # skip system/hook wrappers
        continue
    msgs.append(("you" if kind == "user" else "claude", clip(text)))

age = int(time.time() - os.path.getmtime(path))
when = f"{age // 60}m ago" if age < 3600 else (f"{age // 3600}h ago" if age < 86400 else f"{age // 86400}d ago")
print(f"[{name}] {title or 'untitled'} — {len(msgs)} messages, last activity {when}{' (claude running)' if live else ''}")
print(f"         {folder} · {config}")
for who, text in msgs[-lines:]:
    print(f"\n{who}:")
    for ln in text.splitlines():
        print("  " + ln)
