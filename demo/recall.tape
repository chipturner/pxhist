# pxh recall demo: interactive fuzzy search TUI.
# Run via demo/record.sh, which seeds the fixture database first.

Output out/recall.gif

Set Shell "bash"
Set FontSize 20
Set Width 1200
Set Height 700
Set Padding 16
Set WindowBar Colorful
Set TypingSpeed 80ms

Env PXH_DB_PATH "/tmp/pxh-demo/pxh.db"

Sleep 500ms

Type "pxh recall"
Sleep 500ms
Enter
Sleep 1.5s

# Narrow down to the ffmpeg gif command
Type "ffmpeg"
Sleep 1.5s
Type " gif"
Sleep 2s

# Select it
Enter
Sleep 3s
