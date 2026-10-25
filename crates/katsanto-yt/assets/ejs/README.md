# yt-dlp ejs solver (vendored)

Signature / `n` parameter solver from <https://github.com/yt-dlp/ejs>, release **0.8.0**.

- `yt.solver.lib.js` — bundled meriyah (ISC) + astring (MIT), see file header
- `yt.solver.core.js` — the solver (Unlicense)

SHA3-512 hashes match the ones vendored by yt-dlp
(`yt_dlp/extractor/youtube/jsc/_builtin/vendor/_info.py`):

```
lib   1ee3753a8222fc855f5c39db30a9ccbb7967dbe1fb810e86dc9a89aa073a0907f294c720e9b65427d560a35aa1ce6af19ef854d9126a05ca00afe03f72047733
core  c163a6f376db6ce3da47d516a28a8f2a0554ae95c58dc766f0a6e2b3894f2cef1ee07fa84beb442fa471aac4f300985added1657c7c94c4d1cfefe68920ab599
```

To update: download both files from the ejs GitHub release, compare hashes with
yt-dlp's `_info.py`, replace them here. At runtime, files with the same names in
`<data_dir>/ejs/` override these, so a solver update does not require a rebuild.
