// The browser code the tools share.
//
// A page keeps an event stream open, redraws when the room changes, and
// posts what the person in front of it did. All three do that the same
// way, so it is written once here.

/**
 * The path this tool is served under: "" on its own server, "/numbers"
 * when the shared room serves several tools from one. The server writes
 * it into the page.
 */
export const BASE: string =
  document
    .querySelector<HTMLMetaElement>('meta[name="rooms-base"]')
    ?.content.replace(/\/+$/, "") ?? "";

/** A path on this tool's server, under its base. */
export function here(path: string): string {
  return path.startsWith("/") ? `${BASE}${path}` : path;
}

/**
 * Whether the page is inside the shared room's frame. The shared room
 * shows the join link and the header itself, so the tool's own are
 * hidden (see `.embedded` in rooms.css).
 */
export const EMBEDDED: boolean = (() => {
  try {
    return window.self !== window.top;
  } catch {
    return true; // a frame on another origin cannot see its parent
  }
})();
if (EMBEDDED) document.documentElement.classList.add("embedded");

/** Everything the server sends is stamped with its own clock. */
export interface Envelope<V> {
  now: number;
  view: V;
}

/** The element with this id. The page is expected to have it. */
export function byId<T extends HTMLElement = HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (!found) throw new Error(`the page has no #${id}`);
  return found as T;
}

/** Escapes text going into markup. Names and questions are typed by
 *  players, so they cannot be trusted. */
export function esc(text: unknown): string {
  return String(text).replace(
    /[&<>"]/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] ?? c,
  );
}

/** A number written the way a person would, at any size. */
export function num(value: number | null | undefined): string {
  if (value === null || value === undefined || !isFinite(value)) return "—";
  const size = Math.abs(value);
  if (size !== 0 && (size < 1e-4 || size >= 1e15)) return value.toExponential(2);
  const places = size === 0 ? 0 : size < 10 ? 3 : size < 1000 ? 2 : 0;
  return value.toLocaleString(undefined, { maximumFractionDigits: places });
}

export function plural(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

/** "Ada", "Ada and Bo", "Ada, Bo and Cy". */
export function list(names: string[]): string {
  if (names.length === 0) return "";
  if (names.length === 1) return names[0];
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

/** Shows a refusal, or clears it when given nothing. */
export function fail(message: string): void {
  const bar = byId("error");
  bar.textContent = message;
  bar.hidden = !message;
}

/** A refused request. The status is kept so that a missing room can be
 *  told from a server that is temporarily unreachable. */
export class Refused extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = "Refused";
    this.status = status;
  }
}

/**
 * A request to the room. The server's message becomes the error message;
 * every endpoint answers in JSON, including when it refuses.
 */
export async function api<T>(method: string, path: string, body?: unknown): Promise<T> {
  const response = await fetch(here(path), {
    method,
    headers: body === undefined ? {} : { "content-type": "application/json" },
    body: body === undefined ? null : JSON.stringify(body),
  });
  const text = await response.text();
  let data: unknown = {};
  try {
    data = text ? JSON.parse(text) : {};
  } catch {
    data = {};
  }
  if (!response.ok) {
    const said = (data as { error?: string }).error;
    throw new Refused(
      response.status,
      said || text || "Something went wrong. Try again.",
    );
  }
  return data as T;
}

/** How long to wait before reconnecting. */
const RETRY_MS = 2000;
/** After this long with nothing received, the page is out of date. */
const SILENT_MS = 8000;

/** What a page can ask about the room it is watching. */
export interface Feed {
  /** The server's clock, which is the one every countdown is run off. */
  now: () => number;
  /** Whether the view on screen is still being kept up to date. */
  connected: () => boolean;
  /** Takes a view returned by a request, so that a screen which just
   *  acted does not wait for its own event to arrive. */
  accept: (envelope: Envelope<unknown>) => void;
  /** Whether the room is gone: the server answered 404, because the room
   *  has finished, the link is wrong, or the host took this player out.
   *  Nothing more will arrive, and nothing is retried. */
  gone: () => boolean;
}

/**
 * Keeps a room's event stream open and passes every view to `onView`.
 *
 * The first view comes from `snapshot`, so the page fills immediately;
 * the rest arrive on `events`. The browser reconnects by itself, and this
 * reconnects when the browser gives up. A view that has not changed is
 * not redrawn.
 *
 * Phone clocks and the server clock differ by enough to matter when
 * something is counting down, so every view carries the server's time and
 * `now()` works from the difference.
 *
 * When the server says the room is not there (404), the feed is gone: it
 * stops, `connected()` no longer counts as reconnecting, and `onMissing`
 * is called once. A stream the server closes, as it does when the host
 * takes a player out, finds this out on its next connect.
 */
export function subscribe<V>(
  snapshot: string,
  events: string,
  onView: (view: V) => void,
  onMissing?: () => void,
): Feed {
  let offset = 0;
  let heard = 0;
  let last: string | null = null;
  let lostAt: number | null = null;
  let gone = false;

  const accept = (envelope: Envelope<unknown>): void => {
    if (gone) return;
    offset = envelope.now - Date.now();
    heard = Date.now();
    const view = JSON.stringify(envelope.view);
    if (view === last) return;
    last = view;
    onView(envelope.view as V);
  };

  const connect = async (): Promise<void> => {
    if (gone) return;
    try {
      accept(await api<Envelope<V>>("GET", snapshot));
    } catch (e) {
      if (e instanceof Refused && e.status === 404) {
        gone = true;
        lostAt = null;
        if (onMissing) onMissing();
        return;
      }
      setTimeout(() => void connect(), RETRY_MS);
      return;
    }
    const source = new EventSource(here(events));
    source.onmessage = (event: MessageEvent<string>) => {
      lostAt = null;
      try {
        accept(JSON.parse(event.data) as Envelope<V>);
      } catch {
        /* a half-delivered event; the next one will be whole */
      }
    };
    source.onopen = () => {
      lostAt = null;
    };
    source.onerror = () => {
      lostAt ??= Date.now();
      if (source.readyState === EventSource.CLOSED) {
        source.close();
        setTimeout(() => void connect(), RETRY_MS);
      }
    };
  };
  void connect();

  return {
    now: () => Date.now() + offset,
    connected: () =>
      !gone &&
      heard > 0 &&
      Date.now() - heard < SILENT_MS &&
      !(lostAt !== null && Date.now() - lostAt > RETRY_MS),
    accept,
    gone: () => gone,
  };
}

/** What a page shows when the room is gone, unless it says otherwise. */
export const MISSING = "That room is not here. It may have finished, or the link may be wrong.";

/** Hides every screen and says why: what a page does when its room is
 *  gone, so that nobody answers into a screen that can no longer send. */
export function gone(message = MISSING): void {
  document.querySelectorAll<HTMLElement>("main>section").forEach((section) => {
    section.hidden = true;
  });
  document.body.classList.remove("stale");
  document.body.classList.add("gone");
  fail(message);
}

/** One room, one view, and what happens when the room is gone: by
 *  default the screen is hidden and `missing` shown in its place (see
 *  `gone`); a function instead is called.
 *
 *  `path` is the room's own address, `/api/h/<key>`: the event stream is
 *  found by adding `/events` to it. Passing the stream's address instead
 *  would listen on `/events/events` and the page would never update, so
 *  that is caught here. */
export function watch<V>(
  path: string,
  draw: (view: V) => void,
  missing: string | (() => void) = MISSING,
): Feed {
  const room = path.replace(/\/events$/, "");
  const onMissing = typeof missing === "string" ? () => gone(missing) : missing;
  return subscribe<V>(room, `${room}/events`, draw, onMissing);
}

/** Shows a failed request's message in the error bar. */
export function failed(e: unknown): void {
  fail(e instanceof Error ? e.message : String(e));
}

/** The start screen: one button that opens a room and goes to it. */
export function opening(button = "open"): void {
  show("start");
  onClick(button, async () => {
    byId<HTMLButtonElement>(button).disabled = true;
    try {
      const keys = await api<{ host: string }>("POST", "/api/rooms");
      location.href = here(`/h/${keys.host}`);
    } catch (e) {
      byId<HTMLButtonElement>(button).disabled = false;
      throw e;
    }
  });
}

/** Where a tool's player keys are checked and opened, for `joining`. */
export interface PlayerPaths {
  /** What answers 404 once a kept key no longer works:
   *  `/api/p/<key>` unless given. */
  check?: (key: string) => string;
  /** The player's page: `/p/<key>` unless given. */
  page?: (key: string) => string;
}

/** The join screen: how many are in already, and a name to join with.
 *  A phone that has joined this room before goes straight back to its
 *  own link, which is kept under `ns`, the tool's name. A tool whose
 *  players are not at `/p/<key>` says where they are in `paths`. */
export function joining(ns: string, key: string, paths: PlayerPaths = {}): void {
  const check = paths.check ?? ((kept: string) => `/api/p/${encodeURIComponent(kept)}`);
  const page = paths.page ?? ((kept: string) => `/p/${kept}`);
  const kept = remembered(ns, key);
  if (kept) {
    // Back to the link this phone was given, unless the host has taken
    // it out of the room since, in which case it joins afresh.
    api<unknown>("GET", check(kept))
      .then(() => location.replace(here(page(kept))))
      .catch((e: unknown) => {
        if (e instanceof Refused && e.status === 404) {
          forget(ns, key);
          askName(ns, key, page);
        } else {
          location.replace(here(page(kept)));
        }
      });
    return;
  }
  askName(ns, key, page);
}

function askName(ns: string, key: string, page: (key: string) => string): void {
  show("join");
  const at = `/api/j/${encodeURIComponent(key)}`;
  api<{ players: number }>("GET", at)
    .then((body) => {
      text(
        "join-count",
        body.players === 0
          ? "You are the first one in."
          : `${plural(body.players, "person", "people")} already in.`,
      );
    })
    .catch(failed);
  let sending = false;
  onSubmit("name-form", async () => {
    if (sending) return;
    sending = true;
    try {
      const body = await api<{ player: string }>("POST", at, {
        name: byId<HTMLInputElement>("name").value,
      });
      remember(ns, key, body.player);
      location.href = here(page(body.player));
    } finally {
      sending = false;
    }
  });
}

let shownJoin = "";

/** Puts the join link and its QR code on the host's screen, into
 *  `#join-link` and `#join-qr`, and only redraws them when it changes. */
export function showJoin(join: string): void {
  const link = joinLink(join);
  if (link === shownJoin) return;
  shownJoin = link;
  const anchor = byId<HTMLAnchorElement>("join-link");
  anchor.textContent = link;
  anchor.href = link;
  qrInto("join-qr", link);
}

/** Asks the server for the link as a QR code and inserts it, into the
 *  element or the element with that id. */
export function qrInto(where: string | HTMLElement, link: string): void {
  fetch(here("/api/qr"), { method: "POST", body: link })
    .then((response) => (response.ok ? response.text() : ""))
    .then((svg) => {
      target(where).innerHTML = svg;
    })
    .catch(() => {
      /* the link itself is printed above the code */
    });
}

/** Two digits, for a clock. */
function pad(n: number): string {
  return String(n).padStart(2, "0");
}

/** Milliseconds as a countdown: 9:05, or 1:09:05 past an hour. */
export function countdown(ms: number): string {
  const seconds = Math.max(0, Math.ceil(ms / 1000));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor(seconds / 60) % 60;
  return hours
    ? `${hours}:${pad(minutes)}:${pad(seconds % 60)}`
    : `${minutes}:${pad(seconds % 60)}`;
}

/** A moment as a wall clock would show it. */
export function timeOfDay(at: number): string {
  return new Date(at).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/**
 * An element, its properties and its children. Building the page from
 * these rather than from strings means text typed by a player is never
 * parsed as markup.
 */
export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<HTMLElementTagNameMap[K]> = {},
  ...children: (Node | string | null | undefined)[]
): HTMLElementTagNameMap[K] {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...children.filter((child) => child !== null && child !== undefined));
  return node;
}

/**
 * Runs `build` only when `key` differs from last time. Redrawing a list on
 * every frame loses the scroll position and any selected text in it.
 */
export function memo<A extends unknown[]>(
  build: (...args: A) => void,
): (key: unknown, ...args: A) => void {
  let last: unknown = Symbol("nothing yet");
  return (key: unknown, ...args: A) => {
    if (key === last) return;
    last = key;
    build(...args);
  };
}

/** Something this phone stored, or `fallback` if it stored nothing. */
export function stored<T>(key: string, fallback: T): T {
  try {
    const kept = localStorage.getItem(key);
    return kept === null ? fallback : (JSON.parse(kept) as T);
  } catch {
    return fallback;
  }
}

export function persist(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* a browser with storage turned off can still play */
  }
}

/** Copies a link and reports the result on the button that asked. */
export async function copyTo(button: HTMLElement, text: string): Promise<void> {
  const said = button.textContent;
  try {
    await navigator.clipboard.writeText(text);
    button.textContent = "Copied";
  } catch {
    button.textContent = "Copy failed, select the link instead";
  }
  setTimeout(() => {
    button.textContent = said;
  }, 2000);
}

/** Shows one of the page's screens and hides the rest. */
export function show(screen: string): void {
  document
    .querySelectorAll<HTMLElement>("main>section")
    .forEach((section) => {
      section.hidden = true;
    });
  byId(`screen-${screen}`).hidden = false;
}

export type Kind = "host" | "join" | "player" | "wall" | "start";

/** Which of the links this page was opened with. */
export function route(): { kind: Kind; key: string } {
  const path = location.pathname.startsWith(`${BASE}/`)
    ? location.pathname.slice(BASE.length)
    : location.pathname;
  const parts = path.split("/").filter((part) => part !== "");
  const kinds: Record<string, Kind> = { h: "host", j: "join", p: "player", w: "wall" };
  if (parts.length === 2 && kinds[parts[0]]) {
    return { kind: kinds[parts[0]], key: parts[1] };
  }
  return { kind: "start", key: "" };
}

/**
 * This phone's player key. Stored so that closing the tab and opening the
 * join link again rejoins as the same person.
 */
export function remembered(ns: string, key: string): string | null {
  try {
    return localStorage.getItem(`${ns}:${key}`);
  } catch {
    return null; // a browser with storage turned off still plays
  }
}

export function forget(ns: string, key: string): void {
  try {
    localStorage.removeItem(`${ns}:${key}`);
  } catch {
    /* nothing kept, nothing to forget */
  }
}

export function remember(ns: string, key: string, value: string): void {
  try {
    localStorage.setItem(`${ns}:${key}`, value);
  } catch {
    /* nothing to do: the link in the address bar is the fallback */
  }
}

/** The join link, from the key the room returned. */
export function joinLink(join: string): string {
  return `${location.origin}${BASE}/j/${join}`;
}

/** Connects a form to a handler whose request may be refused. */
export function onSubmit(id: string, run: () => Promise<void>): void {
  byId<HTMLFormElement>(id).addEventListener("submit", (event) => {
    event.preventDefault();
    run().catch((e: unknown) => fail(e instanceof Error ? e.message : String(e)));
  });
}

/** The same, for a button. */
export function onClick(id: string, run: () => Promise<void>): void {
  byId(id).addEventListener("click", () => {
    run().catch((e: unknown) => fail(e instanceof Error ? e.message : String(e)));
  });
}

/** A number field's value, or null if it is empty or not a number. */
export function value(id: string): number | null {
  const field = byId<HTMLInputElement>(id);
  const given = field.valueAsNumber;
  return isFinite(given) ? given : null;
}

export function setValue(id: string, given: number | null | undefined): void {
  byId<HTMLInputElement>(id).value =
    given === null || given === undefined ? "" : String(given);
}

export function text(id: string, said: string): void {
  byId(id).textContent = said;
}

export function html(id: string, markup: string): void {
  byId(id).innerHTML = markup;
}

export function hide(id: string, hidden: boolean): void {
  byId(id).hidden = hidden;
}

/**
 * Where a player's answer stands: not sent yet, sent and still open to
 * change, changed on the phone but not sent, locked in, or missed.
 */
export type Sent = "waiting" | "sent" | "changed" | "locked" | "missed";

/**
 * Says in one box whether a player's answer is in and whether they can
 * still change it. Each state has its own look, so a glance is enough.
 * `target` is the box or its id; `null` hides the box.
 */
export function sentState(target: string | HTMLElement, state: Sent | null, title = "", detail = ""): void {
  const box = typeof target === "string" ? byId(target) : target;
  box.hidden = state === null;
  if (state === null) return;
  const moved = box.classList.contains("just");
  box.className = `sent-state ${state}${moved ? " just" : ""}`;
  box.replaceChildren(el("b", {}, title), ...(detail ? [el("span", {}, detail)] : []));
}

/** The same box, new, for a screen built from elements. */
export function sentBox(state: Sent, title: string, detail = ""): HTMLElement {
  const box = el("div", { className: "sent-state" });
  box.setAttribute("aria-live", "polite");
  sentState(box, state, title, detail);
  return box;
}

/**
 * A moment of movement on that box after a send goes through, so a press
 * that sent is not mistaken for one that did nothing.
 */
export function landed(target: string | HTMLElement): void {
  const box = typeof target === "string" ? byId(target) : target;
  box.classList.remove("just");
  void box.offsetWidth;
  box.classList.add("just");
}

// ------------------------------------------------------------ the lobby
//
// What the server's `rooms::lobby` sends, and the pieces of screen that
// show it: who is in and connected, the host taking somebody out, the
// join link with its QR code, and the wall.

/** Somebody in the room, as the host and the wall see them. */
export interface Present {
  /** What the host's Remove sends. Not a secret, unlike their key. */
  id: number;
  name: string;
  /** Whether their phone is connected. Left out where the game does not
   *  let the room see that, as Petrov Day does while it is live. */
  connected?: boolean;
  /** Where the game seated them: a team, a side, a country. */
  seat?: number;
}

/** What `/api/w/<key>` sends: what the game lets the wall show of the
 *  room, and the game's own public part, whatever the game puts there. */
export interface WallView<G> {
  /** The join key for the QR code, or null to hide the code. */
  join: string | null;
  /** Everybody by name, or null when the names are not shown. */
  people: Present[] | null;
  /** How many are in, or null when that is not shown either. */
  count: number | null;
  /** How many of them are connected, or null when that is not shown. */
  connected: number | null;
  /** Something is under way: the game has the screen. */
  busy: boolean;
  game: G;
}

/** An element, or the id of one. */
function target(where: string | HTMLElement): HTMLElement {
  return typeof where === "string" ? byId(where) : where;
}

/**
 * What the lobby's pieces need to look right on any page, put into the
 * page the first time one of them is used, so that a tool with a
 * stylesheet of its own (Petrov Day) does not have to copy it.
 *
 * It goes first in `<head>`, so the page's own rules win wherever they
 * say something else about the same thing. Colours come from the shared
 * stylesheet's variables (`--border`, `--body`, `--mark`, `--bad`,
 * `--ink`, `--on-ink`, `--shadow`) and fall back to neutral ones; a page
 * sets those variables to restyle every piece at once.
 */
const LOBBY_CSS = `
.visually-hidden {
  position: absolute; width: 1px; height: 1px; overflow: hidden;
  clip-path: inset(50%); white-space: nowrap;
}
.roster { list-style: none; padding: 0; margin: 10px 0 0; display: grid; gap: 6px; }
.roster li {
  display: flex; align-items: center; gap: 10px; min-height: 44px; padding: 4px 0;
  border-bottom: 1px solid var(--border, rgba(127, 127, 127, 0.3));
}
.roster .who { flex: 1 1 auto; font-weight: 600; overflow-wrap: anywhere; }
.roster .away .who { color: var(--body, GrayText); font-weight: 400; }
.roster .seat { font-size: 0.85em; color: var(--body, GrayText); }
.roster .dot, .wall-people .dot {
  flex: none; display: inline-block; width: 10px; height: 10px; border-radius: 50%;
}
.roster .dot.on, .wall-people .dot.on {
  background: var(--mark, #3f8f4f);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--mark, #3f8f4f) 30%, transparent);
}
.roster .dot.off, .wall-people .dot.off { background: transparent; border: 2px solid var(--body, GrayText); }
.remove-away { margin: 10px 0 0; }
.remove-away .note { margin: 6px 0 0; overflow-wrap: anywhere; }
.roster .remove {
  border: 1px solid var(--border, rgba(127, 127, 127, 0.4)); background: transparent; color: inherit;
  border-radius: 6px; min-height: 40px; padding: 6px 12px; font: inherit; font-size: 0.85em; cursor: pointer;
}
.armed {
  border-color: var(--bad, #b3261e) !important; color: var(--bad, #b3261e) !important; font-weight: 700;
  background-image: linear-gradient(color-mix(in srgb, var(--bad, #b3261e) 20%, transparent),
    color-mix(in srgb, var(--bad, #b3261e) 20%, transparent));
  background-repeat: no-repeat; background-size: 0% 100%;
  animation: rooms-armed var(--armed-ms, 8s) linear;
}
/* A page's own .primary sets the whole background, which would wipe the
   bar and leave red text on a dark fill: an armed primary button is light. */
.primary.armed, .primary.armed:hover:not(:disabled) {
  background-color: var(--card, Canvas);
  background-image: linear-gradient(color-mix(in srgb, var(--bad, #b3261e) 20%, transparent),
    color-mix(in srgb, var(--bad, #b3261e) 20%, transparent));
  background-repeat: no-repeat; background-size: 0% 100%;
}
@keyframes rooms-armed { from { background-size: 100% 100%; } to { background-size: 0% 100%; } }
@media (prefers-reduced-motion: reduce) { .armed { animation: none; } }
.share .link { overflow-wrap: anywhere; }
.share .actions { display: flex; flex-wrap: wrap; gap: 8px; margin: 8px 0; }
.share .qr { margin-top: 12px; }
.share .qr svg { width: min(100%, 280px); height: auto; display: block; }
.wall-open.button { display: inline-block; margin-top: 12px; }
p.wall-open { margin-top: 6px; }
.embedded .wall-open { display: none; }
.reconnecting {
  position: fixed; top: calc(10px + env(safe-area-inset-top)); left: 50%; transform: translateX(-50%);
  z-index: 20; background: var(--ink, CanvasText); color: var(--on-ink, Canvas);
  padding: 8px 16px; border-radius: 30px; font-size: 0.9rem; font-weight: 700; margin: 0;
  box-shadow: 0 6px 22px var(--shadow, rgba(0, 0, 0, 0.25));
}
.wall-screen main { width: min(1600px, calc(100% - 32px)); max-width: none; }
.wall { display: grid; gap: 24px; }
.wall-join { display: grid; justify-items: center; gap: 12px; text-align: center; }
.wall-qr svg { display: block; width: min(78vw, 50vh, 520px); height: auto; }
.wall-link { font-size: clamp(1.1rem, 2.6vw, 2.2rem); font-weight: 700; word-break: break-all; margin: 0; }
.wall-screen .eyebrow { font-size: clamp(0.9rem, 1.5vw, 1.4rem); }
.wall-screen .wall-count { margin: 0; }
.wall-people {
  list-style: none; padding: 0; margin: 12px 0 0; display: flex; flex-wrap: wrap;
  gap: 8px 22px; font-size: clamp(1.1rem, 2.2vw, 2rem);
}
.wall-people li { display: flex; align-items: center; gap: 0.35em; border: 0; min-height: 0; padding: 0; }
.wall-people .dot { width: 0.5em; height: 0.5em; }
.wall-people .away { opacity: 0.6; }
.wall-game { margin-top: 24px; font-size: clamp(1.1rem, 2.2vw, 2rem); }
.wall-game:empty { display: none; }
.wall.many .wall-people { font-size: clamp(0.9rem, 1.5vw, 1.4rem); gap: 4px 16px; }
.wall.busy .wall-people, .wall.many.playing .wall-people { display: none; }
.wall.busy .wall-game, .wall.many.playing .wall-game { margin-top: 12px; }
.wall.busy .wall-qr svg { width: min(40vw, 28vh, 240px); }
.wall.busy .wall-link { font-size: clamp(0.8rem, 1.2vw, 1.1rem); }
.wall.busy .wall-join > .eyebrow { font-size: clamp(0.75rem, 1vw, 1rem); }
@media (min-width: 900px) {
  .wall { grid-template-columns: minmax(0, 2fr) minmax(0, 3fr); align-items: start; min-height: calc(100vh - 120px); }
  .wall.busy { grid-template-columns: minmax(0, 1fr) minmax(0, 5fr); }
  .wall.no-join { grid-template-columns: minmax(0, 1fr); }
}
.embedded .wall-join, .embedded .wall-count, .embedded .wall-people { display: none; }
.embedded .wall { display: block; min-height: 0; }
.embedded .wall-game { margin-top: 0; }
`;

let styled = false;

/** Puts the lobby's styles into the page, once. The pieces below call
 *  it themselves; a page that styles its own copy of them need not. */
export function lobbyStyles(): void {
  if (styled || document.getElementById("rooms-lobby-css")) return;
  styled = true;
  const style = document.createElement("style");
  style.id = "rooms-lobby-css";
  style.textContent = LOBBY_CSS;
  document.head.prepend(style);
}

/**
 * A press that cannot be undone asks again first. The first press turns
 * the button into the question, with a bar running down for the `ms` it
 * waits; a second press within them answers true. No dialog, so it works
 * the same in the hall's frame and on a phone held in one hand.
 *
 *     if (!confirmTap(button, "Tap again to remove Ada")) return;
 */
export function confirmTap(button: HTMLElement, ask: string, ms = 8000): boolean {
  lobbyStyles();
  if (button.dataset.armed === "1") {
    delete button.dataset.armed;
    button.classList.remove("armed");
    if (button.dataset.said !== undefined) button.textContent = button.dataset.said;
    return true;
  }
  button.dataset.armed = "1";
  button.dataset.said = button.textContent ?? "";
  button.style.setProperty("--armed-ms", `${ms}ms`);
  button.classList.add("armed");
  button.textContent = ask;
  setTimeout(() => {
    if (button.dataset.armed !== "1") return;
    delete button.dataset.armed;
    button.classList.remove("armed");
    button.textContent = button.dataset.said ?? "";
  }, ms);
  return false;
}

/**
 * Everybody the server says is away, and a button that takes them all
 * out: see `roster`'s `away`.
 */
export interface Away {
  /** The ids of everybody away, from the game's view (the server's
   *  `lobby::away_ids`): not connected, and not heard from in the game in
   *  this round or the one before. Which of the people that is, is the
   *  game's to say, so it is given rather than worked out here. */
  ids: number[];
  /** Takes them out, given the ids the host was shown:
   *  `api("POST", "/api/h/<key>/remove-away", { ids })`. The server takes
   *  out only those still away. */
  remove: (ids: number[]) => Promise<unknown>;
  /** What they are called on the button: "Remove 3 away" by default. A
   *  game whose players can mark themselves away says something else. */
  word?: string;
}

/**
 * "Remove 3 away" above a roster: one button for everybody `away`, with
 * their names under it, asking once more (`confirmTap`) before it takes
 * them out. Hidden while nobody is. Kept in its own box just before the
 * roster's, which is made the first time.
 */
function removeAway(box: HTMLElement, people: Present[], away: Away): void {
  let bar = box.previousElementSibling as HTMLElement | null;
  if (!bar?.classList.contains("remove-away")) {
    bar = el("div", { className: "remove-away" });
    box.before(bar);
  }
  const ids = new Set(away.ids);
  const gone = people.filter((p) => ids.has(p.id));
  const word = away.word ?? "away";
  const shape = JSON.stringify([word, gone.map((p) => [p.id, p.name])]);
  if (bar.dataset.shape === shape) return;
  bar.dataset.shape = shape;
  bar.hidden = gone.length === 0;
  if (gone.length === 0) {
    bar.replaceChildren();
    return;
  }
  const names = list(gone.map((p) => p.name));
  const button = el("button", {
    className: "button remove-all",
    type: "button",
    textContent: `Remove ${gone.length} ${word}`,
  });
  button.addEventListener("click", () => {
    if (!confirmTap(button, `Remove ${gone.length === 1 ? names : `all ${gone.length}`}? Tap again`)) return;
    button.disabled = true;
    away
      .remove(gone.map((p) => p.id))
      .then(() => fail(""))
      .catch(failed)
      .finally(() => {
        button.disabled = false;
      });
  });
  bar.replaceChildren(
    button,
    el(
      "p",
      { className: "note" },
      `Not connected, and nothing from them this round or the last: ${names}. They stay in the count until you remove them.`,
    ),
  );
}

/**
 * The list of who is in, with a dot for whose phone is connected where
 * the room may see that. Give `remove` on the host's screen and each
 * name gets a button that asks once more and then takes them out: it is
 * given the person, whose `id` is what the server's remove takes. Give
 * `away` as well and a "Remove 3 away" button above the list takes out
 * everybody the game says is away at once (see `Away`). `seat` names a
 * seat ("Country A"). Redrawn only when something in it changed, seat
 * names included, so a button that is asking keeps asking.
 */
export function roster(
  where: string | HTMLElement,
  people: Present[],
  options: {
    remove?: (person: Present) => Promise<unknown>;
    seat?: (seat: number) => string;
    away?: Away;
  } = {},
): void {
  lobbyStyles();
  const box = target(where);
  if (options.away) removeAway(box, people, options.away);
  const label = (person: Present): string =>
    person.seat !== undefined && options.seat ? options.seat(person.seat) : "";
  const shape = JSON.stringify([
    !!options.remove,
    people.map((p) => [p.id, p.name, p.connected, label(p)]),
  ]);
  if (box.dataset.shape === shape) return;
  box.dataset.shape = shape;
  box.classList.add("roster");
  box.replaceChildren(
    ...people.map((person) => {
      const known = person.connected !== undefined;
      const dot = known ? el("span", { className: `dot ${person.connected ? "on" : "off"}` }) : null;
      dot?.setAttribute("aria-hidden", "true");
      const state = known
        ? el("span", { className: "visually-hidden" }, person.connected ? " (connected)" : " (not connected)")
        : null;
      const seat = label(person) ? el("span", { className: "seat" }, label(person)) : null;
      const item = el(
        "li",
        { className: person.connected === false ? "away" : "" },
        dot,
        el("span", { className: "who" }, person.name),
        state,
        seat,
      );
      const remove = options.remove;
      if (remove) {
        const button = el("button", { className: "remove", type: "button", textContent: "Remove" });
        button.setAttribute("aria-label", `Remove ${person.name}`);
        button.addEventListener("click", () => {
          if (!confirmTap(button, `Remove ${person.name}? Tap again`)) return;
          button.disabled = true;
          remove(person)
            .catch(failed)
            .finally(() => {
              button.disabled = false;
            });
        });
        item.append(button);
      }
      return item;
    }),
  );
}

/** How many people `waitingLine` names before "and N others". */
const WAITING_NAMES = 5;

/**
 * The host's progress line: "18 of 25 picked · waiting on Ada, Bo and 3
 * others · 2 not connected". `done` says who has acted this round, as a
 * set of ids or a test; `people` is the roster, with whose phone is
 * connected. It never stops naming people: those whose phones are
 * connected come first, since they are the ones worth waiting for, up to
 * five, then a count of the rest, then how many of all those missing are
 * not connected, so a host can tell a slow room from a room of ghosts.
 * Nobody leaves the count on their own; the host removes them.
 */
export function waitingLine(
  done: ReadonlySet<number> | ((person: Present) => boolean),
  people: Present[],
  verb = "picked",
): string {
  const did = typeof done === "function" ? done : (p: Present) => done.has(p.id);
  const missing = people.filter((p) => !did(p));
  let line = `${people.length - missing.length} of ${plural(people.length, "person", "people")} ${verb}`;
  if (missing.length === 0) return line;
  // Connected first, in the order they joined; the sort is stable.
  const order = [...missing].sort((a, b) => Number(b.connected === true) - Number(a.connected === true));
  // "and 1 other" says no more than the name would.
  const named = order.length <= WAITING_NAMES + 1 ? order : order.slice(0, WAITING_NAMES);
  const others = order.length - named.length;
  line += ` · waiting on ${list([
    ...named.map((p) => p.name),
    ...(others ? [plural(others, "other", "others")] : []),
  ])}`;
  const off = missing.filter((p) => p.connected === false).length;
  if (off) line += ` · ${off} not connected`;
  return line;
}

/** "3 people, 2 connected", for above a roster. Where the room may not
 *  see who is connected, only how many are in. */
export function connectedCount(people: Present[]): string {
  if (people.length === 0) return "Nobody has joined yet";
  const all = plural(people.length, "person", "people");
  if (people.some((p) => p.connected === undefined)) return `${all} in`;
  const on = people.filter((p) => p.connected).length;
  return on === people.length ? `${all}, all connected` : `${all}, ${on} connected`;
}

/** The same line from a wall's counts, for when the names are not sent. */
function countLine(count: number | null, connected: number | null): string {
  if (count === null) return "";
  if (count === 0) return "Nobody has joined yet";
  const all = plural(count, "person", "people");
  if (connected === null) return `${all} in`;
  return connected === count ? `${all}, all connected` : `${all}, ${connected} connected`;
}

/**
 * A link to share: the link itself, a button that copies it, and a
 * button that shows it as a QR code. For the host who would otherwise
 * hand-roll all three.
 */
export function shareLink(where: string | HTMLElement, url: string): void {
  lobbyStyles();
  const box = target(where);
  if (box.dataset.url === url) return;
  box.dataset.url = url;
  box.classList.add("share");
  const link = el("a", { className: "link", href: url, textContent: url });
  const copy = el("button", { className: "button", type: "button", textContent: "Copy link" });
  copy.addEventListener("click", () => void copyTo(copy, url));
  const code = el("div", { className: "qr" });
  code.hidden = true;
  const toggle = el("button", { className: "button", type: "button", textContent: "Show QR code" });
  toggle.setAttribute("aria-expanded", "false");
  toggle.addEventListener("click", () => {
    code.hidden = !code.hidden;
    toggle.setAttribute("aria-expanded", String(!code.hidden));
    toggle.textContent = code.hidden ? "Show QR code" : "Hide QR code";
    if (!code.hidden && !code.firstChild) qrInto(code, url);
  });
  box.replaceChildren(link, el("div", { className: "actions" }, copy, toggle), code);
}

/** The wall's address, from the wall key the host's view carries. */
export function wallLink(wall: string): string {
  return `${location.origin}${BASE}/w/${wall}`;
}

/**
 * "Open the wall view" on the host's screen: the read-only page for a
 * projector, in a new tab. The wall key grants nothing, so the projector
 * can show its own address. Hidden inside the hall's frame, which has a
 * wall of its own.
 */
export function wallButton(where: string | HTMLElement, wall: string): void {
  lobbyStyles();
  const box = target(where);
  const url = wallLink(wall);
  if (box.dataset.url === url) return;
  box.dataset.url = url;
  const link = el("a", {
    className: "button wall-open",
    href: url,
    target: "_blank",
    rel: "noopener",
    textContent: "Open the wall view",
  });
  box.replaceChildren(link, el("p", { className: "note wall-open" }, "For a projector or a TV: the QR code, who is in, and what the game shows the room. Nothing on it can change the game."));
}

let wakeLock: WakeLockSentinel | null = null;
let wakeWanted = false;

/**
 * Keeps the screen on while the page is open: a phone that dims mid-round
 * drops its stream. Asked for again whenever the page comes back into
 * view, because browsers let go of it when it is hidden. Answers whether
 * the screen is being kept on; browsers without it carry on as before.
 */
export async function keepAwake(): Promise<boolean> {
  if (!wakeWanted) {
    wakeWanted = true;
    document.addEventListener("visibilitychange", () => void keepAwake());
  }
  try {
    if ("wakeLock" in navigator && document.visibilityState === "visible" && !wakeLock) {
      wakeLock = await navigator.wakeLock.request("screen");
      wakeLock.addEventListener("release", () => {
        wakeLock = null;
      });
    }
  } catch {
    /* not supported, or refused */
  }
  return wakeLock !== null;
}

/**
 * A "Reconnecting…" notice whenever `feed` has stopped hearing from the
 * server, so nobody answers into a screen that is out of date. Not shown
 * in the first few seconds, while the page is still connecting, nor once
 * the room is gone, when there is nothing to reconnect to.
 */
export function showConnection(feed: Feed, ms = 1000): void {
  lobbyStyles();
  const notice = el("p", { className: "reconnecting", textContent: "Reconnecting…" });
  notice.setAttribute("role", "status");
  notice.hidden = true;
  document.body.append(notice);
  const opened = Date.now();
  setInterval(() => {
    notice.hidden = feed.connected() || feed.gone() || Date.now() - opened < 4000;
    document.body.classList.toggle("stale", !notice.hidden);
  }, ms);
}

/** Above this many names the wall shows a count once the game has
 *  something to show, so that the game is not pushed off a projector. */
const WALL_NAMES = 12;

/**
 * The wall: the read-only screen for a projector. A large QR code and
 * the join link, who is in with a dot for each connected phone, and a
 * slot the game draws its public part into with `drawGame`, which is
 * also given the whole view (`view.people`, `view.busy`). The screen is
 * built here, so a page needs nothing for it but a `<main>`.
 *
 * What of the room is shown is the game's to say, on the server
 * (`HasLobby::on_wall`): the code and names may be hidden, and once the
 * game is busy, or has something to show to more than a dozen people, the
 * names give way to a count, the code shrinks to a corner, and the game
 * gets the screen.
 *
 * Something the game draws only to fill the slot while nothing is
 * happening, such as "Waiting for the host", gets the class `wall-idle`,
 * so that it does not count as something to make room for.
 *
 * Inside the hall's frame, the hall shows the code and the names itself,
 * so only the game's part is drawn.
 *
 *     if (where.kind === "wall") wall<MyWall>(where.key, (game, slot) => ...);
 */
export function wall<G>(
  key: string,
  drawGame?: (game: G, slot: HTMLElement, feed: Feed, view: WallView<G>) => void,
  options: { seat?: (seat: number) => string; missing?: string } = {},
): Feed {
  lobbyStyles();
  document.body.classList.add("wall-screen");
  const code = el("div", { className: "qr wall-qr" });
  const link = el("p", { className: "wall-link" });
  const count = el("p", { className: "eyebrow wall-count" });
  const people = el("ul", { className: "wall-people" });
  const slot = el("div", { className: "wall-game", id: "wall-game" });
  const join = el(
    "div",
    { className: "wall-join" },
    el("p", { className: "eyebrow" }, "Join on your phone"),
    code,
    link,
  );
  const frame = el("div", { className: "wall" }, join, el("div", { className: "wall-side" }, count, people, slot));
  const section = el("section", { id: "screen-wall" }, frame);
  document.querySelector("main")?.append(section);
  show("wall");
  void keepAwake();

  let shown = "";
  // The first view arrives after a request, so `feed` is set by then.
  const feed: Feed = watch<WallView<G>>(
    `/api/w/${encodeURIComponent(key)}`,
    (view) => {
      join.hidden = view.join === null;
      frame.classList.toggle("no-join", view.join === null);
      if (view.join !== null) {
        const url = joinLink(view.join);
        if (shown !== url) {
          shown = url;
          qrInto(code, url);
          link.textContent = url.replace(/^https?:\/\//, "");
        }
      }
      const line = view.people ? connectedCount(view.people) : countLine(view.count, view.connected);
      count.textContent = line;
      count.hidden = line === "";
      people.hidden = view.people === null;
      roster(people, view.people ?? [], { seat: options.seat });
      frame.classList.toggle("busy", view.busy);
      frame.classList.toggle("many", (view.people?.length ?? 0) > WALL_NAMES);
      if (drawGame) drawGame(view.game, slot, feed, view);
      // A slot holding only `.wall-idle` ("Waiting for the host") has
      // nothing the names would push off the screen.
      frame.classList.toggle(
        "playing",
        Array.from(slot.children).some((child) => !child.classList.contains("wall-idle")),
      );
    },
    options.missing,
  );
  showConnection(feed);
  return feed;
}
