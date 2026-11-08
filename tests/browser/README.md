# Browser regressions

These tests use the real editor and an isolated dufs process bound to
`127.0.0.1`. They cover native read-only behavior, failed saves that preserve
the draft and selection, and successful saves that reload the document.
The HTTP 403 and 500 cases use actual server responses rather than mocked
requests. Run as a non-root user on Linux so the read-only file fixture can
produce a write failure.

From the repository root, with Node.js 22.12 or newer:

```sh
cargo build --bin dufs
npm ci --prefix tests/browser
npm run check:es2017 --prefix tests/browser
npm test --prefix tests/browser
```

Puppeteer downloads its pinned Chrome build during `npm ci`. To use an
already installed Chromium, set `PUPPETEER_SKIP_DOWNLOAD=true` for installation
and `PUPPETEER_EXECUTABLE_PATH=/path/to/chromium` when running the tests.
Set `DUFS_BIN` to test a different dufs executable. The fixture uses `--assets`
to serve the checkout's current assets; it does not depend on when the binary
last embedded those files.

The Acorn check parses `assets/index.js` as ES2017 to protect older WebViews
from syntax that would prevent the entire script loading.
