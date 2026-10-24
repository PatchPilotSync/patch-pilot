import { SELF } from "cloudflare:test";
import { describe, expect, it } from "vitest";

const RIG = "0123456789abcdef0123456789abcdef";
const TOKEN = "a".repeat(64);
const OTHER = "b".repeat(64);
const base = `https://relay.test/v1/rigs/${RIG}`;
const auth = (t = TOKEN) => ({ Authorization: `Bearer ${t}` });

function put(name: string, body: BodyInit, token = TOKEN, rig = RIG) {
  return SELF.fetch(`https://relay.test/v1/rigs/${rig}/objects/${name}`, {
    method: "PUT",
    headers: auth(token),
    body,
  });
}

async function socket(url: string, headers: Record<string, string> = {}) {
  const res = await SELF.fetch(url, { headers: { Upgrade: "websocket", ...headers } });
  expect(res.status).toBe(101);
  const ws = res.webSocket!;
  const inbox: Array<string | ArrayBuffer> = [];
  const waiters: Array<() => void> = [];
  ws.addEventListener("message", (e) => {
    inbox.push(e.data);
    waiters.shift()?.();
  });
  ws.accept();
  ws.binaryType = "arraybuffer";
  const next = async () => {
    if (!inbox.length) await new Promise<void>((r) => waiters.push(r));
    return inbox.shift()!;
  };
  return { ws, next };
}

describe("rig objects", () => {
  it("first PUT claims the rig, then only that token reads and writes", async () => {
    const rig = "1".repeat(32);
    expect((await put("machine/pc", "sealed-1", TOKEN, rig)).status).toBe(200);
    expect((await put("machine/pc", "sealed-2", OTHER, rig)).status).toBe(401);

    const ok = await SELF.fetch(`https://relay.test/v1/rigs/${rig}/objects/machine/pc`, {
      headers: auth(),
    });
    expect(ok.status).toBe(200);
    expect(new TextDecoder().decode(await ok.arrayBuffer())).toBe("sealed-1");
    expect(Number(ok.headers.get("x-updated"))).toBeGreaterThan(0);

    const nope = await SELF.fetch(`https://relay.test/v1/rigs/${rig}/objects/machine/pc`, {
      headers: auth(OTHER),
    });
    expect(nope.status).toBe(401);
  });

  it("an unclaimed rig reveals nothing", async () => {
    const res = await SELF.fetch(`https://relay.test/v1/rigs/${"2".repeat(32)}/objects`, {
      headers: auth(),
    });
    expect(res.status).toBe(401);
  });

  it("lists, replaces and deletes objects", async () => {
    await put("machine/pc", "one");
    await put("machine/mac", "two!");
    await put("machine/pc", "three");
    const list = await (await SELF.fetch(`${base}/objects`, { headers: auth() })).json<{
      objects: Array<{ name: string; size: number }>;
    }>();
    expect(list.objects.map((o) => [o.name, o.size])).toEqual([
      ["machine/mac", 4],
      ["machine/pc", 5],
    ]);
    const del = await SELF.fetch(`${base}/objects/machine/mac`, {
      method: "DELETE",
      headers: auth(),
    });
    expect(del.status).toBe(204);
    const gone = await SELF.fetch(`${base}/objects/machine/mac`, { headers: auth() });
    expect(gone.status).toBe(404);
  });

  it("refuses bad names, bad tokens, empty and oversized bodies", async () => {
    expect((await put("Machine/PC", "x")).status).toBe(400);
    expect((await put("machine/a_b", "x")).status).toBe(400);
    expect((await put("machine/pc", "x", "short")).status).toBe(401);
    expect((await put("machine/pc", "")).status).toBe(400);
    expect((await put("machine/pc", new Uint8Array(2_000_001))).status).toBe(413);
    const bad = await SELF.fetch("https://relay.test/v1/rigs/NOTHEX/objects/x");
    expect(bad.status).toBe(404);
  });

  it("pings live listeners when an object changes", async () => {
    await put("machine/pc", "claim");
    const { ws, next } = await socket(`${base}/live`, auth());
    await put("machine/mac", "sealed");
    const event = JSON.parse((await next()) as string);
    expect(event).toMatchObject({ type: "changed", name: "machine/mac" });
    ws.send("ping");
    expect(await next()).toBe("pong");
    ws.close();
  });

  it("refuses live listeners without the token", async () => {
    await put("machine/pc", "claim");
    const res = await SELF.fetch(`${base}/live`, {
      headers: { Upgrade: "websocket", ...auth(OTHER) },
    });
    expect(res.status).toBe(401);
  });
});

describe("link mailbox", () => {
  it("forwards frames between two machines, buffering for the late one", async () => {
    const pc = await socket("https://relay.test/v1/link/7K2M");
    pc.ws.send("pc-hello");
    const mac = await socket("https://relay.test/v1/link/7K2M");
    expect(await mac.next()).toBe("pc-hello");
    mac.ws.send("mac-hello");
    expect(await pc.next()).toBe("mac-hello");
    pc.ws.close();
  });

  it("forwards binary frames unchanged", async () => {
    const pc = await socket("https://relay.test/v1/link/BBBB");
    pc.ws.send(new Uint8Array([1, 2, 3]));
    const mac = await socket("https://relay.test/v1/link/BBBB");
    const got = await mac.next();
    expect(got).toBeInstanceOf(ArrayBuffer);
    expect([...new Uint8Array(got as ArrayBuffer)]).toEqual([1, 2, 3]);
    mac.ws.send(new Uint8Array([9]).buffer);
    expect([...new Uint8Array((await pc.next()) as ArrayBuffer)]).toEqual([9]);
    pc.ws.close();
  });

  it("allows one attempt per code", async () => {
    const a = await socket("https://relay.test/v1/link/AAAA");
    const b = await socket("https://relay.test/v1/link/AAAA");
    const third = await SELF.fetch("https://relay.test/v1/link/AAAA", {
      headers: { Upgrade: "websocket" },
    });
    expect(third.status).toBe(409);
    a.ws.close();
    b.ws.close();
    const again = await SELF.fetch("https://relay.test/v1/link/AAAA", {
      headers: { Upgrade: "websocket" },
    });
    expect(again.status).toBe(409);
  });

  it("refuses names that aren't link-code mailboxes", async () => {
    for (const name of ["aaaa", "AAA", "AAAAA", "AAIA"]) {
      const res = await SELF.fetch(`https://relay.test/v1/link/${name}`, {
        headers: { Upgrade: "websocket" },
      });
      expect(res.status, name).toBe(404);
    }
  });
});
