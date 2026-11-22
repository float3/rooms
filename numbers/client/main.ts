// The number games, in a browser.
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
  list,
  num,
  onClick,
  onSubmit,
  opening,
  plural,
  roster,
  route,
  sentState,
  setValue,
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
import { type Answers, read } from "./answer";

interface Playing {
  label: string;
  low: number;
  high: number;
  whole: boolean;
  answers: Answers;
  open: boolean;
  revealed: boolean;
  asked: number;
}

interface Chosen {
  name: string;
  value: number;
  text: string | null;
  won: boolean;
}

interface Outcome {
  n: number;
  mean: number | null;
  median: number | null;
  target: number | null;
  target_label: string | null;
  total: number | null;
  label: string;
  winners: string[];
  why: string | null;
  picks: Chosen[];
}

interface Who {
  id: number;
  name: string;
  picked: boolean;
}

interface Score {
  name: string;
  played: number;
  /** Every round won, a shared one counting in full. */
  wins: number;
  /** Of those, the rounds won alone. */
  solo: number;
  /** How far off the number to aim at, as a share of the range, on
   *  average over the rounds that had one. */
  off: number | null;
}

interface Past {
  label: string;
  target: number | null;
  mean: number | null;
  winners: string[];
  n: number;
  /** On the newest of a run of rounds with the same rule and range, the
   *  target of each, oldest first. */
  targets: number[];
}

interface HostView {
  join: string;
  /** The wall's key, for "Open the wall view". */
  wall: string;
  /** Everybody, with whose phone is connected. */
  people: Present[];
  /** Everybody, with whether they have picked this round. */
  players: Who[];
  /** Not connected, and no pick in this round or the one before. */
  away: number[];
  round: Playing | null;
  outcome: Outcome | null;
  history: Past[];
  scores: Score[];
}

interface PlayerView {
  name: string;
  round: Playing | null;
  mine: number | null;
  mine_text: string | null;
  picked: number;
  players: number;
  outcome: Outcome | null;
  won: boolean | null;
  score: Score | null;
}

/** One number on the wall's strip: how many picked it, never who. */
interface Spot {
  value: number;
  count: number;
  won: boolean;
}

/** What the wall shows of a round that has come out. */
interface Shown {
  n: number;
  target: number | null;
  target_label: string | null;
  mean: number | null;
  median: number | null;
  total: number | null;
  winners: number;
  why: string | null;
  spots: Spot[];
}

/** The game's part of the wall. Names only on the top of the board. */
interface NumbersWall {
  round: Playing | null;
  picked: number;
  players: number;
  shown: Shown | null;
  top: { name: string; wins: number; solo: number; played: number }[];
}

/** How a rule is sent, and the range to start the fields at. */
interface Preset {
  id: string;
  name: string;
  rule: string;
  num?: number;
  den?: number;
  /** Rules that count matching picks, which need whole numbers. */
  whole?: true;
  /** Only the limit rule has one, and only it shows the field. */
  limit?: number;
  low: number;
  high: number;
}

const RULES: Preset[] = [
  {
    id: "mean-1-3",
    name: "Closest to 1/3 of the average",
    rule: "of-the-mean",
    num: 1,
    den: 3,
    low: 0,
    high: 10,
  },
  {
    id: "mean-2-3",
    name: "Closest to 2/3 of the average",
    rule: "of-the-mean",
    num: 2,
    den: 3,
    low: 0,
    high: 100,
  },
  {
    id: "mean-1-2",
    name: "Closest to 1/2 of the average",
    rule: "of-the-mean",
    num: 1,
    den: 2,
    low: 0,
    high: 100,
  },
  {
    id: "mean-1-1",
    name: "Closest to the average",
    rule: "of-the-mean",
    num: 1,
    den: 1,
    low: 0,
    high: 100,
  },
  {
    id: "median-2-3",
    name: "Closest to 2/3 of the middle number",
    rule: "of-the-median",
    num: 2,
    den: 3,
    low: 0,
    high: 100,
  },
  {
    id: "lowest-unique",
    name: "The lowest number nobody else picked",
    rule: "lowest-unique",
    whole: true,
    low: 1,
    high: 20,
  },
  {
    id: "highest-unique",
    name: "The highest number nobody else picked",
    rule: "highest-unique",
    whole: true,
    low: 1,
    high: 20,
  },
  {
    id: "under-the-mean",
    name: "The highest number below the average",
    rule: "under-the-mean",
    low: 0,
    high: 100,
  },
  {
    id: "most-popular",
    name: "The number the most people pick",
    rule: "most-popular",
    whole: true,
    low: 1,
    high: 10,
  },
  {
    id: "loneliest",
    name: "The number furthest from everybody else's",
    rule: "loneliest",
    low: 0,
    high: 100,
  },
  {
    id: "under-the-limit",
    name: "The highest number, if the room's total is no more than the limit",
    rule: "under-the-limit",
    limit: 100,
    low: 0,
    high: 100,
  },
];

const chosen = (): Preset =>
  RULES.find((rule) => rule.id === byId<HTMLSelectElement>("rule").value) ??
  RULES[0];

const sentence = (said: string): string =>
  said.charAt(0).toUpperCase() + said.slice(1);

/** A share of a range as a percentage: "4%", or "0.5%" below ten. */
const share = (part: number): string => {
  const percent = part * 100;
  return `${percent < 10 ? Number(percent.toFixed(1)) : Math.round(percent)}%`;
};

/** How many of someone's wins were theirs alone, said only when some
 *  were shared: "(1 alone)", or "(shared)" when none were. Ranks equal
 *  wins on the board. */
const alone = (score: { wins: number; solo: number }): string =>
  score.solo === score.wins ? "" : score.solo === 0 ? " (shared)" : ` (${score.solo} alone)`;

/** Wins on the board, with the ones won alone when some were shared. */
const won = (score: { wins: number; solo: number }): string =>
  `${score.wins}${alone(score) ? `<span class="alone">${alone(score)}</span>` : ""}`;

/** A pick as it was typed, with what it came to when that differs. */
const typed = (text: string | null, value: number): string =>
  text === null || text === num(value) ? num(value) : `${text} = ${num(value)}`;

const ANSWERS: Record<Answers, string> = {
  whole: "whole numbers",
  decimal: "decimals allowed",
  expression: "sums allowed, like 1,001/3",
};

/** Every number along the round's range, with the target marked. */
function strip(round: Playing, outcome: Outcome): string {
  const width = round.high - round.low || 1;
  const at = (given: number) =>
    Math.min(100, Math.max(0, ((given - round.low) / width) * 100));
  const target =
    outcome.target === null
      ? ""
      : `<div class="target" style="left:${at(outcome.target)}%"><span>${num(outcome.target)}</span></div>`;
  // One dot per distinct number, sized by how many picked it, with the
  // winning numbers drawn last so nothing covers them.
  const spots = new Map<number, { value: number; names: string[]; won: boolean }>();
  for (const pick of outcome.picks) {
    const spot = spots.get(pick.value) ?? { value: pick.value, names: [], won: false };
    spot.names.push(pick.name);
    spot.won ||= pick.won;
    spots.set(pick.value, spot);
  }
  const dots = [...spots.values()]
    .sort((a, b) => Number(a.won) - Number(b.won))
    .map(
      (spot) =>
        `<div class="pick${spot.won ? " winner" : ""}" style="left:${at(spot.value)}%" title="${esc(list(spot.names))}: ${num(spot.value)}">${
          spot.names.length > 1 ? `<i>${spot.names.length}</i>` : ""
        }${spot.won ? `<span>${esc(list(spot.names))}</span>` : ""}</div>`,
    )
    .join("");
  return `<div class="strip">${target}${dots}</div>
    <div class="ends"><span>${num(round.low)}</span><span>${num(round.high)}</span></div>`;
}

function outcomeHtml(round: Playing, outcome: Outcome): string {
  const winners =
    outcome.winners.length > 0
      ? `<div class="verdict won"><p class="eyebrow">${outcome.winners.length === 1 ? "The winner" : "Shared between"}</p>
         <p class="big">${esc(list(outcome.winners))}</p></div>`
      : `<div class="verdict none"><p class="eyebrow">Nobody won</p>
         <p class="big">${esc(outcome.why ?? "Nobody's number met the rule this round.")}</p></div>`;
  if (outcome.n === 0) return winners;
  const rows: [string, string][] = [
    ["The average", num(outcome.mean)],
    ["The middle number", num(outcome.median)],
  ];
  if (outcome.target !== null && outcome.target_label !== "The average, to stay under") {
    rows.unshift([outcome.target_label ?? "The number to aim at", num(outcome.target)]);
  }
  if (outcome.total !== null) {
    rows.unshift(["Everybody together", num(outcome.total)]);
  }
  return `${winners}
    ${strip(round, outcome)}
    <div class="facts">${rows
      .map(
        ([name, said]) =>
          `<div class="fact"><span>${name}</span><b>${said}</b></div>`,
      )
      .join("")}</div>
    <div class="scroll"><table><thead><tr><th>Who</th><th>Picked</th></tr></thead>
      <tbody>${outcome.picks
        .map(
          (pick) => `<tr class="${pick.won ? "good" : ""}">
        <td>${pick.won ? "🏆 " : ""}${esc(pick.name)}</td><td>${esc(typed(pick.text, pick.value))}</td>
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
  html(
    "rule",
    RULES.map(
      (rule) => `<option value="${rule.id}">${esc(rule.name)}</option>`,
    ).join(""),
  );

  /** Each rule has a range to start from, until the host edits one. */
  let touched = false;
  for (const id of ["low", "high"]) {
    byId(id).addEventListener("input", () => {
      touched = true;
    });
  }
  const answers = byId<HTMLSelectElement>("answers");
  answers.value = "decimal";
  function answersNote(): void {
    const rule = chosen();
    text(
      "answers-note",
      rule.whole
        ? "This rule counts who picked the same number, so it needs whole numbers. Expressions are fine if they come out whole."
        : answers.value === "expression"
          ? "Phones show what a sum comes to before it is sent, and everybody sees the sums at the reveal."
          : "",
    );
  }
  answers.addEventListener("change", answersNote);
  function ruleChanged(): void {
    const rule = chosen();
    const decimal = answers.querySelector<HTMLOptionElement>('option[value="decimal"]');
    if (decimal) decimal.disabled = rule.whole === true;
    if (rule.whole && answers.value === "decimal") answers.value = "whole";
    answersNote();
    hide("limit-field", rule.limit === undefined);
    if (rule.limit !== undefined) setValue("limit", rule.limit);
    if (touched) return;
    setValue("low", rule.low);
    setValue("high", rule.high);
  }
  byId("rule").addEventListener("change", ruleChanged);
  ruleChanged();

  onSubmit("start-form", async () => {
    // Starting a round closes the one before it without revealing it, so
    // its numbers would never be shown. Ask, but only when there are some.
    // A second tap rather than confirm(), which the hall's frame may not
    // show.
    const current = now?.round;
    if (
      current &&
      !current.revealed &&
      now?.players.some((who) => who.picked) &&
      !confirmTap(byId("start"), "Nobody has seen these numbers. Tap again to drop them")
    ) {
      return;
    }
    const rule = chosen();
    const body: Record<string, unknown> = {
      rule: rule.rule,
      answers: answers.value,
      low: value("low"),
      high: value("high"),
    };
    if (rule.num !== undefined) {
      body.num = rule.num;
      body.den = rule.den;
    }
    if (rule.limit !== undefined) body.limit = value("limit");
    await api("POST", at("/start"), body);
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
    // Only once a round with a number to aim at has come out.
    const aimed = view.scores.some((score) => score.off !== null);
    html(
      "board",
      `<h3>The evening so far</h3><div class="scroll"><table><thead><tr>
        <th>Who</th><th>Rounds</th><th>Won</th>${aimed ? "<th>Off the target</th>" : ""}
      </tr></thead><tbody>${view.scores
        .map(
          (score) => `<tr>
        <td>${esc(score.name)}</td><td>${score.played}</td><td>${won(score)}</td>
        ${aimed ? `<td>${score.off === null ? "–" : share(score.off)}</td>` : ""}
      </tr>`,
        )
        .join("")}</tbody></table></div>${
        aimed
          ? `<p class="note">Off the target: how far each pick was from the number to aim at, as a share of the round's range, on average over the rounds that had one. Equal wins go by rounds won alone, then by this.</p>`
          : ""
      }`,
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
        <span>${esc(sentence(past.label))}</span>
        <span>${past.n === 0 ? "nobody picked" : `${past.target === null ? "" : `target ${num(past.target)} · `}average ${num(past.mean)} · ${
          past.winners.length > 0 ? `won by ${esc(list(past.winners))}` : "nobody won"
        }`}${
          past.targets.length > 1
            ? `<b class="trail">${past.label === "the highest number below the average" ? "averages" : "targets"} ${past.targets
                .map((target) => num(target))
                .join(" → ")}</b>`
            : ""
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
      text(
        "h-state",
        round.revealed ? "Revealed" : round.open ? "Taking numbers" : "Closed",
      );
      text("h-rule", sentence(round.label));
      text(
        "h-range",
        `${num(round.low)} to ${num(round.high)}, ${ANSWERS[round.answers]}`,
      );
      const picked = new Set(view.players.filter((who) => who.picked).map((who) => who.id));
      text(
        "progress",
        round.revealed
          ? `${picked.size} of ${plural(view.players.length, "person", "people")} picked`
          : `${waitingLine(picked, view.people)} · nobody sees a number until you reveal`,
      );
      text("close", round.open ? "Stop taking numbers" : "Take more numbers");
      // Once the round is out, the next thing is often the same rule
      // again, to see where the room goes with it.
      hide("close", round.revealed);
      hide("reveal", round.revealed);
      hide("again", !round.revealed);
      html("outcome", view.outcome ? outcomeHtml(round, view.outcome) : "");
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
  const field = byId<HTMLInputElement>("pick");
  let filledFor: number | null = null;
  let last: PlayerView | null = null;
  let sending = false;

  /** What the field reads as, in the current round. */
  const reading = () => (last?.round ? read(last.round.answers, field.value) : null);

  /** Whether what is in the field is what the room already has. */
  function unchanged(view: PlayerView): boolean {
    if (view.mine === null) return false;
    if (view.mine_text !== null) return field.value.trim() === view.mine_text;
    const r = reading();
    return r !== null && "value" in r && r.value === view.mine;
  }

  /** The line under the field: what it reads as, or why it does not. */
  function drawReading(): void {
    const round = last?.round;
    const line = byId("reading");
    const r = reading();
    if (!round || r === null || field.value.trim() === "") {
      line.textContent = "";
      line.className = "reading";
      return;
    }
    if ("error" in r) {
      line.textContent = r.error;
      line.className = "reading bad";
    } else if (r.value < round.low || r.value > round.high) {
      line.textContent = `That comes to ${num(r.value)}, outside ${num(round.low)} to ${num(round.high)}.`;
      line.className = "reading bad";
    } else {
      // Only worth saying when it is not just the number typed back.
      const plain = field.value.trim();
      line.textContent = plain === String(r.value) ? "" : `Reads as ${num(r.value)}`;
      line.className = "reading";
    }
  }

  /** Whether your number is in, and whether you can still change it. */
  function drawStatus(): void {
    const view = last;
    const round = view?.round;
    const send = byId<HTMLButtonElement>("send");
    if (!view || !round || round.revealed) {
      sentState("status", null);
      return;
    }
    const yours = view.mine === null ? "" : typed(view.mine_text, view.mine);
    const fresh = !unchanged(view);
    send.classList.toggle("done", !fresh);
    send.disabled = sending || !fresh;
    if (!round.open) {
      if (view.mine === null) sentState("status", "missed", "You did not send a number", "The host has stopped taking numbers.");
      else sentState("status", "locked", `🔒 Locked in: ${yours}`, "The host has stopped taking numbers, so this is final. Wait for the reveal.");
    } else if (view.mine === null) {
      sentState("status", "waiting", "Not sent yet", "Your number only counts once you press Send.");
      send.textContent = "Send";
    } else if (!fresh) {
      sentState("status", "sent", `✓ Sent: ${yours}`, "You can still change it until the host stops taking numbers.");
      send.textContent = "Sent ✓";
    } else {
      sentState("status", "changed", "Your change is not sent yet", `The room still has ${yours} from you. Press Send to replace it.`);
      send.textContent = "Send the change";
    }
    text("pick-label", view.mine === null ? "Your number" : "Change your number");
  }

  field.addEventListener("input", () => {
    drawReading();
    drawStatus();
  });

  onSubmit("pick-form", async () => {
    const r = reading();
    if (r === null || "error" in r) {
      fail(r?.error ?? "Pick a number first.");
      return;
    }
    sending = true;
    try {
      const body = await api<{ view: PlayerView }>("PUT", at("/pick"), {
        text: field.value.trim(),
      });
      draw(body.view);
      fail("");
      landed("status");
    } finally {
      sending = false;
      drawStatus();
    }
  });

  function draw(view: PlayerView): void {
    last = view;
    html("you", `<span class="dot"></span> You are ${esc(view.name)}`);
    const round = view.round;
    if (!round) {
      text("p-state", "Waiting");
      text("p-rule", "Waiting for the first round");
      text("p-range", "");
      text("mine", "");
      hide("pick-form", true);
      hide("status", true);
      html("p-outcome", "");
      return;
    }

    text(
      "p-state",
      round.revealed ? "Revealed" : round.open ? "Pick a number" : "Closed",
    );
    text("p-rule", sentence(round.label));
    text(
      "p-range",
      `Anything from ${num(round.low)} to ${num(round.high)}. ${
        round.answers === "whole"
          ? "Whole numbers only."
          : round.answers === "decimal"
            ? "Decimals are fine."
            : `You can type a sum, like 1,001/3 or 2^10 - 1${round.whole ? ", as long as it comes out whole" : ""}.`
      }`,
    );
    hide("pick-form", !round.open);

    // A phone keyboard to suit: digits for plain numbers, letters and
    // signs for sums, and a minus key whenever the range goes below nought.
    field.inputMode =
      round.answers === "expression" || round.low < 0 ? "text" : round.answers === "whole" ? "numeric" : "decimal";
    field.placeholder = round.answers === "expression" ? "e.g. 1,001/3" : "";
    if (filledFor !== round.asked) {
      filledFor = round.asked;
      field.value = view.mine === null ? "" : (view.mine_text ?? String(view.mine));
    }
    drawReading();
    drawStatus();

    text(
      "mine",
      round.revealed ? "" : `${view.picked} of ${plural(view.players, "person", "people")} have sent a number.`,
    );

    html(
      "p-outcome",
      view.outcome
        ? `<div class="panel">${
            view.mine === null
              ? `<p class="you-result">You did not pick this round.</p>`
              : view.won === true
                ? `<p class="you-result good">🎉 You won with ${esc(typed(view.mine_text, view.mine))}</p>`
                : `<p class="you-result">You picked ${esc(typed(view.mine_text, view.mine))}, not a winner this time.</p>`
          }${outcomeHtml(round, view.outcome)}${
            view.score && view.score.played
              ? `<p class="note">Your evening: ${view.score.wins} of
                 ${plural(view.score.played, "round", "rounds")} won${
                   view.score.off === null
                     ? "."
                     : `, ${share(view.score.off)} of the range off the target on average.`
                 }</p>`
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

/** The unnamed numbers along the range: a dot per number, as large as
 *  the number of people who picked it, the winning ones in gold. */
function wallStrip(round: Playing, shown: Shown): string {
  const width = round.high - round.low || 1;
  const at = (given: number) =>
    Math.min(100, Math.max(0, ((given - round.low) / width) * 100));
  const most = Math.max(1, ...shown.spots.map((spot) => spot.count));
  const target =
    shown.target === null
      ? ""
      : `<div class="target" style="left:${at(shown.target)}%"><span>${esc(num(shown.target))}</span></div>`;
  const dots = [...shown.spots]
    .sort((a, b) => Number(a.won) - Number(b.won))
    .map((spot) => {
      const size = 0.7 + 1.1 * Math.sqrt(spot.count / most);
      return `<div class="spot${spot.won ? " winner" : ""}" style="left:${at(spot.value)}%;--size:${size.toFixed(2)}em" title="${esc(num(spot.value))}">${
        spot.count > 1 ? `<i>${spot.count}</i>` : ""
      }${spot.won ? `<span>${esc(num(spot.value))}</span>` : ""}</div>`;
    })
    .join("");
  return `<div class="wall-strip">${target}${dots}</div>
    <div class="ends"><span>${esc(num(round.low))}</span><span>${esc(num(round.high))}</span></div>`;
}

function drawWall(game: NumbersWall | null, slot: HTMLElement, _feed: Feed, view: WallView<NumbersWall | null>): void {
  const round = game?.round ?? null;
  if (!game || !round) {
    slot.replaceChildren(
      el("p", { className: "eyebrow wall-idle" }, "Number games"),
      el("p", { className: "nw-rule wall-idle" }, "Waiting for the first round"),
    );
    return;
  }
  const state = round.revealed ? "Revealed" : round.open ? "Pick a number on your phone" : "Numbers are in";
  slot.classList.toggle("nw-revealed", round.revealed);
  const parts: Node[] = [
    el("p", { className: "eyebrow nw-state" }, state),
    el("p", { className: "nw-rule" }, sentence(round.label)),
    el("p", { className: "nw-range" }, `${num(round.low)} to ${num(round.high)} · ${ANSWERS[round.answers]}`),
  ];
  if (!round.revealed) {
    parts.push(
      el(
        "p",
        { className: "nw-count" },
        el("b", {}, `${game.picked} of ${game.players}`),
        ` picked`,
      ),
      el("p", { className: "note nw-connected" }, `${view.connected === null ? "" : `${view.connected} of ${plural(game.players, "phone", "phones")} connected · `}nobody sees a number until the reveal`),
    );
  }
  const shown = game.shown;
  if (shown) {
    const verdict =
      shown.winners > 0
        ? `${plural(shown.winners, "winner", "winners")} out of ${shown.n}`
        : shown.why ?? "Nobody won this round.";
    const box = el("div", { className: "nw-shown" });
    const facts: [string, string][] = [];
    if (shown.total !== null) facts.push(["Everybody together", num(shown.total)]);
    if (shown.target !== null && shown.target_label !== "The average, to stay under") {
      facts.push([shown.target_label ?? "The number to aim at", num(shown.target)]);
    }
    if (shown.n > 0) {
      facts.push(["The average", num(shown.mean)], ["The middle number", num(shown.median)]);
    }
    box.innerHTML = `<p class="verdict ${shown.winners > 0 ? "won" : "none"}"><b>${esc(verdict)}</b></p>
      ${shown.n > 0 ? wallStrip(round, shown) : ""}
      <div class="facts">${facts
        .map(([name, said]) => `<div class="fact"><span>${esc(name)}</span><b>${esc(said)}</b></div>`)
        .join("")}</div>`;
    parts.push(box);
  }
  if (game.top.length > 0) {
    parts.push(
      el(
        "div",
        { className: "nw-board" },
        el("p", { className: "eyebrow nw-top-title" }, "Top of the board"),
        el(
          "ol",
          { className: "nw-top" },
          ...game.top.map((top) =>
            el("li", {}, el("span", { className: "who" }, top.name), el("span", { className: "wins" }, plural(top.wins, "win", "wins"), alone(top) ? el("span", { className: "alone" }, alone(top)) : null)),
          ),
        ),
      ),
    );
  }
  slot.replaceChildren(...parts);
}

const where = route();
if (where.kind === "host") host(where.key);
else if (where.kind === "join") joining("numbers", where.key);
else if (where.kind === "player") player(where.key);
else if (where.kind === "wall") wall<NumbersWall | null>(where.key, drawWall);
else opening();
