# Duck Forge brand

The app icon for Duck Forge Studio: the duck at the anvil, forging a
glowing pixel. Made for this project, in the style of the Duck Quill icon
(an indigo tile with stars, and the yellow duck).

| File | Use |
|---|---|
| `duck-forge.ico` | Windows icon (16 to 256 px; the close-up head for 48 px and below) |
| `duck-forge-icon-1024.png`, `-512`, `-256`, `-128`, `-64` | App icon, full scene |
| `duck-forge-icon-48.png`, `-32`, `-24`, `-16` | App icon, close-up head for small sizes |
| `duck-forge-icon.svg`, `duck-forge-icon-small.svg` | Sources of the two icons |

Colours, shared with Duck Quill: duck yellow `#FFCC2E`, bill orange
`#FF8C22`, paint cyan `#46D6FF`, quill lavender `#DCD5FF`, night indigo
`#5546D0`, ink navy `#1B1446`.

To make the PNGs again after changing an SVG: open it in a browser at each
size and save the picture (any SVG renderer does), then join the PNGs into
the `.ico`: `convert duck-forge-icon-{16,24,32,48,64,128,256}.png duck-forge.ico`.
