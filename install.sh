#!/bin/bash
# Links go-with-me (and the short name gwm) into ~/.local/bin
set -e
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$HOME/.local/bin"
chmod +x "$here/go-with-me"
ln -sf "$here/go-with-me" "$HOME/.local/bin/go-with-me"
ln -sf "$here/go-with-me" "$HOME/.local/bin/gwm"
command -v tmux >/dev/null || echo "Note: tmux is not installed — brew install tmux"
case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) echo "Add to your shell profile:  export PATH=\"\$HOME/.local/bin:\$PATH\"";; esac
echo "Installed. Next: gwm init <this-host-name>"
