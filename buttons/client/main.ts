// Red or blue, in a browser.
//
// One page with four screens: the one that opens a room, the host's, the
// player's, which is almost always a phone, and the wall, which is a
// projector.

import {
  type Feed,
  type Present,
  type WallView,
  api,
  byId,
  confirmTap,
  connectedCount,
  el,
  esc,
  fail,
  hide,
  html,
  joinLink,
  joining,
  keepAwake,
  landed,
  onClick,
  onSubmit,
  opening,
  plural,
  roster,
  route,
  sentState,
  shareLink,
  show,
  showConnection,
  text,
  value,
  waitingLine,
  wall,
  wallButton,
  watch,
} from "./rooms";

type Colour = "red" | "blue";

interface Playing {
  /** The share of the presses blue has to beat, in percent. */
  percent: number;
  open: boolean;
  revealed: boolean;
  asked: number;
}

interface Pressed {
  name: string;
  colour: Colour;
  lived: boolean;
}

/** How a round split. Whoever pressed nothing is not in `n`. */
interface Split {
  n: number;
  blue: number;
  /** Null when nobody pressed. */
  blue_won: boolean | null;
}

interface Outcome extends Split {
  percent: number;
  presses: Pressed[];
}

interface Past extends Split {
  percent: number;
}

interface Who {
  id: number;
  name: string;
  pressed: boolean;
}

interface Score {
  name: string;
  played: number;
  lived: number;
  blue: number;
}

interface HostView {
  join: string;
  /** The wall's key, for "Open the wall view". */
  wall: string;
  /** Everybody, with whose phone is connected. */
  people: Present[];
  /** Everybody, with whether they have pressed this round. */
  players: Who[];
  /** Not connected, and no press in this round or the one before. */
  away: number[];
  round: Playing | null;
  outcome: Outcome | null;
  history: Past[];
  scores: Score[];
}

interface PlayerView {
  name: string;
  round: Playing | null;
  mine: Colour | null;
  pressed: number;
  players: number;
  outcome: Outcome | null;
  lived: boolean | null;
  score: Score | null;
}

/** The game's part of the wall: the split, and never who. */
interface ButtonsWall {
  round: Playing | null;
  pressed: number;
  players: number;
  shown: Split | null;
}

const BUTTONS: readonly (readonly [string, Colour])[] = [
  ["press-red", "red"],
  ["press-blue", "blue"],
];

/** The rule of a round, as every screen says it. */
const rule = (percent: number): string =>
  `If more than ${percent}% press blue, everybody lives. If not, only red does.`;

/** A share as a percentage: "60%", or "33.3%" when it is not whole. */
const share = (part: number): string => `${Number((part * 100).toFixed(1))}%`;

/** "Blue got 3 of 5, 60%, more than the 50% it needed". */
const splitLine = (percent: number, split: Split): string =>
  `Blue got ${split.blue} of ${split.n}, ${share(split.blue / split.n)}, ${
    split.blue_won ? "more than" : "not more than"
  } the ${percent}% it needed`;

/** The share that pressed blue, against the line it had to pass. */
function splitBar(percent: number, split: Split): string {
  return `<div class="split-bar"><div class="track"><div class="blue" style="width:${(split.blue / split.n) * 100}%"></div><div class="red"></div></div>
    <div class="line" style="left:${percent}%"><span>${percent}%</span></div></div>
    <div class="ends"><span>${split.blue} blue</span><span>${split.n - split.blue} red</span></div>`;
}

/** Who lived, or that nobody pressed, as the first thing the reveal says. */
function verdict(percent: number, split: Split): string {
  if (split.n === 0) {
    return `<div class="verdict none"><p class="eyebrow">Nobody pressed</p>
      <p class="big">Nobody pressed a button this round.</p></div>`;
  }
  return `<div class="verdict ${split.blue_won ? "won" : "none"}"><p class="eyebrow">${
    split.blue_won ? "Everybody lives" : "Only red lives"
  }</p><p class="big">${esc(splitLine(percent, split))}</p></div>`;
}

function outcomeHtml(outcome: Outcome): string {
  if (outcome.n === 0) return verdict(outcome.percent, outcome);
  return `${verdict(outcome.percent, outcome)}${splitBar(outcome.percent, outcome)}
    <div class="scroll"><table><thead><tr><th>Who</th><th>Pressed</th></tr></thead>
      <tbody>${outcome.presses
        .map(
          (pressed) => `<tr class="${pressed.lived ? "lived" : ""}">
        <td>${pressed.lived ? "" : "💀 "}${esc(pressed.name)}</td><td><span class="colour ${pressed.colour}">${pressed.colour}</span></td>
      </tr>`,
        )
        .join("")}</tbody></table></div>`;
}


// ------------------------------------------------------------------- host

function host(key: string): void {
  show("host");
  const at = (path: string) => `/api/h/${encodeURIComponent(key)}${path}`;
  let now: HostView | null = null;

  text("where", "You are the host");

  onSubmit("start-form", async () => {
    const percent = value("percent");
    if (percent === null || !Number.isInteger(percent) || percent < 0 || percent > 99) {
      fail("The share has to be a whole number from 0 to 99.");
      return;
    }
    // Starting a round closes the one before it without revealing it, so
    // its presses would never be shown. Ask, but only when there are some.
    // A second tap rather than confirm(), which the hall's frame may not
    // show.
    const current = now?.round;
    if (
      current &&
      !current.revealed &&
      now?.players.some((who) => who.pressed) &&
      !confirmTap(byId("start"), "Nobody has seen these presses. Tap again to drop them")
    ) {
      return;
    }
    await api("POST", at("/start"), { percent });
    fail("");
  });

  onClick("close", async () => {
    await api("PUT", at("/open"), { open: !now?.round?.open });
    fail("");
  });

  onClick("reveal", async () => {
    await api("POST", at("/reveal"));
    fail("");
  });

  onClick("again", async () => {
    await api("POST", at("/again"));
    fail("");
  });

  const remove = (person: Present) => api("POST", at("/remove"), { id: person.id });

  function drawRoom(view: HostView): void {
    shareLink("share", joinLink(view.join));
    // The list folds away once rounds start; the count of who is away
    // stays in sight, so the host knows to open it.
    text(
      "who",
      connectedCount(view.people) + (view.away.length ? ` · ${view.away.length} away` : ""),
    );
    roster("people", view.people, {
      remove,
      away: { ids: view.away, remove: (ids) => api("POST", at("/remove-away"), { ids }) },
    });
    wallButton("wall-button", view.wall);
  }

  /** While a round is being played, the form for the next one is folded
   *  away under it, so Reveal is the obvious thing to press. It opens
   *  again once the round has come out. Only on a change of round, so a
   *  host who opened it keeps it open. */
  let foldedFor = "";
  function fold(round: Playing | null): void {
    const playing = round !== null && !round.revealed;
    const shape = round === null ? "none" : `${round.asked}:${round.revealed}`;
    text("next-title", round === null ? "Start a round" : "Start the next round");
    if (shape === foldedFor) return;
    foldedFor = shape;
    byId<HTMLDetailsElement>("next").open = !playing;
    // Who is in is folded too once rounds are being played: on a phone,
    // a long list would push Reveal off the screen.
    byId<HTMLDetailsElement>("in-room").open = round === null;
  }

  function drawBoard(view: HostView): void {
    if (view.scores.length === 0) {
      html("board", "");
      return;
    }
    html(
      "board",
      `<h3>The evening so far</h3><div class="scroll"><table><thead><tr>
        <th>Who</th><th>Rounds</th><th>Lived</th><th>Pressed blue</th>
      </tr></thead><tbody>${view.scores
        .map(
          (score) => `<tr>
        <td>${esc(score.name)}</td><td>${score.played}</td><td>${score.lived}</td><td>${score.blue}</td>
      </tr>`,
        )
        .join("")}</tbody></table></div>`,
    );
  }

  function drawHistory(view: HostView): void {
    if (view.history.length === 0) {
      html("history", "");
      return;
    }
    html(
      "history",
      `<h3>Played already</h3>${view.history
        .map(
          (past) => `<div class="past">
        <span>Blue needed more than ${past.percent}%</span>
        <span>${
          past.n === 0
            ? "nobody pressed"
            : `${past.blue} of ${past.n} blue · ${past.blue_won ? "everybody lived" : "only red lived"}`
        }</span>
      </div>`,
        )
        .join("")}`,
    );
  }

  function draw(view: HostView): void {
    now = view;
    drawRoom(view);
    const round = view.round;
    fold(round);
    hide("playing", round === null);
    if (round) {
      text("h-state", round.revealed ? "Revealed" : round.open ? "Taking presses" : "Closed");
      text("h-rule", rule(round.percent));
      const pressed = new Set(view.players.filter((who) => who.pressed).map((who) => who.id));
      text(
        "progress",
        round.revealed
          ? `${pressed.size} of ${plural(view.players.length, "person", "people")} pressed`
          : `${waitingLine(pressed, view.people, "pressed")} · nobody sees a press until you reveal`,
      );
      text("close", round.open ? "Stop taking presses" : "Take more presses");
      // Once the round is out, the next thing is often the same share
      // again, to see what the room does now it knows.
      hide("close", round.revealed);
      hide("reveal", round.revealed);
      hide("again", !round.revealed);
      html("outcome", view.outcome ? outcomeHtml(view.outcome) : "");
    }
    drawBoard(view);
    drawHistory(view);
  }

  // Fetches the room once, then keeps it up to date from its event
  // stream; `watch` adds the "/events" itself.
  showConnection(watch<HostView>(at(""), draw));
  void keepAwake();
}


// ----------------------------------------------------------------- player

function player(key: string): void {
  show("player");
  const at = (path: string) => `/api/p/${encodeURIComponent(key)}${path}`;
  let last: PlayerView | null = null;
  let sending = false;

  /** Which button is in, and whether it can still be changed. */
  function drawStatus(): void {
    const view = last;
    const round = view?.round;
    if (!view || !round || round.revealed) {
      sentState("status", null);
      return;
    }
    for (const [id, colour] of BUTTONS) {
      const button = byId<HTMLButtonElement>(id);
      button.classList.toggle("chosen", view.mine === colour);
      button.disabled = sending;
    }
    if (!round.open) {
      if (view.mine === null) sentState("status", "missed", "You did not press a button", "The host has stopped taking presses.");
      else sentState("status", "locked", `🔒 Locked in: ${view.mine}`, "The host has stopped taking presses, so this is final. Wait for the reveal.");
    } else if (view.mine === null) {
      sentState("status", "waiting", "Not pressed yet", "Nobody is counted until they press one.");
    } else {
      sentState("status", "sent", `✓ Pressed: ${view.mine}`, "You can still change it until the host stops taking presses.");
    }
  }

  // A press sends at once: there is nothing to read back first.
  for (const [id, colour] of BUTTONS) {
    onClick(id, async () => {
      if (sending || last?.mine === colour) return;
      sending = true;
      try {
        const sent = await api<{ view: PlayerView }>("PUT", at("/press"), { colour });
        draw(sent.view);
        fail("");
        landed("status");
      } finally {
        sending = false;
        drawStatus();
      }
    });
  }

  function draw(view: PlayerView): void {
    last = view;
    html("you", `<span class="dot"></span> You are ${esc(view.name)}`);
    const round = view.round;
    if (!round) {
      text("p-state", "Waiting");
      text("p-rule", "Waiting for the first round");
      text("p-note", "");
      text("mine", "");
      hide("press", true);
      hide("status", true);
      html("p-outcome", "");
      return;
    }

    text("p-state", round.revealed ? "Revealed" : round.open ? "Press a button" : "Closed");
    text("p-rule", rule(round.percent));
    text(
      "p-note",
      "Whoever presses nothing is not counted, and nobody sees who pressed what until the reveal.",
    );
    hide("press", !round.open);
    drawStatus();

    text(
      "mine",
      round.revealed
        ? ""
        : `${view.pressed} of ${plural(view.players, "person", "people")} have pressed a button.`,
    );

    html(
      "p-outcome",
      view.outcome
        ? `<div class="panel">${
            view.mine === null
              ? `<p class="you-result">You did not press a button this round.</p>`
              : view.lived === true
                ? `<p class="you-result lived">You pressed ${view.mine} and lived.</p>`
                : `<p class="you-result">💀 You pressed ${view.mine} and did not make it.</p>`
          }${outcomeHtml(view.outcome)}${
            view.score && view.score.played
              ? `<p class="note">Your evening: lived through ${view.score.lived} of
                 ${plural(view.score.played, "round", "rounds")}.</p>`
              : ""
          }</div>`
        : "",
    );
  }

  // Fetches the room once, then keeps it up to date from its event
  // stream; `watch` adds the "/events" itself.
  showConnection(
    watch<PlayerView>(
      at(""),
      draw,
      "You are not in this room. The host may have taken you out, or the room has finished. Scan the code again to rejoin.",
    ),
  );
  void keepAwake();
}


// ------------------------------------------------------------------- wall

function drawWall(game: ButtonsWall | null, slot: HTMLElement, _feed: Feed, view: WallView<ButtonsWall | null>): void {
  const round = game?.round ?? null;
  if (!game || !round) {
    slot.replaceChildren(
      el("p", { className: "eyebrow wall-idle" }, "Red or blue"),
      el("p", { className: "bw-rule wall-idle" }, "Waiting for the first round"),
    );
    return;
  }
  const state = round.revealed
    ? "Revealed"
    : round.open
      ? "Press a button on your phone"
      : "The presses are in";
  const parts: Node[] = [
    el("p", { className: "eyebrow bw-state" }, state),
    el("p", { className: "bw-rule" }, rule(round.percent)),
    el("p", { className: "bw-note" }, "Whoever presses nothing is not counted"),
  ];
  if (!round.revealed) {
    parts.push(
      el("p", { className: "bw-count" }, el("b", {}, `${game.pressed} of ${game.players}`), " pressed"),
      el(
        "p",
        { className: "note bw-connected" },
        `${view.connected === null ? "" : `${view.connected} of ${plural(game.players, "phone", "phones")} connected · `}nobody sees a press until the reveal`,
      ),
    );
  }
  const shown = game.shown;
  if (shown) {
    const box = el("div", { className: "bw-shown" });
    box.innerHTML =
      shown.n === 0
        ? `<p class="verdict none"><b>Nobody pressed a button this round.</b></p>`
        : `<p class="verdict ${shown.blue_won ? "won" : "none"}"><b>${shown.blue_won ? "Everybody lives" : "Only red lives"}</b></p>
          <p class="bw-note">${esc(splitLine(round.percent, shown))}</p>
          ${splitBar(round.percent, shown)}`;
    parts.push(box);
  }
  slot.replaceChildren(...parts);
}

const where = route();
if (where.kind === "host") host(where.key);
else if (where.kind === "join") joining("buttons", where.key);
else if (where.kind === "player") player(where.key);
else if (where.kind === "wall") wall<ButtonsWall | null>(where.key, drawWall);
else opening();
