Output docs/assets/demo.gif

Set Shell "bash"
Set FontFamily "Menlo"
Set FontSize 19
Set Width 1480
Set Height 640
Set Theme "Dracula"
Set Padding 28
Set TypingSpeed 35ms
Set Framerate 30
Set CursorBlink false

# clean, quiet prompt for the recording — not shown
Hide
Type "clear; export PS1='➜ '"
Enter
Sleep 500ms
Type "clear"
Enter
Show

Type "# which managers does this machine actually have"
Enter
Type "pum doctor"
Enter
Sleep 3s

Type "clear"
Enter
Type "# what's outdated, right now — real packages on this machine"
Enter
Type "pum report --outdated --manager brew | head -8"
Enter
Sleep 5s

Type "clear"
Enter
Type "# preview updating everything — dry-run touches nothing"
Enter
Type "pum update --dry-run --all"
Enter
Sleep 4s
