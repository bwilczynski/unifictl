# Every device, then zoom in on the core switch.
Output "docs/demo/devices.gif"
Set Width 1300
Set Height 940
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
