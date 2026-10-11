# Every device, zoom in on the core switch, then the whole tree.
Output "docs/demo/devices.gif"
Set Width 1720
Set Height 1600
Source "docs/demo/common.tape"

Type "unifictl devices list"
Sleep 300ms
Enter
Sleep 3s

Ctrl+L
Type "unifictl devices get switch-core --all-ports"
Sleep 300ms
Enter
Sleep 8s

Ctrl+L
Type "unifictl topology --include-wireless"
Sleep 300ms
Enter
Sleep 5s
