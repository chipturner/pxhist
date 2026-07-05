# pxh Ctrl-R demo: shell integration end to end. A command typed live is
# recorded by the hooks, found via Ctrl-R, and re-run with Enter; a second
# Ctrl-R retrieves an old command onto the line for editing (Ctrl-E).
# Run via demo/record.sh, which seeds the fixture database and demo HOME.

Output out/ctrl-r.gif

Set Shell "zsh"
Set FontSize 20
Set Width 1200
Set Height 700
Set Padding 16
Set WindowBar Colorful
Set TypingSpeed 80ms

Env HOME "/tmp/pxh-demo/home"
Env PXH_DB_PATH "/tmp/pxh-demo/pxh.db"

# vhs pins ZDOTDIR to its own config, so load the pxh integration
# off-camera instead of relying on zsh sourcing our .zshrc.
Hide
Type "source /tmp/pxh-demo/home/.zshrc && cd ~ && clear"
Enter
Sleep 1s
Show

Sleep 1s

# A live command -- the pxh hooks record it as it runs
Type "echo hello from pxh"
Sleep 500ms
Enter
Sleep 1.5s

# Ctrl-R finds it instantly and Enter re-runs it
Ctrl+R
Wait+Screen /Enter Run/
Sleep 1s
Type "hello"
Sleep 1.5s
Enter
Sleep 2s

# Ctrl-R again: pull an old ffmpeg command onto the line to edit
Ctrl+R
Wait+Screen /Enter Run/
Sleep 1s
Type "timelapse"
Sleep 2s
Ctrl+E
Sleep 3.5s
