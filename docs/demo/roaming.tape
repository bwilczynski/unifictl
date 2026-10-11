# Diagnosing a client that keeps roaming between two access points.
Output "docs/demo/roaming.gif"
Set Width 2040
Set Height 760
Source "docs/demo/common.tape"

Type "unifictl clients list"
Sleep 300ms
Enter
Sleep 3s

Ctrl+L
Type "unifictl clients get roborock-0c04"
Sleep 300ms
Enter
Sleep 3s

Ctrl+L
Type "unifictl logs --client roborock-0c04"
Sleep 300ms
Enter
Sleep 8s
