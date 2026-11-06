# Duck Quill test sheets

Real sprite sheets exported by Duck Quill (`quill`, its `phase-5` branch),
used by the `SpriteSheet` tests. `duck-script.json` is the Duck Quill
script they came from; to make them again:

```sh
quill export duck-script.json --sheet --out scale1            # then rename the files to duck.*
quill export duck-script.json --sheet --scale 2 --out scale2
```

(`quill export` names its files after the script, so `duck-script.png` and
`duck-script.json` are renamed to `duck.png` and `duck.json`, and the
JSON's `"image"` says `duck.png`.)
