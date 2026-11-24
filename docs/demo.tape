# Records docs/demo.gif and screenshots from `gwm demo` (run: vhs docs/demo.tape from the repo root)
Output docs/demo.gif
Require zellij
Set Shell "bash"
Set FontSize 15
Set Width 1800
Set Height 1000
Set Padding 12
Set Theme "Catppuccin Mocha"
Set TypingSpeed 60ms

Hide
Type "export PATH=$PWD:/opt/homebrew/bin:$PATH PS1='$ ' && clear" Enter
Show
Sleep 500ms
Type "./go-with-me demo" Sleep 400ms Enter
Sleep 20s
Screenshot docs/hub-overview.png
Sleep 2s
Ctrl+g
Sleep 300ms
Type "t"
Sleep 300ms
Right
Sleep 300ms
Enter
Sleep 7s
Screenshot docs/session-tab.png
Ctrl+g
Sleep 300ms
Type "t"
Sleep 300ms
Right
Sleep 300ms
Enter
Sleep 7s
Ctrl+g
Sleep 300ms
Type "t"
Sleep 300ms
Right
Sleep 300ms
Enter
Sleep 5s
Ctrl+g
Sleep 300ms
Type "t"
Sleep 300ms
Left 3
Sleep 300ms
Enter
Sleep 4s
Hide
Ctrl+g
Type "o"
Type "d"
Sleep 1s
Type "./go-with-me demo status" Enter
Sleep 3s
Show
Screenshot docs/status.png
Sleep 3s
