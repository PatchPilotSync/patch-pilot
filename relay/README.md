# Patch Pilot sync relay

The small Cloudflare Worker that lets linked machines see each other live
(decision #38). Machines encrypt everything before upload (`crates/sync`), so
the relay only stores bytes it can't read.

| Route | What it does |
| --- | --- |
| `PUT/GET/DELETE /v1/rigs/<rig id>/objects/<name>` | One sealed object, e.g. `machine/<machine id>`. Max 2 MB. |
| `GET /v1/rigs/<rig id>/objects[?prefix=machine/]` | `{ objects: [{ name, size, updated }] }`, only names starting with `prefix` when given |
| `POST /v1/rigs/<rig id>/present` | Body `{ names: [...] }` (up to 1,000): `{ present: [...] }`, the ones that exist |
| `GET /v1/rigs/<rig id>/live` (WebSocket) | `{"type":"changed"\|"deleted","name","updated"}` on every write |
| `GET /v1/link/<mailbox>` (WebSocket) | Pairs two machines for the link handshake; one attempt per code, 10 minutes max |
| `GET /health` | `ok` |

Rig routes need `Authorization: Bearer <relay token>` (`RigKey::relay_token`).
The first `PUT` to a rig claims it; the relay keeps only a SHA-256 of the token.
Each rig and each mailbox is a Durable Object with its own SQLite storage.
Live listeners hibernate, so a quiet rig costs nothing.

Durable Objects bill by **rows read** (5 million a day on the free plan), and
a rig has one row per shared file, often tens of thousands. A full
`GET objects` reads every one, so the app never makes it: it lists with a
prefix (a primary-key range) and asks `present` about files it hasn't seen
there yet (decision #113).

## Develop

```sh
cd relay
npm ci
npm test            # runs the Worker in the real workerd runtime
npm run typecheck
npm run dev         # local relay on http://localhost:8787
```

## Deploy

`.github/workflows/relay.yml` deploys from `main` when `relay/` changes (or
from the Actions tab), once these repo secrets exist:

1. Make a free account at <https://dash.cloudflare.com/sign-up>.
2. My Profile > API Tokens > Create Token > "Edit Cloudflare Workers" template.
   Save it as the `CLOUDFLARE_API_TOKEN` repo secret.
3. Copy the Account ID (Workers & Pages overview, right side) into the
   `CLOUDFLARE_ACCOUNT_ID` repo secret.

The relay is then at `https://patch-pilot-relay.<your subdomain>.workers.dev`.
Durable Objects on the free plan need the SQLite storage backend, which this
Worker uses.
