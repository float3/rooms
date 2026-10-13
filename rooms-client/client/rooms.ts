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
    throw new Error(said || "Something went wrong. Try again.");
  }
  return data as T;
}

/**
 * Holds the room's event stream open and hands every view to `draw`.
 *
 * The browser reconnects on its own, so an error only matters once it has
 * given up. A view that will not draw means the room is gone.
 */
export function watch<V>(path: string, draw: (view: V) => void): EventSource {
  const stream = new EventSource(path);
  stream.onmessage = (event: MessageEvent<string>) => {
    let envelope: Envelope<V>;
    try {
      envelope = JSON.parse(event.data) as Envelope<V>;
    } catch {
      return;
    }
    draw(envelope.view);
    fail("");
  };
  stream.onerror = () => {
    if (stream.readyState === EventSource.CLOSED) {
      fail("Lost the connection. Reload the page.");
    }
  };
  return stream;
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
