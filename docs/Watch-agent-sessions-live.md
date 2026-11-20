# Watch agent sessions live

Obscura is headless, so an agent driving it through MCP, Puppeteer, or Playwright works invisibly. The CDP screencast surface lets you stream what the browser sees to a local tab while the agent works, which helps when supervising long tasks or debugging what an agent clicked.

This guide builds a small live viewer on top of `obscura serve`. It uses only Node's built-in modules and its native WebSocket client (Node 21+).

## Glass Pane observer

Glass Pane uses the observer contract on a server started with `--pane-observer`. Connect with `glass-pane --attach <ws-url> --cdp-token <token> --observer-token <observer-token>`; the observer token can also be supplied as `OMNIS_GLASS_PANE_OBSERVER_TOKEN`. The observer and CDP tokens are separate. The pane follows the most recently active page, or accepts `--target <connectionId>/<targetId>` to pin a page. If there are no pages, it opens its own blank page. MCP stdio sessions run in another process and are not visible to this observer in v1.1. Mouse input is not individually receipted; navigation caused by input follows the existing navigation gate and receipt path.

The pane and server must use the same state folder (`--state-dir` or `OMNIS_GLASS_STATE_DIR`), or the receipt rail will show the wrong ledger.

The standalone viewer below uses an ordinary CDP connection. On the current per-connection server it cannot view pages owned by another client; use the observer pane for that purpose.

## How it works

1. Start the CDP server:

```bash
obscura serve --port 9222
```

2. Run the viewer script below:

```bash
node watch.mjs
```

3. Open http://localhost:8080 in a browser. The script shows pages available on its own CDP connection.

The script attaches to the CDP endpoint, captures the current page twice a second with `Page.captureScreenshot`, and forwards JPEG frames to your browser over Server-Sent Events.

```js
// watch.mjs
// Usage: node watch.mjs [cdpPort] [httpPort]
import http from "node:http";

const cdpPort = process.argv[2] ?? 9222;
const httpPort = process.argv[3] ?? 8080;

const clients = new Set();
let latest = null;
let sessionId = null;
let ws = null;

const page = `
<!doctype html>
<html>
<head><meta charset="utf-8"><title>Obscura live</title>
<style>body{margin:0;background:#111;display:grid;place-items:center;height:100vh}
img{max-width:100%;max-height:100%}</style></head>
<body><img id="s" alt="live page">
<script>
const img = document.getElementById("s");
let old = null;
const es = new EventSource("/events");
es.onmessage = (e) => {
  const bytes = Uint8Array.from(atob(e.data), (c) => c.charCodeAt(0));
  const url = URL.createObjectURL(new Blob([bytes], { type: "image/jpeg" }));
  img.src = url;
  if (old) URL.revokeObjectURL(old);
  old = url;
};
</script></body>
</html>`;

const server = http.createServer((req, res) => {
  if (req.url === "/events") {
    res.writeHead(200, {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-store",
      Connection: "keep-alive",
    });
    res.socket.setNoDelay(true);
    if (latest) res.write(`data:${latest}\n\n`);
    clients.add(res);
    req.on("close", () => clients.delete(res));
  } else {
    res.writeHead(200, { "Content-Type": "text/html" });
    res.end(page);
  }
});

server.listen(httpPort, "127.0.0.1", () => {
  console.log(`live view: http://localhost:${httpPort}`);
});

function broadcast(base64) {
  latest = base64;
  for (const res of clients) {
    if (res.writable) res.write(`data:${base64}\n\n`);
  }
}

async function connect() {
  ws = new WebSocket(`ws://127.0.0.1:${cdpPort}/devtools/browser`);
  let id = 0;
  const pending = new Map();
  const call = (method, params = {}, sess) =>
    new Promise((resolve, reject) => {
      const mid = ++id;
      pending.set(mid, { resolve, reject });
      const msg = { id: mid, method, params };
      if (sess) msg.sessionId = sess;
      ws.send(JSON.stringify(msg));
    });

  ws.addEventListener("message", (ev) => {
    const msg = JSON.parse(ev.data);
    if (msg.id && pending.has(msg.id)) {
      const p = pending.get(msg.id);
      pending.delete(msg.id);
      msg.error ? p.reject(new Error(msg.error.message)) : p.resolve(msg.result);
    }
  });
  ws.addEventListener("close", () => {
    sessionId = null;
    setTimeout(() => connect().catch(retry), 2000);
  });
  ws.addEventListener("error", () => ws.close());

  await new Promise((resolve) => ws.addEventListener("open", resolve));

  // reuse the page target an agent session may already have created
  const { targetInfos } = await call("Target.getTargets");
  const existing = targetInfos.find((t) => t.type === "page");
  if (existing) {
    // pre-existing target: attach explicitly and use the returned session id
    const attached = await call("Target.attachToTarget", {
      targetId: existing.targetId,
      flatten: true,
    });
    sessionId = attached.sessionId;
  } else {
    // a freshly created target is auto-attached under a managed session id
    const created = await call("Target.createTarget", { url: "about:blank" });
    sessionId = `${created.targetId}-session`;
  }
  await call("Page.enable", {}, sessionId);

  // capture on an interval instead of Page.startScreencast: screencast
  // frames stream to the session that drives the page, so a passive
  // viewer stops receiving them once an agent takes over. An explicit
  // capture always reflects the current page state.
  const capture = async () => {
    if (ws.readyState !== WebSocket.OPEN || !sessionId) return;
    try {
      const shot = await call(
        "Page.captureScreenshot",
        { format: "jpeg", quality: 70 },
        sessionId
      );
      if (shot.data && shot.data.length > 100) broadcast(shot.data);
    } catch {
      // transient failures during navigation are normal, retry next tick
    }
    setTimeout(capture, 500);
  };
  capture();
}

function retry(err) {
  console.error(err.message);
  sessionId = null;
  setTimeout(() => connect().catch(retry), 2000);
}
connect().catch(retry);
```


> For a packaged version with adaptive pacing (fast while painting, idle back-off) and multi-page handling, see `tools/live-view.mjs`.
## Things worth knowing

- **Connection scope.** The example polls `Page.captureScreenshot` on its own connection. It cannot see another connection’s pages on the current server; the Glass observer routes frames from the owning connection.
- **Acknowledge every frame.** Without `Page.screencastFrameAck`, frame delivery stops.
- **One client per page socket.** If an agent's CDP client already holds the page's `webSocketDebuggerUrl`, connecting to it again fails with `503`. Attach through the browser endpoint instead (`ws://127.0.0.1:9222/devtools/browser`) using `Target.attachToTarget` with `flatten: true`, then pass the returned session id as `sessionId` on every command.
- **Still images vs continuous view.** The MCP server exposes `browser_screenshot` for one-shot captures. Screencast is the right tool when you want to watch continuously; it stays CDP-only by design.

## Verifying

With `obscura serve` running and the viewer open:

1. Navigate a page on the viewer’s own CDP connection. A separate client or MCP stdio session is not visible to this script.
2. The tab shows the page within a second of it painting.
3. Closing the viewer tab and reopening it resumes from the most recent frame.

Scripts run in one page at a time. A page that is not the working page keeps its content but loses its scripts, timers, and event handlers until it is loaded again; the pane shows a notice while the displayed page has stopped scripts, and Reload brings them back.

`Glass.listTargets` includes the boolean `scriptsStopped` for each page. When it changes, authorized observers receive `Glass.scriptsStoppedChanged` with `connectionId`, `targetId`, and `scriptsStopped`; the change does not create a frame or an action receipt.
