# Static image of the topology tree for the top of the README.
Output "docs/demo/topology.gif"
Set Width 900
Set Height 480
Source "docs/demo/common.tape"

Type "unifictl topology --include-wireless"
Enter
Sleep 500ms
Screenshot "docs/demo/topology.png"
# The screenshot is saved asynchronously; give it time before vhs exits.
Sleep 500ms
