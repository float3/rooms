// The browser half of a room: the parts every one of these tools needs.
//
// A page here holds an event stream open, redraws when the room changes,
// and posts what the person in front of it did. That is the whole of it,
// and it is the same in all three tools, so it lives here.

/** Everything the server sends is stamped with its own clock. */
export interface Envelope<V> {
  now: number;
  view: V;
}

/** The element with this id, which the page is expected to have. */
export function byId<T extends HTMLElement = HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (!found) throw new Error(`the page has no #${id}`);
  return found as T;
}

/** Text going into markup. Names and questions come from the room. */
export function esc(text: unknown): string {
  return String(text).replace(
    /[&<>"]/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] ?? c,
  );
}

/** A number as a person would write it, at whatever size it comes in. */
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

/** A refusal, with the status so that a missing room can be told apart. */
export class Refused extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = "Refused";
    this.status = status;
  }
}

/**
 * A request to the room, with the server's refusal as the error message.
 * Every endpoint answers in JSON, including when it says no.
 */
export async function api<T>(method: string, path: string, body?: unknown): Promise<T> {
  const response = await fetch(path, {
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

/** How long to wait before trying the room again. */
const RETRY_MS = 2000;
/** With nothing heard for this long, the page is out of date. */
const SILENT_MS = 8000;

/** What a watcher can ask about the room it is watching. */
export interface Feed {
  /** The server's clock, which is the one every countdown is run off. */
  now: () => number;
  /** Whether the view on screen is still being kept up to date. */
  connected: () => boolean;
  /** Takes a view that came back from a request, so a screen that acted
   *  does not wait for its own event to come round. */
  accept: (envelope: Envelope<unknown>) => void;
}

/**
 * Holds a room's event stream open and hands every view to `onView`.
 *
 * The first view comes from `snapshot`, so the page fills immediately;
 * after that they come down `events`. The browser reconnects on its own,
 * and this reconnects when the browser gives up. A view that has not
 * changed is not redrawn.
 *
 * Phones and the server disagree about the time by enough to matter when
 * something is counting down, so every view is stamped with the server's
 * clock and `now()` works from the difference.
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
    const source = new EventSource(events);
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

/** The common case: one room, one view, and a line when it goes away. */
export function watch<V>(
  path: string,
  draw: (view: V) => void,
  missing = "That room is not here. It may have finished, or the link may be wrong.",
): Feed {
  return subscribe<V>(path, `${path}/events`, draw, () => fail(missing));
}

/** Asks the server for the link as a QR code and puts it in the page. */
export function qrInto(id: string, link: string): void {
  fetch("/api/qr", { method: "POST", body: link })
    .then((response) => (response.ok ? response.text() : ""))
    .then((svg) => {
      byId(id).innerHTML = svg;
    })
    .catch(() => {
      /* the link is written out above it either way */
    });
}

/** A whole number of two digits, for a clock. */
function pad(n: number): string {
  return String(n).padStart(2, "0");
}

/** Milliseconds as a clock counting down: 9:05, or 1:09:05 past an hour. */
export function countdown(ms: number): string {
  const seconds = Math.max(0, Math.ceil(ms / 1000));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor(seconds / 60) % 60;
  return hours
    ? `${hours}:${pad(minutes)}:${pad(seconds % 60)}`
    : `${minutes}:${pad(seconds % 60)}`;
}

/** A moment as the clock on the wall said it. */
export function timeOfDay(at: number): string {
  return new Date(at).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/**
 * An element, its properties, and its children. Building the page out of
 * these rather than out of strings keeps text that came from the room
 * from ever being read as markup.
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
 * Runs `build` only when `key` is not what it was last time. Redrawing a
 * list on every frame throws away the scroll position and whatever was
 * half-selected in it.
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

/** Something this phone kept, or the fallback if it kept nothing. */
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
    /* a browser with storage turned off still plays */
  }
}

/** Puts a link on the clipboard and says so on the button that asked. */
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

export type Kind = "host" | "join" | "player" | "start";

/** Which of the three links this page was opened with. */
export function route(): { kind: Kind; key: string } {
  const parts = location.pathname.split("/").filter((part) => part !== "");
  const kinds: Record<string, Kind> = { h: "host", j: "join", p: "player" };
  if (parts.length === 2 && kinds[parts[0]]) {
    return { kind: kinds[parts[0]], key: parts[1] };
  }
  return { kind: "start", key: "" };
}

/**
 * What this phone is in the room. Kept so that closing the tab and opening
 * the join link again walks back in as the same person.
 */
export function remembered(ns: string, key: string): string | null {
  try {
    return localStorage.getItem(`${ns}:${key}`);
  } catch {
    return null; // a browser with storage turned off still plays
  }
}

export function remember(ns: string, key: string, value: string): void {
  try {
    localStorage.setItem(`${ns}:${key}`, value);
  } catch {
    /* nothing to do: the link in the address bar is the fallback */
  }
}

/** The link to put on the wall, from the key the room gave back. */
export function joinLink(join: string): string {
  return `${location.origin}/j/${join}`;
}

/** Wires a form up to a handler that may refuse. */
export function onSubmit(id: string, run: () => Promise<void>): void {
  byId<HTMLFormElement>(id).addEventListener("submit", (event) => {
    event.preventDefault();
    run().catch((e: unknown) => fail(e instanceof Error ? e.message : String(e)));
  });
}

/** The same for a button. */
export function onClick(id: string, run: () => Promise<void>): void {
  byId(id).addEventListener("click", () => {
    run().catch((e: unknown) => fail(e instanceof Error ? e.message : String(e)));
  });
}

/** A number field's value, or null when it is empty or not a number. */
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
