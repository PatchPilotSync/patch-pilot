// Patch Pilot sync relay (decision #38): a dumb, end-to-end-encrypted store.
//
// Machines encrypt everything before upload (crates/sync), so the relay only
// ever holds opaque bytes. It does three things:
//
//   /v1/rigs/<rig id>/objects/<name>   PUT / GET / DELETE one sealed object
//   /v1/rigs/<rig id>/objects          GET the list (name, size, updated)
//   /v1/rigs/<rig id>/live             WebSocket: {"type":"changed",...} pings
//   /v1/link/<mailbox>                 WebSocket: pairs two machines for a link
//
// Rig requests carry `Authorization: Bearer <relay token>`. The first PUT to a
// rig claims it; the relay keeps only a SHA-256 of the token.

import { DurableObject } from "cloudflare:workers";

export interface Env {
  RIG: DurableObjectNamespace<Rig>;
  MAILBOX: DurableObjectNamespace<Mailbox>;
}

const RIG_ID = /^[0-9a-f]{32}$/;
const TOKEN = /^[0-9a-f]{64}$/;
const OBJECT = /^[a-z0-9-]+(\/[a-z0-9-]+)*$/;
const MAILBOX_NAME = /^[0-9A-HJKMNP-TV-Z]{4}$/;
/** Durable Object SQLite rows top out at 2 MB. */
const MAX_OBJECT_BYTES = 2_000_000;
/** How long a link attempt may stay open. */
const LINK_TTL_MS = 10 * 60 * 1000;

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const parts = url.pathname.split("/").filter(Boolean);
    if (url.pathname === "/health") return new Response("ok");
    if (parts[0] !== "v1") return text(404, "not found");

    if (parts[1] === "rigs" && parts.length >= 4 && RIG_ID.test(parts[2])) {
      const rig = env.RIG.get(env.RIG.idFromName(parts[2]));
      return rig.fetch(request);
    }
    if (parts[1] === "link" && parts.length === 3 && MAILBOX_NAME.test(parts[2])) {
      const box = env.MAILBOX.get(env.MAILBOX.idFromName(parts[2]));
      return box.fetch(request);
    }
    return text(404, "not found");
  },
} satisfies ExportedHandler<Env>;

/** One linked rig: its sealed objects and its live listeners. */
export class Rig extends DurableObject<Env> {
  private sql: SqlStorage;

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    // Keep-alive pings are answered without waking the object.
    ctx.setWebSocketAutoResponse(new WebSocketRequestResponsePair("ping", "pong"));
    this.sql.exec(`
      CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS objects (
        name TEXT PRIMARY KEY, data BLOB NOT NULL, updated INTEGER NOT NULL);
    `);
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    // /v1/rigs/<id>/<rest...>
    const rest = url.pathname.split("/").slice(4);
    const { auth, hash } = await this.authorize(request);

    if (rest[0] === "live" && rest.length === 1) {
      if (request.headers.get("Upgrade") !== "websocket") return text(426, "websocket only");
      if (auth !== "ok") return text(401, "unauthorized");
      const pair = new WebSocketPair();
      this.ctx.acceptWebSocket(pair[1]);
      return new Response(null, { status: 101, webSocket: pair[0] });
    }

    if (rest[0] !== "objects") return text(404, "not found");
    if (rest.length === 1) {
      if (request.method !== "GET") return text(405, "method not allowed");
      if (auth !== "ok") return text(401, "unauthorized");
      const rows = this.sql
        .exec<{ name: string; size: number; updated: number }>(
          "SELECT name, length(data) AS size, updated FROM objects ORDER BY name",
        )
        .toArray();
      return Response.json({ objects: rows });
    }

    const name = rest.slice(1).join("/");
    if (name.length > 128 || !OBJECT.test(name)) return text(400, "bad object name");

    switch (request.method) {
      case "PUT": {
        if (auth === "bad") return text(401, "unauthorized");
        const data = await request.arrayBuffer();
        if (data.byteLength === 0) return text(400, "empty object");
        if (data.byteLength > MAX_OBJECT_BYTES) return text(413, "object too large");
        if (auth === "unclaimed") {
          this.sql.exec("INSERT INTO meta (key, value) VALUES ('token_hash', ?)", hash);
        }
        const updated = Date.now();
        this.sql.exec(
          "INSERT INTO objects (name, data, updated) VALUES (?, ?, ?) " +
            "ON CONFLICT(name) DO UPDATE SET data = excluded.data, updated = excluded.updated",
          name,
          data,
          updated,
        );
        this.broadcast({ type: "changed", name, updated });
        return Response.json({ name, updated });
      }
      case "GET": {
        if (auth !== "ok") return text(401, "unauthorized");
        const row = this.sql
          .exec<{ data: ArrayBuffer; updated: number }>(
            "SELECT data, updated FROM objects WHERE name = ?",
            name,
          )
          .toArray()[0];
        if (!row) return text(404, "no such object");
        return new Response(row.data, {
          headers: {
            "content-type": "application/octet-stream",
            "x-updated": String(row.updated),
          },
        });
      }
      case "DELETE": {
        if (auth !== "ok") return text(401, "unauthorized");
        this.sql.exec("DELETE FROM objects WHERE name = ?", name);
        this.broadcast({ type: "deleted", name, updated: Date.now() });
        return new Response(null, { status: 204 });
      }
      default:
        return text(405, "method not allowed");
    }
  }

  // Hibernating sockets: listeners cost nothing while the rig is quiet.
  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    if (message === "ping") ws.send("pong");
  }

  async webSocketClose(ws: WebSocket, code: number): Promise<void> {
    ws.close(code === 1005 ? 1000 : code, "bye");
  }

  private broadcast(event: object): void {
    const msg = JSON.stringify(event);
    for (const ws of this.ctx.getWebSockets()) {
      try {
        ws.send(msg);
      } catch {
        // Gone; the runtime cleans it up.
      }
    }
  }

  private async authorize(
    request: Request,
  ): Promise<{ auth: "ok" | "unclaimed" | "bad"; hash: string }> {
    const header = request.headers.get("Authorization") ?? "";
    const token = header.startsWith("Bearer ") ? header.slice(7) : "";
    if (!TOKEN.test(token)) return { auth: "bad", hash: "" };
    const hash = await sha256(token);
    const stored = this.sql
      .exec<{ value: string }>("SELECT value FROM meta WHERE key = 'token_hash'")
      .toArray()[0]?.value;
    if (stored === undefined) return { auth: "unclaimed", hash };
    return { auth: stored === hash ? "ok" : "bad", hash };
  }
}

/**
 * Where two machines meet to link (decision #38). Forwards each frame to the
 * other side, buffering until both are here. One attempt per mailbox: once two
 * machines have met, the mailbox refuses everyone for LINK_TTL_MS, so a wrong
 * guess at a code can't be retried quickly. Sessions close after LINK_TTL_MS.
 */
export class Mailbox extends DurableObject<Env> {
  private peers: WebSocket[] = [];
  private queued: Array<string | ArrayBuffer> = [];

  async fetch(request: Request): Promise<Response> {
    if (request.headers.get("Upgrade") !== "websocket") return text(426, "websocket only");
    const burnedUntil = (await this.ctx.storage.get<number>("burned_until")) ?? 0;
    if (Date.now() < burnedUntil || this.peers.length >= 2) {
      return text(409, "this link code was already used; make a new one");
    }

    const pair = new WebSocketPair();
    const ws = pair[1];
    ws.accept();
    // Link messages are binary; the default here is Blob, which send() would
    // turn into the text "[object Blob]".
    ws.binaryType = "arraybuffer";
    this.peers.push(ws);
    if (this.peers.length === 1) {
      await this.ctx.storage.setAlarm(Date.now() + LINK_TTL_MS);
    } else {
      // Both here: this mailbox is spent, whatever happens next.
      await this.ctx.storage.put("burned_until", Date.now() + LINK_TTL_MS);
      for (const m of this.queued) ws.send(m);
      this.queued = [];
    }

    ws.addEventListener("message", (e) => {
      const other = this.peers.find((p) => p !== ws);
      if (other) other.send(e.data);
      else if (this.queued.length < 8) this.queued.push(e.data);
    });
    ws.addEventListener("close", () => this.closeAll());
    ws.addEventListener("error", () => this.closeAll());
    return new Response(null, { status: 101, webSocket: pair[0] });
  }

  async alarm(): Promise<void> {
    this.closeAll();
    const burnedUntil = (await this.ctx.storage.get<number>("burned_until")) ?? 0;
    if (Date.now() >= burnedUntil) await this.ctx.storage.deleteAll();
    else await this.ctx.storage.setAlarm(burnedUntil);
  }

  private closeAll(): void {
    const peers = this.peers;
    this.peers = [];
    this.queued = [];
    for (const p of peers) {
      try {
        p.close(1000, "link finished");
      } catch {
        // Already closed.
      }
    }
  }
}

async function sha256(s: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(s));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

function text(status: number, body: string): Response {
  return new Response(body, { status, headers: { "content-type": "text/plain" } });
}
