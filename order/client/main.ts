// One food order, in a browser.
//
// The host's screen says where the food is from and gathers the whole
// order. A player's screen, almost always a phone, opens the menu and
// adds what they want. The wall shows where from and how far along.

import {
  type Feed,
  type Present,
  type WallView,
  api,
  byId,
  confirmTap,
  connectedCount,
  copyTo,
  el,
  fail,
  hide,
  joinLink,
  joining,
  keepAwake,
  landed,
  list,
  onClick,
  onSubmit,
  opening,
  plural,
  qrInto,
  roster,
  route,
  shareLink,
  show,
  showConnection,
  text,
  value,
  wall,
  wallButton,
  watch,
} from "./rooms";

interface Line {
  id: number;
  what: string;
  /** In cents. */
  price: number | null;
}

interface Share {
  id: number;
  name: string;
  items: Line[];
  total: number;
  unpriced: boolean;
  paid: boolean;
}

interface Tally {
  what: string;
  count: number;
  names: string[];
}

interface Place {
  place: string;
  menu: string;
  note: string;
  open: boolean;
}

interface HostView extends Place {
  join: string;
  wall: string;
  people: Present[];
  away: number[];
  shares: Share[];
  waiting: string[];
  tally: Tally[];
  items: number;
  total: number;
  unpriced: number;
}

interface PlayerView extends Place {
  name: string;
  mine: Line[];
  total: number;
  paid: boolean;
  others: Share[];
  people: number;
  ordered: number;
}

interface OrderWall extends Place {
  ordered: number;
  items: number;
}

/** Cents as euros: "9,50 €" or "€9.50", as the phone writes money. */
function euros(cents: number): string {
  return (cents / 100).toLocaleString(undefined, { style: "currency", currency: "EUR" });
}

/** The server only keeps web links, but a link is never drawn unless it
 *  is one. */
function safe(link: string): boolean {
  return /^https?:\/\//i.test(link);
}

/** "Open the menu", in a new tab, or a line saying there is none yet. */
function menuLine(target: string, view: Place): void {
  const box = byId(target);
  if (safe(view.menu)) {
    const link = el("a", {
      className: "button menu-button",
      href: view.menu,
      target: "_blank",
      rel: "noopener noreferrer",
      textContent: "Open the menu",
    });
    const shown = view.menu.replace(/^https?:\/\//i, "").replace(/\/$/, "");
    box.replaceChildren(link, el("span", { className: "note menu-url" }, shown));
  } else {
    box.replaceChildren(el("span", { className: "note" }, "No menu link yet."));
  }
}

/** The order as text, for pasting into a chat or reading out on the phone. */
function orderText(view: HostView): string {
  const lines = [`Order${view.place ? ` from ${view.place}` : ""}:`];
  for (const t of view.tally) lines.push(`${t.count}× ${t.what}`);
  lines.push("", `${plural(view.items, "item", "items")}${view.total ? `, ${euros(view.total)}` : ""}`);
  if (view.unpriced) lines.push(`(${plural(view.unpriced, "item has", "items have")} no price)`);
  lines.push("", "Who ordered what:");
  for (const share of view.shares) {
    const items = share.items.map((i) => i.what).join(", ");
    lines.push(`${share.name}: ${items}${share.total ? ` (${euros(share.total)})` : ""}`);
  }
  return lines.join("\n");
}

function priced(line: Line): string {
  return line.price === null ? "" : euros(line.price);
}

// ------------------------------------------------------------------- host

function host(key: string): void {
  show("host");
  const at = (path: string) => `/api/h/${encodeURIComponent(key)}${path}`;
  let last: HostView | null = null;
  text("where", "You are the host");

  const place = byId<HTMLInputElement>("place");
  const menu = byId<HTMLInputElement>("menu");
  const note = byId<HTMLTextAreaElement>("note");
  /** The form is filled from the room once, and again only when the room
   *  changes underneath it and the host is not typing in it. */
  let filledFor = "";
  let typing = false;
  for (const field of [place, menu, note]) {
    field.addEventListener("input", () => {
      typing = true;
    });
  }

  onSubmit("place-form", async () => {
    const body = await api<{ view: HostView }>("PUT", at("/place"), {
      place: place.value,
      menu: menu.value,
      note: note.value,
    });
    typing = false;
    filledFor = "";
    fail("");
    draw(body.view);
    if (body.view.place || body.view.menu) byId<HTMLDetailsElement>("about").open = false;
  });

  onClick("toggle", async () => {
    if (!last) return;
    const body = await api<{ view: HostView }>("PUT", at("/open"), { open: !last.open });
    fail("");
    draw(body.view);
  });

  byId("copy").addEventListener("click", () => {
    if (last) void copyTo(byId("copy"), orderText(last));
  });

  const remove = (person: Present) => api("POST", at("/remove"), { id: person.id });

  function drawForm(view: HostView): void {
    const shape = JSON.stringify([view.place, view.menu, view.note]);
    if (shape === filledFor || typing) return;
    const first = filledFor === "";
    filledFor = shape;
    place.value = view.place;
    menu.value = view.menu;
    note.value = view.note;
    text("about-title", view.place || view.menu ? "Change where from" : "Where from");
    // A fresh order starts with the form open; one already described
    // starts folded, so the order itself is what the host sees.
    if (first) byId<HTMLDetailsElement>("about").open = !(view.place || view.menu);
  }

  function drawSummary(view: HostView): void {
    text("h-state", view.open ? "Taking orders" : "The order is closed");
    text("h-place", view.place || "Somewhere to order from");
    menuLine("h-menu", view);
    const ordered = view.shares.length;
    const everybody = ordered + view.waiting.length;
    const parts = [
      everybody === 0 ? "Nobody has joined yet" : `${ordered} of ${plural(everybody, "person has", "people have")} ordered`,
    ];
    if (view.waiting.length && ordered) {
      parts.push(view.waiting.length <= 4 ? `waiting on ${list(view.waiting)}` : `${view.waiting.length} still to order`);
    }
    if (view.items) parts.push(`${plural(view.items, "item", "items")}${view.total ? `, ${euros(view.total)}` : ""}`);
    if (view.unpriced && view.total) parts.push(`${view.unpriced} without a price`);
    text("progress", parts.join(" · "));
    byId("toggle").textContent = view.open ? "Close the order" : "Open it again";
    byId<HTMLButtonElement>("copy").disabled = view.items === 0;

    const tally = byId("tally");
    if (view.tally.length === 0) {
      tally.replaceChildren();
      return;
    }
    tally.replaceChildren(
      el("p", { className: "eyebrow tally-title" }, "To order"),
      el(
        "ul",
        { className: "tally" },
        ...view.tally.map((t) =>
          el(
            "li",
            {},
            el("b", { className: "count" }, `${t.count}×`),
            el("span", { className: "what" }, t.what),
            el("span", { className: "note who" }, list(t.names)),
          ),
        ),
      ),
    );
  }

  function drawShares(view: HostView): void {
    const box = byId("shares");
    if (view.shares.length === 0) {
      box.replaceChildren();
      return;
    }
    box.replaceChildren(
      el("h2", { className: "shares-title" }, "Who ordered what"),
      ...view.shares.map((share) => {
        const paid = el("button", {
          className: `button paid${share.paid ? " on" : ""}`,
          type: "button",
          textContent: share.paid ? "Paid ✓" : "Mark paid",
        });
        paid.setAttribute("aria-pressed", String(share.paid));
        paid.addEventListener("click", () => {
          api<{ view: HostView }>("PUT", at("/paid"), { id: share.id, paid: !share.paid })
            .then((body) => draw(body.view))
            .catch((e: unknown) => fail(e instanceof Error ? e.message : String(e)));
        });
        const total = share.total
          ? `${euros(share.total)}${share.unpriced ? " + unpriced" : ""}`
          : share.unpriced
            ? "no prices"
            : "";
        return el(
          "div",
          { className: `panel share${share.paid ? " is-paid" : ""}` },
          el("div", { className: "share-head" }, el("b", {}, share.name), el("span", { className: "note" }, total), paid),
          el(
            "ul",
            { className: "items" },
            ...share.items.map((item) => {
              const strike = el("button", {
                className: "button strike",
                type: "button",
                textContent: "×",
                title: "Take this off the order",
              });
              strike.setAttribute("aria-label", `Take ${item.what} off ${share.name}'s order`);
              strike.addEventListener("click", () => {
                if (!confirmTap(strike, "Remove?", 4000)) return;
                api<{ view: HostView }>("POST", at("/strike"), { item: item.id })
                  .then((body) => draw(body.view))
                  .catch((e: unknown) => fail(e instanceof Error ? e.message : String(e)));
              });
              return el(
                "li",
                {},
                el("span", { className: "what" }, item.what),
                el("span", { className: "price" }, priced(item)),
                strike,
              );
            }),
          ),
        );
      }),
    );
  }

  function draw(view: HostView): void {
    last = view;
    shareLink("share", joinLink(view.join));
    text("who", connectedCount(view.people) + (view.away.length ? ` · ${view.away.length} away` : ""));
    roster("people", view.people, {
      remove,
      away: { ids: view.away, remove: (ids) => api("POST", at("/remove-away"), { ids }) },
    });
    wallButton("wall-button", view.wall);
    drawForm(view);
    drawSummary(view);
    drawShares(view);
  }

  showConnection(watch<HostView>(at(""), draw));
  void keepAwake();
}

// ----------------------------------------------------------------- player

function player(key: string): void {
  show("player");
  const at = (path: string) => `/api/p/${encodeURIComponent(key)}${path}`;
  const what = byId<HTMLInputElement>("what");
  let sending = false;

  onSubmit("add-form", async () => {
    if (sending) return;
    if (!what.value.trim()) {
      what.focus();
      return;
    }
    sending = true;
    try {
      const body = await api<{ view: PlayerView }>("POST", at("/items"), {
        what: what.value,
        price: value("price"),
      });
      what.value = "";
      byId<HTMLInputElement>("price").value = "";
      fail("");
      draw(body.view);
      landed("mine");
    } finally {
      sending = false;
    }
  });

  function drawMine(view: PlayerView): void {
    const box = byId("mine");
    box.replaceChildren(
      ...view.mine.map((item) => {
        const parts: (Node | string)[] = [
          el("span", { className: "what" }, item.what),
          el("span", { className: "price" }, priced(item)),
        ];
        if (view.open) {
          const off = el("button", { className: "button strike", type: "button", textContent: "×" });
          off.setAttribute("aria-label", `Take ${item.what} off your order`);
          off.addEventListener("click", () => {
            off.disabled = true;
            api<{ view: PlayerView }>("POST", at("/items/remove"), { item: item.id })
              .then((body) => {
                fail("");
                draw(body.view);
              })
              .catch((e: unknown) => {
                off.disabled = false;
                fail(e instanceof Error ? e.message : String(e));
              });
          });
          parts.push(off);
        }
        return el("li", {}, ...parts);
      }),
    );
    box.hidden = view.mine.length === 0;
    const said =
      view.mine.length === 0
        ? view.open
          ? "You have not added anything yet."
          : "You did not order anything."
        : `${plural(view.mine.length, "item", "items")}${view.total ? `, ${euros(view.total)}` : ""}${view.paid ? " · paid ✓" : ""}`;
    text("mine-total", said);
  }

  function drawOthers(view: PlayerView): void {
    const box = byId("others");
    text(
      "others-title",
      view.others.length === 0
        ? "Nobody else has ordered yet"
        : `What everybody else is having (${view.others.length})`,
    );
    box.replaceChildren(
      el(
        "ul",
        { className: "others" },
        ...view.others.map((share) =>
          el("li", {}, el("b", {}, share.name), " ", el("span", {}, share.items.map((i) => i.what).join(", "))),
        ),
      ),
    );
  }

  function draw(view: PlayerView): void {
    text("you", `You are ${view.name}`);
    text("where", view.name);
    text(
      "p-state",
      view.open
        ? `Taking orders · ${view.ordered} of ${plural(view.people, "person has", "people have")} ordered`
        : "The order has gone in",
    );
    text("p-place", view.place || "Waiting for the host to say where from");
    menuLine("p-menu", view);
    hide("p-note", view.note === "");
    text("p-note", view.note);
    hide("add-form", !view.open);
    drawMine(view);
    drawOthers(view);
  }

  showConnection(
    watch<PlayerView>(
      at(""),
      draw,
      "You are not in this order. The host may have taken you out, or it has finished. Scan the code again to rejoin.",
    ),
  );
  void keepAwake();
}

// ------------------------------------------------------------------- wall

let wallMenu = "";

function drawWall(game: OrderWall | null, slot: HTMLElement, _feed: Feed, view: WallView<OrderWall | null>): void {
  if (!game) {
    slot.replaceChildren();
    return;
  }
  const everybody = view.count ?? view.people?.length ?? 0;
  const parts: HTMLElement[] = [
    el("p", { className: "eyebrow" }, game.open ? "Taking orders" : "The order has gone in"),
    el("h2", { className: "ow-place" }, game.place || "Food order"),
    el("p", { className: "ow-count" }, el("b", {}, `${game.ordered} of ${everybody}`), " have ordered"),
  ];
  if (game.note) parts.push(el("p", { className: "note ow-note" }, game.note));
  if (safe(game.menu)) {
    // The menu's own QR code, so people can open it without typing.
    const code = el("div", { className: "qr ow-menu-qr" });
    parts.push(el("div", { className: "ow-menu" }, el("p", { className: "eyebrow" }, "The menu"), code));
    if (wallMenu !== game.menu || !slot.querySelector(".ow-menu-qr svg")) {
      wallMenu = game.menu;
      qrInto(code, game.menu);
    } else {
      const old = slot.querySelector(".ow-menu-qr");
      if (old) code.replaceChildren(...Array.from(old.childNodes));
    }
  }
  slot.replaceChildren(...parts);
}

const where = route();
if (where.kind === "host") host(where.key);
else if (where.kind === "join") joining("order", where.key);
else if (where.kind === "player") player(where.key);
else if (where.kind === "wall") wall<OrderWall | null>(where.key, drawWall);
else opening();
