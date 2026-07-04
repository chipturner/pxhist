# pxh search demo: regex search, verbose output, failed-command filter.
# Run via demo/record.sh, which seeds the fixture database first.

Output out/search.gif

Set Shell "bash"
Set FontSize 18
Set Width 1500
Set Height 700
Set Padding 16
Set WindowBar Colorful
Set TypingSpeed 60ms

Env PXH_DB_PATH "/tmp/pxh-demo/pxh.db"

Sleep 500ms

Type "pxh s ffmpeg"
Sleep 500ms
Enter
Sleep 3s

Type "pxh s -v rsync"
Sleep 500ms
Enter
Sleep 3s

Type "pxh s --failed"
Sleep 500ms
Enter
Sleep 4s
