// The estimation game, in a browser.
//
// One page with four screens: the one that opens a room, the host's, the
// player's, which is almost always a phone, and the wall, which is
// projected for the whole room.

import {
  api,
  byId,
  confirmTap,
  connectedCount,
  el,
  esc,
  fail,
  failed,
  hide,
  html,
  joining,
  joinLink,
  keepAwake,
  landed,
  num,
  onClick,
  onSubmit,
  opening,
  plural,
  type Present,
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

type Stage = "first" | "discussion" | "second" | "revealed";

interface Bound {
  low: number;
  high: number;
  at: number;
}

interface Extreme {
  name: string;
  value: number;
}

interface Stats {
  n: number;
  median_low: number;
  median_high: number;
  mean_low: number;
  mean_high: number;
  lowest: Extreme;
  highest: Extreme;
  widest: Extreme;
  tightest: Extreme;
  median_width: number;
  hits: number | null;
}

interface Change {
  name: string;
  before: Bound;
  after: Bound;
  moved: number;
  width_change: number;
  hit_before: boolean | null;
  hit_after: boolean | null;
}

interface Pull {
  name: string;
  towards: number;
}

interface Shift {
  n: number;
  moved: number;
  narrower: number;
  wider: number;
  median_middle_before: number;
  median_middle_after: number;
  median_width_before: number;
  median_width_after: number;
  biggest: Change | null;
  pulled: Pull | null;
  hits_before: number | null;
  hits_after: number | null;
  towards: number | null;
}

interface Asked {
  question: string;
  unit: string;
  stage: Stage;
  answer: number | null;
  /** The level it was asked at, which its numbers are judged against. */
  confidence: number;
  asked: number;
}

interface Who {
  id: number;
  name: string;
  first: boolean;
  second: boolean;
}

interface Answer {
  name: string;
  low: number;
  high: number;
  width: number;
  hit: boolean | null;
}

interface Score {
  name: string;
  answered: number;
  scored: number;
  hits: number;
  /** How many should have held the answer, at each question's level. */
  expected: number;
  /** The level, when every scored question was asked at the same one. */
  level: number | null;
  calibration: "narrow" | "wide" | "right" | null;
  /** The interval score's two parts, averaged; lower is better. */
  width_cost: number | null;
  miss_cost: number | null;
  tighter: number;
  changed_mind: number;
}

interface Past {
  index: number;
  question: string;
  unit: string;
  answer: number | null;
  confidence: number;
  n: number;
  hits: number | null;
  hits_before: number | null;
}

/** A question lined up: see `game::Queued`. */
interface Queued {
  question: string;
  answer: number | null;
  unit: string;
}

/** The list as the host would type it, for the field it is edited in. */
function queueText(queue: Queued[]): string {
  return queue
    .map((q) => {
      const parts = [q.question];
      if (q.answer !== null || q.unit) parts.push(q.answer === null ? "" : String(q.answer));
      if (q.unit) parts.push(q.unit);
      return parts.join(" | ");
    })
    .join("\n");
}

/** Sets a button's label, unless it is asking "Tap again" (confirmTap):
 *  then the label is kept for afterwards, and the question is dropped if
 *  there is nothing left to confirm. */
function label(button: HTMLElement, said: string, asking: boolean): void {
  if (button.dataset.armed !== "1") button.textContent = said;
  else if (asking) button.dataset.said = said;
  else {
    delete button.dataset.armed;
    button.classList.remove("armed");
    button.textContent = said;
  }
}

interface HostView {
  join: string;
  /** The wall's key, for "Open the wall view". */
  wall: string;
  /** Who is in, and whose phone is connected. */
  people: Present[];
  /** The level the next question will be asked at. */
  confidence: number;
  players: Who[];
  /** Not connected, and no range for this question or the one before. */
  away: number[];
  round: Asked | null;
  before: Stats | null;
  after: Stats | null;
  standing: Stats | null;
  shift: Shift | null;
  changes: Change[];
  answers: Answer[];
  /** The picture, once the question is revealed. */
  reveal: Reveal | null;
  history: Past[];
  scores: Score[];
  /** The questions lined up. Only the host is sent them. */
  queue: Queued[];
}

interface PlayerView {
  name: string;
  confidence: number;
  round: Asked | null;
  mine_before: Bound | null;
  mine_after: Bound | null;
  submitted: number;
  players: number;
  standing: Stats | null;
  before: Stats | null;
  shift: Shift | null;
  hit: boolean | null;
  hit_before: boolean | null;
  mine_changed: Change | null;
  /** Everybody's ranges, unnamed, with this player's marked; after the
   *  reveal only. */
  reveal: Reveal | null;
  score: Score | null;
}

/** The unit of the current question, appended to every number shown. */
let unit = "";

const withUnit = (given: number): string =>
  unit ? `${num(given)} ${esc(unit)}` : num(given);
const range = (low: number, high: number): string =>
  `${num(low)} – ${withUnit(high)}`;
const ranged = (bound: Bound): string => range(bound.low, bound.high);
/** A width rather than a pair of bounds, labelled so it is not read as
 *  one. */
const span = (width: number): string => `${withUnit(width)} apart`;

const STAGES: Record<Stage, string> = {
  first: "First ranges",
  discussion: "Talking it over",
  second: "Second ranges",
  revealed: "Revealed",
};

function facts(rows: [string, string][]): string {
  return `<div class="facts">${rows
    .map(([name, said]) => `<div class="fact"><span>${name}</span>${said}</div>`)
    .join("")}</div>`;
}

/** An interval score, to two places, or one once it is in the tens. */
const points = (cost: number): string => cost.toFixed(cost >= 10 ? 1 : 2);

const VERDICT: Record<"narrow" | "wide" | "right", string> = {
  narrow: "your ranges are too narrow",
  wide: "your ranges are wider than they need to be",
  right: "that is about right",
};

/** "7 of 10 contained it; at 90% about 9 should, so your ranges are too
 *  narrow." What the game is there to teach. */
function calibrationLine(score: Score): string {
  const at = score.level === null ? "at the levels asked" : `at ${score.level}%`;
  const said = `${score.hits} of ${score.scored} contained the answer; ${at} about ${Math.round(score.expected)} should`;
  return score.calibration === null
    ? `${said}.`
    : score.calibration === "right"
      ? `${said}, and ${VERDICT.right}.`
      : `${said}, so ${VERDICT[score.calibration]}.`;
}

/** The room's numbers for one pass. */
function statsHtml(stats: Stats, confidence: number): string {
  const rows: [string, string][] = [
    [
      "The middle of the room",
      `<b>${range(stats.median_low, stats.median_high)}<small>median of the lower and the upper bounds</small></b>`,
    ],
    ["On average", `<b>${range(stats.mean_low, stats.mean_high)}</b>`],
    [
      "As low as anyone went",
      `<b>${withUnit(stats.lowest.value)}<small>${esc(stats.lowest.name)}</small></b>`,
    ],
    [
      "As high as anyone went",
      `<b>${withUnit(stats.highest.value)}<small>${esc(stats.highest.name)}</small></b>`,
    ],
    [
      "Widest range",
      `<b>${span(stats.widest.value)}<small>${esc(stats.widest.name)}</small></b>`,
    ],
    [
      "Tightest range",
      `<b>${span(stats.tightest.value)}<small>${esc(stats.tightest.name)}</small></b>`,
    ],
    ["Middling width", `<b>${span(stats.median_width)}</b>`],
  ];
  if (stats.hits !== null) {
    const expected = Math.round((stats.n * confidence) / 100);
    rows.push([
      "Contained the answer",
      `<b>${stats.hits} of ${stats.n}<small>at ${confidence}%, about ${expected} should</small></b>`,
    ]);
  }
  return facts(rows);
}

/** What changed between the two passes. */
function shiftHtml(shift: Shift): string {
  const rows: [string, string][] = [
    [
      "Changed their range",
      `<b>${shift.moved} of ${shift.n}<small>${shift.narrower} narrowed, ${shift.wider} widened</small></b>`,
    ],
    [
      "The middle of the room",
      `<b>${num(shift.median_middle_before)} → ${withUnit(shift.median_middle_after)}</b>`,
    ],
    [
      "Middling width",
      `<b>${num(shift.median_width_before)} → ${span(shift.median_width_after)}</b>`,
    ],
  ];
  if (shift.biggest) {
    rows.push([
      "Moved furthest",
      `<b>${esc(shift.biggest.name)}<small>${ranged(shift.biggest.before)} → ${ranged(shift.biggest.after)}</small></b>`,
    ]);
  }
  if (shift.pulled) {
    rows.push([
      "The room moved towards",
      `<b>${esc(shift.pulled.name)}<small>everyone else ended ${withUnit(shift.pulled.towards)} closer to their first answer</small></b>`,
    ]);
  }
  if (shift.hits_before !== null && shift.hits_after !== null) {
    rows.push([
      "Contained the answer",
      `<b>${shift.hits_before} → ${shift.hits_after} of ${shift.n}</b>`,
    ]);
  }
  if (shift.towards !== null) {
    rows.push([
      "Moved towards the answer",
      `<b>${shift.towards} of ${shift.n}</b>`,
    ]);
  }
  return facts(rows);
}


// ------------------------------------------------------------------- host

function host(key: string): void {
  show("host");
  const at = (path: string) => `/api/h/${encodeURIComponent(key)}${path}`;
  let answerFor: number | null = null;
  /** The question still being answered, which asking another one ends. */
  let open: Asked | null = null;

  text("where", "You are the host");

  // Asking the next question reveals the open one, so the button asks
  // once more first. Not with confirm(), which a browser may not show
  // inside the shared room's frame.
  onSubmit("ask-form", async () => {
    if (
      open &&
      !confirmTap(
        byId("ask-button"),
        open.stage === "first"
          ? "Reveal it with no second range? Tap again"
          : "Reveal it to everybody? Tap again",
      )
    ) {
      return;
    }
    await api("POST", at("/ask"), {
      question: byId<HTMLInputElement>("question").value,
      unit: byId<HTMLInputElement>("unit").value,
      answer: value("answer"),
    });
    byId<HTMLInputElement>("question").value = "";
    byId<HTMLInputElement>("unit").value = "";
    byId<HTMLInputElement>("answer").value = "";
    unsign(byId("ask-form"));
    fail("");
  });

  onClick("ask-next", async () => {
    if (
      open &&
      !confirmTap(
        byId("ask-next"),
        open.stage === "first"
          ? "Reveal it with no second range? Tap again"
          : "Reveal it to everybody? Tap again",
      )
    ) {
      return;
    }
    await api("POST", at("/next"));
    fail("");
  });

  // The list is the host's to edit: a view arriving while they type does
  // not replace it, and saving does.
  let queueEdited = false;
  let drawnQueue = "";
  byId("queue-text").addEventListener("input", () => {
    queueEdited = true;
  });
  onClick("queue-save", async () => {
    await api("PUT", at("/queue"), { text: byId<HTMLTextAreaElement>("queue-text").value });
    queueEdited = false;
    drawnQueue = "";
    fail("");
  });

  function drawQueue(view: HostView): void {
    const next = view.queue[0];
    hide("next-box", !next);
    if (next) {
      text(
        "next-what",
        `Next of ${view.queue.length}: “${next.question}”${next.answer === null ? "" : " (answer ready)"}`,
      );
      label(byId("ask-next"), open ? "Reveal this one and ask next" : "Ask next", open !== null);
    }
    text(
      "queue-summary",
      view.queue.length === 0
        ? "Line up a list of questions"
        : `${plural(view.queue.length, "question", "questions")} lined up: see or change the list`,
    );
    const drawing = queueText(view.queue);
    if (!queueEdited && drawing !== drawnQueue) {
      drawnQueue = drawing;
      byId<HTMLTextAreaElement>("queue-text").value = drawing;
    }
  }

  onSubmit("answer-form", async () => {
    await api("PUT", at("/answer"), { answer: value("true-answer") });
    fail("");
  });

  const moveTo = (stage: Stage) => api("PUT", at("/stage"), { stage });
  onClick("to-discussion", async () => {
    await moveTo("discussion");
    fail("");
  });
  onClick("to-second", async () => {
    await moveTo("second");
    fail("");
  });
  onClick("to-first", async () => {
    await moveTo("first");
    fail("");
  });
  onClick("reveal", async () => {
    await api("POST", at("/reveal"));
    fail("");
  });

  // Answers 204; the host's own stream brings the new view.
  const remove = (person: Present) =>
    api("POST", at("/remove"), { id: person.id }).then(() => fail(""));

  function drawRoom(view: HostView): void {
    shareLink("join-share", joinLink(view.join));
    // The list is folded; the count of who is away stays in sight.
    text(
      "who",
      connectedCount(view.people) + (view.away.length ? ` · ${view.away.length} away` : ""),
    );
    roster("people", view.people, {
      remove,
      away: { ids: view.away, remove: (ids) => api("POST", at("/remove-away"), { ids }) },
    });
    wallButton("wall-button", view.wall);

    // The level is fixed while a question is open: whoever has sent a
    // range sent it at this one.
    const fixed = open !== null;
    html(
      "levels",
      [50, 80, 90, 98]
        .map(
          (level) =>
            `<button type="button" class="chip" data-level="${level}" aria-pressed="${level === view.confidence}"${
              fixed && level !== view.confidence ? " disabled" : ""
            }>${level}%</button>`,
        )
        .join(""),
    );
    text(
      "levels-note",
      fixed
        ? "Fixed until this question is revealed. Change it before the next one."
        : "Use the same level all evening. Each question keeps the level it was asked at.",
    );
    for (const chip of Array.from(byId("levels").children)) {
      chip.addEventListener("click", () => {
        const level = Number((chip as HTMLElement).dataset.level);
        api("PUT", at("/confidence"), { confidence: level })
          .then(() => fail(""))
          .catch((e: unknown) =>
            fail(e instanceof Error ? e.message : String(e)),
          );
      });
    }
  }

  function drawWaiting(view: HostView, round: Asked): void {
    const taking = round.stage === "second" ? "second" : "first";
    const inYet = new Set(
      view.players
        .filter((who) => (taking === "second" ? who.second : who.first))
        .map((who) => who.id),
    );
    let said =
      round.stage === "revealed"
        ? `${inYet.size} of ${plural(view.players.length, "person", "people")} answered`
        : waitingLine(inYet, view.people, "answered");
    if (round.answer === null) said += " · no answer entered yet";
    text("progress", said);
  }

  function drawNumbers(view: HostView): void {
    const round = view.round;
    if (!round) return;
    const parts: string[] = [];
    const twice = view.shift !== null;
    if (round.stage === "revealed" && twice) {
      parts.push(
        `<h3>What the discussion did</h3>${shiftHtml(view.shift as Shift)}`,
      );
    }
    if (view.standing) {
      parts.push(
        `<h3>${twice ? "Where the room ended up" : "What the room thinks"}</h3>${statsHtml(view.standing, round.confidence)}`,
      );
    }
    if (twice && view.before) {
      parts.push(
        `<details><summary>Before the discussion</summary>${statsHtml(view.before, round.confidence)}</details>`,
      );
    }
    if (!view.standing) parts.push(`<p class="note">No ranges yet.</p>`);
    html("numbers", parts.join(""));
  }

  function drawRanges(view: HostView): void {
    if (view.answers.length === 0) {
      html("ranges", "");
      return;
    }
    const scored = view.answers.some((answer) => answer.hit !== null);
    const changed = new Map(view.changes.map((change) => [change.name, change]));
    const chart = view.reveal ? rangeChart(view.reveal, unit, false) : null;
    html(
      "ranges",
      `<h3>Everybody's ranges</h3>${chart ? chart.outerHTML : ""}<div class="scroll"><table><thead><tr>
        <th>Who</th><th>At least</th><th>At most</th><th>Moved</th>${scored ? "<th>In?</th>" : ""}
      </tr></thead><tbody>${view.answers
        .map((answer) => {
          const change = changed.get(answer.name);
          return `<tr>
            <td>${esc(answer.name)}</td><td>${num(answer.low)}</td><td>${num(answer.high)}</td>
            <td>${change ? num(change.moved) : "—"}</td>
            ${scored ? `<td class="${answer.hit ? "good" : "poor"}">${answer.hit ? "yes" : "no"}</td>` : ""}
          </tr>`;
        })
        .join("")}</tbody></table></div>`,
    );
  }

  function drawBoard(view: HostView): void {
    if (view.scores.length === 0) {
      html("board", "");
      return;
    }
    const scored = view.scores.some((score) => score.scored > 0);
    const short: Record<"narrow" | "wide" | "right", string> = {
      narrow: "too narrow",
      wide: "too wide",
      right: "about right",
    };
    html(
      "board",
      `<h3>The evening so far</h3><div class="scroll"><table><thead><tr>
        <th>Who</th>${scored ? "<th>Score</th><th>In</th>" : "<th>Answered</th>"}<th>Changed</th>
      </tr></thead><tbody>${view.scores
        .map((score) => {
          const cost =
            score.width_cost === null || score.miss_cost === null
              ? "—"
              : `<b>${points(score.width_cost + score.miss_cost)}</b><small>${points(score.width_cost)} + ${points(score.miss_cost)}</small>`;
          const held = score.scored
            ? `${score.hits}/${score.scored}<small>aim ${Math.round(score.expected)}${
                score.calibration ? ` · ${short[score.calibration]}` : ""
              }</small>`
            : "—";
          return `<tr>
          <td>${esc(score.name)}</td>
          ${scored ? `<td class="cost">${cost}</td><td class="cost">${held}</td>` : `<td>${score.answered}</td>`}
          <td>${score.changed_mind}</td>
        </tr>`;
        })
        .join("")}</tbody></table></div>
      <p class="note">Lowest score first, like golf. The score is the width
        of each range plus what its miss cost, averaged over the questions
        with an answer. A miss costs 2 ÷ (1 − the level) times the distance
        it missed by: 20 times at 90%. A range wide enough to hold
        everything loses on width; one too narrow to hold the answer loses
        on misses. Distances are in powers of ten when every number in a
        question is above zero, and in the room's median first range
        otherwise. “In” counts the ranges that held the answer, against
        how many should have. “Changed” counts the questions where the
        second range differed from the first.</p>`,
    );
  }

  // Each earlier question has a field for its answer, for one looked up
  // afterwards. The list is only redrawn when it changes, so that a
  // player answering the current question does not wipe what the host is
  // typing.
  let drawnHistory = "";
  byId("history").addEventListener("submit", (event) => {
    event.preventDefault();
    const form = event.target as HTMLFormElement;
    const field = form.querySelector("input") as HTMLInputElement;
    const given = field.valueAsNumber;
    api("PUT", at("/answer"), {
      question: Number(form.dataset.index),
      answer: isFinite(given) ? given : null,
    })
      .then(() => fail(""))
      .catch(failed);
  });

  function drawHistory(view: HostView): void {
    const drawing = JSON.stringify([view.history, view.confidence]);
    if (drawing === drawnHistory) return;
    drawnHistory = drawing;
    if (view.history.length === 0) {
      html("history", "");
      return;
    }
    html(
      "history",
      `<h3>Asked already</h3>${view.history
        .map(
          (past) => `<div class="past">
        <p>${esc(past.question)}</p>
        <p class="note">${
          past.answer === null
            ? "No answer yet."
            : `The answer: ${esc(num(past.answer))}${past.unit ? ` ${esc(past.unit)}` : ""}.`
        }${
          past.hits === null
            ? ""
            : ` ${past.hits_before === null ? "" : `${past.hits_before} → `}${past.hits} of ${past.n} ranges contained it${
                past.confidence === view.confidence ? "" : `, asked at ${past.confidence}%`
              }.`
        }</p>
        <form class="past-answer" data-index="${past.index}">
          <span class="signed"><input type="number" step="any" inputmode="decimal"
            aria-label="The answer" placeholder="The answer"
            value="${past.answer === null ? "" : past.answer}"><button type="button" class="sign" aria-label="Make it negative" aria-pressed="false">±</button></span>
          <button class="button" type="submit">${past.answer === null ? "Save" : "Change"}</button>
        </form>
      </div>`,
        )
        .join("")}`,
    );
  }

  function draw(view: HostView): void {
    unit = view.round ? view.round.unit : "";
    const round = view.round;
    open = round && round.stage !== "revealed" ? round : null;
    drawRoom(view);
    hide("asked", round === null);
    // A button that is asking "Tap again" keeps asking while ranges come
    // in, and stops if the question was revealed some other way.
    label(byId("ask-button"), open ? "Reveal this one and ask" : "Ask the room", open !== null);
    drawQueue(view);
    if (round) {
      text("h-state", STAGES[round.stage]);
      text("h-question", round.question);
      drawWaiting(view, round);
      // In the first pass the host can move to the discussion; in the
      // discussion, on to the second pass or back to the first; in the
      // second pass, back to the discussion.
      hide("to-discussion", round.stage !== "first" && round.stage !== "second");
      text(
        "to-discussion",
        round.stage === "second" ? "Back to the discussion" : "Stop and talk it over",
      );
      hide("to-second", round.stage !== "discussion");
      hide("to-first", round.stage !== "discussion");
      hide("reveal", round.stage === "revealed");
      // The answer can be entered when the question is asked, typed in
      // while it is being answered, or corrected after the reveal. It
      // stays editable afterwards too, under "Asked already".
      hide("answer-form", false);
      if (answerFor !== round.asked) {
        answerFor = round.asked;
        setValue("true-answer", round.answer);
        unsign(byId("answer-form"));
      }
      text(
        "answer-label",
        round.answer === null ? "The answer" : "The answer, which you can still change",
      );
      drawNumbers(view);
      drawRanges(view);
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
  /** Which pass the fields were last filled in for. */
  let filledFor = "";
  let last: PlayerView | null = null;

  /** What this player has sent in the pass being taken. */
  const sent = (view: PlayerView): Bound | null =>
    view.round?.stage === "second" ? view.mine_after : view.mine_before;

  /** Whether your range is in, and whether you can still change it. */
  function drawStatus(): void {
    const view = last;
    const round = view?.round;
    const send = byId<HTMLButtonElement>("send");
    if (!view || !round || round.stage === "revealed") {
      sentState("status", null);
      return;
    }
    if (round.stage === "discussion") {
      const first = view.mine_before;
      if (first) sentState("status", "locked", `🔒 Locked in: ${ranged(first)}`, "Talk it over. The host will ask everyone again, and you can change it then.");
      else sentState("status", "missed", "You did not send a range", "Talk it over. The host will ask everyone again.");
      return;
    }
    const mine = sent(view);
    const low = value("low");
    const high = value("high");
    // The typed numbers read back with separators and the unit, so that a
    // zero too many or too few shows before it is sent.
    text(
      "preview",
      low === null && high === null
        ? ""
        : `That is ${low === null ? "…" : num(Math.min(low, high ?? low))} – ${
            high === null ? "…" : withUnit(Math.max(high, low ?? high))
          }`,
    );
    const same = mine !== null && low === mine.low && high === mine.high;
    send.classList.toggle("done", same);
    send.disabled = same;
    const pass = round.stage === "second" ? "new range" : "range";
    if (mine === null) {
      sentState(
        "status",
        "waiting",
        "Not sent yet",
        round.stage === "second" && view.mine_before
          ? "Your first range is filled in. Press Send to keep it, or change it first. It only counts once sent."
          : `Your ${pass} only counts once you press Send.`,
      );
      send.textContent = round.stage === "second" ? "Send the new range" : "Send";
    } else if (same) {
      sentState("status", "sent", `✓ Sent: ${ranged(mine)}`, "You can still change it until the host moves on.");
      send.textContent = "Sent ✓";
    } else {
      sentState("status", "changed", "Your change is not sent yet", `The room still has ${ranged(mine)} from you. Press Send to replace it.`);
      send.textContent = "Send the change";
    }
  }

  for (const id of ["low", "high"]) byId(id).addEventListener("input", drawStatus);

  onSubmit("bounds-form", async () => {
    const low = value("low");
    const high = value("high");
    if (low === null || high === null) {
      fail("Both bounds need a number.");
      return;
    }
    const body = await api<{ view: PlayerView }>("PUT", at("/bounds"), {
      low,
      high,
    });
    draw(body.view);
    fail("");
    landed("status");
  });

  /** This player's two answers and the difference between them. */
  function mineHtml(view: PlayerView): string {
    const change = view.mine_changed;
    if (!change) {
      const standing = view.mine_after ?? view.mine_before;
      return standing
        ? facts([["Your range", `<b>${ranged(standing)}</b>`]])
        : "";
    }
    const rows: [string, string][] = [
      ["Your first range", `<b>${ranged(change.before)}</b>`],
      ["After the discussion", `<b>${ranged(change.after)}</b>`],
    ];
    rows.push([
      "Your middle moved",
      change.moved === 0 && change.width_change === 0
        ? "<b>not at all</b>"
        : `<b>${withUnit(change.moved)}<small>${
            change.width_change === 0
              ? "same width"
              : change.width_change < 0
                ? `${num(-change.width_change)} narrower`
                : `${num(change.width_change)} wider`
          }</small></b>`,
    ]);
    if (change.hit_before !== null && change.hit_after !== null) {
      rows.push([
        "Contained the answer",
        `<b>${change.hit_before ? "yes" : "no"} → <span class="${change.hit_after ? "good" : "poor"}">${change.hit_after ? "yes" : "no"}</span></b>`,
      ]);
    }
    return facts(rows);
  }

  function resultHtml(view: PlayerView): string {
    const round = view.round;
    if (!round || round.stage !== "revealed") return "";
    const verdict =
      view.hit === null
        ? ""
        : view.hit
          ? `<p class="good">Your range contained it.</p>`
          : `<p class="poor">Your range missed it.</p>`;
    const answer =
      round.answer === null
        ? `<p class="note">The host has not entered the answer.</p>`
        : `<p class="eyebrow">The answer</p><p class="big">${withUnit(round.answer)}</p>${verdict}`;
    const room = view.standing
      ? `<h3>The room</h3>${statsHtml(view.standing, round.confidence)}`
      : "";
    const shift = view.shift
      ? `<h3>What the discussion did</h3>${shiftHtml(view.shift)}`
      : "";
    const score = view.score;
    const evening =
      score && score.scored && score.width_cost !== null && score.miss_cost !== null
        ? `<h3>Your evening</h3><p class="calibration">${esc(calibrationLine(score))}</p>
           <p class="note">Your score is ${points(score.width_cost + score.miss_cost)}:
           ${points(score.width_cost)} for the width of your ranges and
           ${points(score.miss_cost)} for the misses. Lower is better. The
           discussion moved you on ${plural(score.changed_mind, "question", "questions")}.</p>`
        : "";
    const chart = view.reveal ? rangeChart(view.reveal, round.unit, false) : null;
    const picture = chart ? `<h3>Everybody's ranges</h3>${chart.outerHTML}` : "";
    return `<div class="panel">${answer}${mineHtml(view)}${picture}${shift}${room}${evening}</div>`;
  }

  function draw(view: PlayerView): void {
    last = view;
    unit = view.round ? view.round.unit : "";
    html("you", `<span class="dot"></span> You are ${esc(view.name)}`);
    const round = view.round;
    text("where", `${round ? round.confidence : view.confidence}% sure`);
    if (!round) {
      text("p-state", "Waiting");
      text("p-question", "Waiting for the first question");
      text("p-unit", "");
      hide("bounds-form", true);
      sentState("status", null);
      text("mine", "");
      html("p-result", "");
      return;
    }

    const taking = round.stage === "first" || round.stage === "second";
    text("p-state", STAGES[round.stage]);
    text("p-question", round.question);
    text(
      "p-unit",
      (round.unit ? `Answer in ${round.unit}. ` : "") +
        (taking
          ? `Give the range you are ${round.confidence}% sure about.`
          : ""),
    );
    hide("bounds-form", !taking);

    // The second pass starts from the first answer, so that submitting
    // it unchanged counts as an answer.
    const fillFor = `${round.asked}:${round.stage}`;
    if (taking && filledFor !== fillFor) {
      filledFor = fillFor;
      const from = round.stage === "second" ? view.mine_before : null;
      const mine = (round.stage === "second" ? view.mine_after : view.mine_before) ?? from;
      setValue("low", mine ? mine.low : null);
      setValue("high", mine ? mine.high : null);
      unsign(byId("bounds-form"));
    }

    drawStatus();
    text(
      "mine",
      taking ? `${view.submitted} of ${plural(view.players, "person", "people")} have sent a range.` : "",
    );
    if (round.stage === "second" && view.mine_before) {
      text(
        "mine-before",
        `You said ${ranged(view.mine_before)} before the discussion.`,
      );
    }
    hide("mine-before", round.stage !== "second" || !view.mine_before);

    html("p-result", resultHtml(view));
  }

  // Fetches the room once, then keeps it up to date from its event
  // stream. The stream closes when the host takes this player out, and
  // then the link is gone: the question goes too, so nobody types a
  // range into a screen that can no longer send one.
  showConnection(
    watch<PlayerView>(
      at(""),
      draw,
      "You are not in this room. The host may have taken you out, or the room has finished. Open the join link again to rejoin.",
    ),
  );
  void keepAwake();
}

// ------------------------------------------------------------------- wall

/** One range in the reveal's picture. Nobody's name is sent with it. */
interface ChartRange {
  low: number;
  high: number;
  hit: boolean | null;
  /** Their first range, where this is their second. */
  first: [number, number] | null;
  /** Only on a player's phone: this one is theirs. */
  mine?: boolean;
}

/** A revealed question, for the picture: see `view::Reveal`. */
interface Reveal {
  answer: number | null;
  ranges: ChartRange[];
  /** The median lower and upper bound. */
  middle: [number, number] | null;
  hits: number | null;
  twice: {
    n: number;
    moved: number;
    hits_before: number | null;
    hits_after: number | null;
  } | null;
}

/** What the server puts on the wall: see `view::wall`. Before the
 *  reveal there is no answer and no range in it to show. */
interface WallGame {
  confidence: number;
  round: {
    question: string;
    unit: string;
    confidence: number;
    stage: Stage;
    sent: number;
    of: number;
    reveal: Reveal | null;
  } | null;
  top: { name: string; hits: number; scored: number; cost: number }[];
}

/** Where a value sits across the chart, from 0 to 1. A log scale when
 *  everything is above zero and the values span two orders of magnitude
 *  or more, so one guess of a billion does not squash the rest into a
 *  line. */
function scale(values: number[]): { at: (v: number) => number; log: boolean; lo: number; hi: number } {
  const lo = Math.min(...values);
  const hi = Math.max(...values);
  const log = lo > 0 && hi / lo >= 100;
  const f = log ? Math.log10 : (v: number) => v;
  const a = f(lo);
  const b = f(hi);
  const pad = b === a ? 1 : (b - a) * 0.04;
  return { at: (v: number) => (f(v) - (a - pad)) / (b - a + 2 * pad), log, lo, hi };
}

/** The ranges as bars against the answer, by midpoint, unnamed. A
 *  second range is drawn over a faint copy of the first. `wide` is the
 *  wall's: wider, with more room for rows; otherwise it is sized for a
 *  phone or the host's column, with a line saying what the colours are. */
function rangeChart(reveal: Reveal, unitName: string, wide: boolean): Element | null {
  const ranges = reveal.ranges;
  if (ranges.length === 0) return null;
  const values = ranges.flatMap((r) => [r.low, r.high, ...(r.first ?? [])]);
  if (reveal.answer !== null) values.push(reveal.answer);
  const { at, log, lo, hi } = scale(values);
  const width = wide ? 1000 : 600;
  const row = Math.max(4, Math.min(wide ? 28 : 40, (wide ? 420 : 360) / ranges.length));
  const bar = Math.max(2, row * 0.6);
  const top = 8;
  const base = top + row * ranges.length + 6;
  const height = base + 40;
  const x = (v: number) => (at(v) * width).toFixed(1);
  const rect = (low: number, high: number, y: number, kind: string) => {
    const x1 = at(low) * width;
    const x2 = Math.max(at(high) * width, x1 + 3);
    return `<rect class="chart-bar ${kind}" x="${x1.toFixed(1)}" y="${y.toFixed(1)}" width="${(x2 - x1).toFixed(1)}" height="${bar.toFixed(1)}" rx="${Math.min(4, bar / 2).toFixed(1)}"/>`;
  };
  const parts: string[] = [];
  ranges.forEach((r, i) => {
    const y = top + i * row + (row - bar) / 2;
    if (r.first) parts.push(rect(r.first[0], r.first[1], y, "first"));
    const kind = r.hit === null ? "open" : r.hit ? "hit" : "miss";
    parts.push(rect(r.low, r.high, y, `${kind}${r.mine ? " mine" : ""}`));
  });
  parts.push(`<line class="chart-axis" x1="0" x2="${width}" y1="${base}" y2="${base}"/>`);
  const tick = (v: number, anchor: string) =>
    `<text class="chart-tick" x="${x(v)}" y="${base + 32}" text-anchor="${anchor}">${esc(num(v))}</text>`;
  parts.push(tick(lo, "start"), tick(hi, "end"));
  if (log) {
    parts.push(
      `<text class="chart-tick" x="${width / 2}" y="${base + 32}" text-anchor="middle">log scale</text>`,
    );
  }
  if (reveal.answer !== null) {
    const ax = x(reveal.answer);
    parts.push(`<line class="chart-answer" x1="${ax}" x2="${ax}" y1="0" y2="${base}"/>`);
  }
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
  svg.setAttribute("class", wide ? "range-chart wall-chart" : "range-chart");
  svg.setAttribute("role", "img");
  const mine = ranges.find((r) => r.mine);
  svg.setAttribute(
    "aria-label",
    `${plural(ranges.length, "range", "ranges")}${
      reveal.answer === null ? "" : ` against the answer, ${num(reveal.answer)} ${unitName}`
    }${log ? ", on a log scale" : ""}${mine ? `; yours is ${num(mine.low)} to ${num(mine.high)}` : ""}`,
  );
  svg.innerHTML = parts.join("");
  if (wide) return svg;
  const key: string[] = [];
  if (reveal.answer !== null) key.push("the line is the answer", "green held it, red missed");
  if (mine) key.push("yours is outlined");
  if (ranges.some((r) => r.first)) key.push("the faint bars are first ranges");
  const figure = el("figure", { className: "chart" }, svg);
  if (key.length > 0) {
    const said = key.join(", ");
    figure.append(el("figcaption", { className: "note" }, `${said[0].toUpperCase()}${said.slice(1)}.`));
  }
  return figure;
}

/** The game's part of the wall. Large, for the back of the room. Names
 *  appear only in the top of the board, never beside a range. */
function drawWall(game: WallGame, slot: HTMLElement): void {
  const shape = JSON.stringify(game);
  if (slot.dataset.shape === shape) return;
  slot.dataset.shape = shape;
  const round = game.round;
  if (!round) {
    slot.replaceChildren(
      el("p", { className: "eyebrow wall-idle" }, `${game.confidence}% sure`),
      el("p", { className: "wall-question wall-idle" }, "Waiting for the first question"),
    );
    return;
  }
  const children: Node[] = [
    el(
      "p",
      { className: "eyebrow" },
      round.stage === "revealed"
        ? `${STAGES[round.stage]} · ranges at ${round.confidence}%`
        : `${STAGES[round.stage]} · give a range you are ${round.confidence}% sure of`,
    ),
    el("p", { className: "wall-question" }, round.question),
  ];
  const reveal = round.reveal;
  if (round.unit && !reveal) children.push(el("p", { className: "wall-unit" }, `Answer in ${round.unit}`));
  if (!reveal) {
    children.push(
      el("p", { className: "wall-count-big" }, `${round.sent} of ${round.of}`),
      el(
        "p",
        { className: "wall-unit" },
        round.stage === "discussion"
          ? "sent a first range. Talk it over."
          : round.stage === "second"
            ? "have sent a second range"
            : "have sent a range",
      ),
    );
    slot.replaceChildren(...children);
    return;
  }
  const withIt = (v: number) => (round.unit ? `${num(v)} ${round.unit}` : num(v));
  children.push(
    el(
      "p",
      { className: "wall-answer-big" },
      el("span", { className: "eyebrow" }, "The answer "),
      reveal.answer === null ? "not entered yet" : withIt(reveal.answer),
    ),
  );
  // The bars first, so they are on screen without scrolling; the
  // counts under them.
  const chart = rangeChart(reveal, round.unit, true);
  if (chart) children.push(chart);
  const said: string[] = [];
  const n = reveal.ranges.length;
  if (reveal.hits !== null && n > 0) {
    const expected = Math.round((n * round.confidence) / 100);
    said.push(
      `${reveal.hits} of ${plural(n, "range", "ranges")} contained it; at ${round.confidence}%, about ${expected} should.`,
    );
  }
  const twice = reveal.twice;
  if (twice && twice.hits_before !== null && twice.hits_after !== null) {
    said.push(
      `Of the ${twice.n} who answered twice, ${twice.hits_before} → ${twice.hits_after} contained it, and ${twice.moved} changed their range.`,
    );
  } else if (reveal.middle) {
    said.push(`The middle of the room: ${num(reveal.middle[0])} – ${withIt(reveal.middle[1])}.`);
  }
  if (said.length > 0) children.push(el("p", { className: "wall-line" }, said.join(" ")));
  if (game.top.length > 0) {
    children.push(
      el(
        "ol",
        { className: "wall-top" },
        el("li", { className: "eyebrow" }, "Top of the board"),
        ...game.top.map((t) =>
          el("li", {}, el("b", {}, t.name), ` ${points(t.cost)} · ${t.hits} of ${t.scored}`),
        ),
      ),
    );
  }
  slot.replaceChildren(...children);
}

// ------------------------------------------------------------ the sign

// The decimal keypad on an iPhone has no minus key, so each number field
// has a ± button beside it. It flips the sign of what is in the field.
// On an empty field there is nothing to flip yet: the button stays
// pressed, and the first number typed is made negative.
function signOf(button: HTMLButtonElement): HTMLInputElement | null {
  const field = button.previousElementSibling;
  return field instanceof HTMLInputElement ? field : null;
}

/** Lets go of every pressed ± in `root`, for fields the page fills or
 *  empties itself. */
function unsign(root: ParentNode): void {
  root.querySelectorAll<HTMLButtonElement>("button.sign").forEach((button) => arm(button, false));
}

function arm(button: HTMLButtonElement, armed: boolean): void {
  button.setAttribute("aria-pressed", String(armed));
  const field = signOf(button);
  if (!field) return;
  if (armed) {
    field.dataset.placeholder ??= field.placeholder;
    field.placeholder = "−";
  } else if (field.dataset.placeholder !== undefined) {
    field.placeholder = field.dataset.placeholder;
    delete field.dataset.placeholder;
  }
}

document.addEventListener("click", (event) => {
  const button = (event.target as Element | null)?.closest?.("button.sign");
  if (!(button instanceof HTMLButtonElement)) return;
  const field = signOf(button);
  if (!field) return;
  const given = field.valueAsNumber;
  if (isFinite(given) && given !== 0) {
    field.value = String(-given);
    arm(button, false);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  } else {
    arm(button, button.getAttribute("aria-pressed") !== "true");
  }
  field.focus();
});

document.addEventListener("input", (event) => {
  const field = event.target;
  if (!(field instanceof HTMLInputElement)) return;
  const button = field.nextElementSibling;
  if (!(button instanceof HTMLButtonElement) || !button.classList.contains("sign")) return;
  if (button.getAttribute("aria-pressed") !== "true") return;
  const given = field.valueAsNumber;
  if (isFinite(given) && given > 0) {
    arm(button, false);
    field.value = String(-given);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  }
});

const where = route();
if (where.kind === "host") host(where.key);
else if (where.kind === "join") joining("estimate", where.key);
else if (where.kind === "player") player(where.key);
else if (where.kind === "wall") wall<WallGame>(where.key, drawWall);
else opening();
