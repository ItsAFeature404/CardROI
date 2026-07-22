# Security Policy

## Reporting a vulnerability

Please don't open a regular public issue for anything sensitive — a
public issue would broadcast the vulnerability to everyone before a fix
exists. Instead, open a
[GitHub issue](https://github.com/ItsAFeature404/CardROI/issues) with a
deliberately vague title and a note asking to coordinate a private
channel, or check the repo's Security tab in case private vulnerability
reporting has been enabled since this was written. This is a
solo-maintained project, so there's no guaranteed response time, but
reports will be looked at.

## What CardROI's actual attack surface looks like

There's **no network code at all** in the CLI or the web app's own
logic — neither ever makes an outbound request, listens on a port, or
phones home. That rules out an entire category of vulnerability (remote
exploits, data exfiltration over the network, server-side issues) by
construction.

The web app's own trust boundary is the browser it runs in: it fetches
its own code from wherever it's hosted (like any website), then runs
entirely client-side from that point on. Its data lives in that
browser's own storage (IndexedDB) — there's no server holding a copy to
compromise, but the flip side is real too: whoever has access to that
browser profile on that device has access to that data, the same as any
other locally-stored browser data.

What's actually left to worry about:
- **The CLI's SQLite database file.** CardROI's CLI reads and writes a
  single local `.db` file with the permissions of whatever user runs it.
  Treat that file like any other sensitive local data file — it contains
  your full financial ledger.
- **The dependency supply chain.** Like any Rust project, CardROI depends
  on third-party crates. CI runs `cargo audit` against the RustSec
  advisory database on every push, and dependency-reported vulnerabilities
  get triaged and patched as they're found.

## Supported versions

Pre-1.0 — only the latest commit on `main` is supported. There's no
long-term-support branch or backported patches yet.
