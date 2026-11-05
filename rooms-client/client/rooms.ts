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

  const accept = (envelope: Envelope<unknown>): void => {
    offset = envelope.now - Date.now();
    heard = Date.now();
    const view = JSON.stringify(envelope.view);
    if (view === last) return;
    last = view;
    onView(envelope.view as V);
  };

  const connect = async (): Promise<void> => {
    try {
      accept(await api<Envelope<V>>("GET", snapshot));
    } catch (e) {
      if (e instanceof Refused && e.status === 404) {
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
      heard > 0 &&
      Date.now() - heard < SILENT_MS &&
      !(lostAt !== null && Date.now() - lostAt > RETRY_MS),
    accept,
  };
}

/** One room, one view, and a message when the room is gone.
 *
 *  `path` is the room's own address, `/api/h/<key>`: the event stream is
 *  found by adding `/events` to it. Passing the stream's address instead
 *  would listen on `/events/events` and the page would never update, so
 *  that is caught here. */
export function watch<V>(
  path: string,
  draw: (view: V) => void,
  missing = "That room is not here. It may have finished, or the link may be wrong.",
): Feed {
  const room = path.replace(/\/events$/, "");
  return subscribe<V>(room, `${room}/events`, draw, () => fail(missing));
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

/** The join screen: how many are in already, and a name to join with.
 *  A phone that has joined this room before goes straight back to its
 *  own link, which is kept under `ns`, the tool's name. */
export function joining(ns: string, key: string): void {
  const kept = remembered(ns, key);
  if (kept) {
    // Back to the link this phone was given, unless the host has taken
    // it out of the room since, in which case it joins afresh.
    api<unknown>("GET", `/api/p/${encodeURIComponent(kept)}`)
      .then(() => location.replace(here(`/p/${kept}`)))
      .catch((e: unknown) => {
        if (e instanceof Refused && e.status === 404) {
          forget(ns, key);
          askName(ns, key);
        } else {
          location.replace(here(`/p/${kept}`));
        }
      });
    return;
  }
  askName(ns, key);
}

function askName(ns: string, key: string): void {
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
      location.href = here(`/p/${body.player}`);
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
  name: string;
  connected: boolean;
  /** The server's clock when their phone was last connected. */
  seen: number;
  /** Where the game seated them: a team, a side, a country. */
  seat?: number;
}

/** What `/api/w/<key>` sends: the join key for the QR code, everybody,
 *  and the game's own public part, whatever the game puts there. */
export interface WallView<G> {
  join: string;
  people: Present[];
  game: G;
}

/** An element, or the id of one. */
function target(where: string | HTMLElement): HTMLElement {
  return typeof where === "string" ? byId(where) : where;
}

/**
 * A press that cannot be undone asks again first. The first press turns
 * the button into the question for a few seconds; a second press within
 * them answers true. No dialog, so it works the same in the hall's frame
 * and on a phone held in one hand.
 *
 *     if (!confirmTap(button, "Tap again to remove Ada")) return;
 */
export function confirmTap(button: HTMLElement, ask: string, ms = 4000): boolean {
  if (button.dataset.armed === "1") {
    delete button.dataset.armed;
    button.classList.remove("armed");
    if (button.dataset.said !== undefined) button.textContent = button.dataset.said;
    return true;
  }
  button.dataset.armed = "1";
  button.dataset.said = button.textContent ?? "";
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
 * The list of who is in, with a dot for whose phone is connected. Give
 * `remove` on the host's screen and each name gets a button that asks
 * once more and then takes them out. `seat` names a seat ("Country A").
 * Redrawn only when something in it changed, so a button that is asking
 * keeps asking.
 */
export function roster(
  where: string | HTMLElement,
  people: Present[],
  options: { remove?: (name: string) => Promise<unknown>; seat?: (seat: number) => string } = {},
): void {
  const box = target(where);
  const shape = JSON.stringify(people.map((p) => [p.name, p.connected, p.seat]));
  if (box.dataset.shape === shape) return;
  box.dataset.shape = shape;
  box.classList.add("roster");
  box.replaceChildren(
    ...people.map((person) => {
      const dot = el("span", { className: `dot ${person.connected ? "on" : "off"}` });
      dot.setAttribute("aria-hidden", "true");
      const state = el("span", { className: "visually-hidden" }, person.connected ? " (connected)" : " (not connected)");
      const seat =
        person.seat !== undefined && options.seat
          ? el("span", { className: "seat" }, options.seat(person.seat))
          : null;
      const item = el("li", { className: person.connected ? "" : "away" }, dot, el("span", { className: "who" }, person.name), state, seat);
      const remove = options.remove;
      if (remove) {
        const button = el("button", { className: "remove", type: "button", textContent: "Remove" });
        button.setAttribute("aria-label", `Remove ${person.name}`);
        button.addEventListener("click", () => {
          if (!confirmTap(button, `Remove ${person.name}?`)) return;
          button.disabled = true;
          remove(person.name)
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

/** "3 of 4 connected", for above a roster. */
export function connectedCount(people: Present[]): string {
  if (people.length === 0) return "Nobody has joined yet";
  const on = people.filter((p) => p.connected).length;
  return on === people.length
    ? `${plural(people.length, "person", "people")}, all connected`
    : `${plural(people.length, "person", "people")}, ${on} connected`;
}

/**
 * A link to share: the link itself, a button that copies it, and a
 * button that shows it as a QR code. For the host who would otherwise
 * hand-roll all three.
 */
export function shareLink(where: string | HTMLElement, url: string): void {
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
 * in the first few seconds, while the page is still connecting.
 */
export function showConnection(feed: Feed, ms = 1000): void {
  const notice = el("p", { className: "reconnecting", textContent: "Reconnecting…" });
  notice.setAttribute("role", "status");
  notice.hidden = true;
  document.body.append(notice);
  const opened = Date.now();
  setInterval(() => {
    notice.hidden = feed.connected() || Date.now() - opened < 4000;
    document.body.classList.toggle("stale", !notice.hidden);
  }, ms);
}

/**
 * The wall: the read-only screen for a projector. A large QR code and
 * the join link, who is in with a dot for each connected phone, and a
 * slot the game draws its public part into with `drawGame`. The screen
 * is built here, so a page needs nothing for it but a `<main>`.
 *
 * Inside the hall's frame, the hall shows the code and the names itself,
 * so only the game's part is drawn.
 *
 *     if (where.kind === "wall") wall<MyWall>(where.key, (game, slot) => ...);
 */
export function wall<G>(
  key: string,
  drawGame?: (game: G, slot: HTMLElement, feed: Feed) => void,
  options: { seat?: (seat: number) => string } = {},
): Feed {
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
  const section = el(
    "section",
    { id: "screen-wall" },
    el("div", { className: "wall" }, join, el("div", { className: "wall-side" }, count, people, slot)),
  );
  document.querySelector("main")?.append(section);
  show("wall");
  void keepAwake();

  let shown = "";
  // The first view arrives after a request, so `feed` is set by then.
  const feed: Feed = watch<WallView<G>>(`/api/w/${encodeURIComponent(key)}`, (view) => {
    const url = joinLink(view.join);
    if (shown !== url) {
      shown = url;
      qrInto(code, url);
      link.textContent = url.replace(/^https?:\/\//, "");
    }
    count.textContent = connectedCount(view.people);
    roster(people, view.people, { seat: options.seat });
    if (drawGame) drawGame(view.game, slot, feed);
  });
  showConnection(feed);
  return feed;
}
