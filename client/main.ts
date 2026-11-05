// Speed updating, in a browser.
//
// One page with five screens: the one that opens a room, the
// facilitator's, the one that joins, the player's, which is almost
// always a phone, and the wall, which is a projector.

import {
  type Feed,
  type Present,
  api,
  byId,
  confirmTap,
  connectedCount,
  countdown,
  el,
  fail,
  here,
  failed,
  hide,
  joinLink,
  joining,
  keepAwake,
  landed,
  onClick,
  onSubmit,
  plural,
  roster,
  route,
  persist,
  sentBox,
  shareLink,
  show,
  stored,
  showConnection,
  text,
  wall,
  wallButton,
  watch,
} from "./rooms";

type Mode = "bingo" | "open" | "free" | "pick";

/** Set once a phone has dismissed how the evening works. Kept in the
 *  browser: it only saves somebody reading it twice. */
const EXPLAINED = "updating:explained";
type Scale = "written" | "agree" | "chance";
type Phase = "picking" | "answering" | "warmup" | "talking" | "updating" | "finished";
type Stage = "before" | "clue" | "revealed";

interface Statement {
  id: number;
  text: string;
  topic: string;
  chance: boolean;
}

interface TopicChoice {
  key: string;
  name: string;
  size: number;
  chosen: boolean;
}

interface Said {
  name: string;
  before: number;
  after: number | null;
  done: boolean;
}

interface GroupView {
  number: number;
  spot: string;
  statement: Statement;
  people: Said[];
  gap: number;
  settled: boolean;
  closed: number | null;
}

interface RoundView {
  number: number;
  groups: GroupView[];
  waiting: string[];
}

interface Score {
  together: number;
  listening: number;
  kudos: number;
  persuaded: number;
  opened: number;
  best_closed: number;
  total: number;
}

interface Leader extends Score {
  name: string;
}

interface Award {
  title: string;
  why: string;
  names: string[];
  detail: string;
}

interface Calibrated {
  name: string;
  bits: number;
  bot: boolean;
}

interface Outcome {
  truth: boolean;
  answers: number[];
  mean_before: number | null;
  mean_after: number | null;
  bot: number | null;
  room: number | null;
}

interface WarmHost {
  number: number;
  setup: string;
  claim: string;
  prior: number;
  stage: Stage;
  before: number;
  after: number;
  outcome: Outcome | null;
}

interface WarmResult {
  truth: boolean;
  bits_before: number | null;
  bits_after: number | null;
  ideal: number;
  ideal_from_yours: number | null;
}

interface WarmMine {
  number: number;
  setup: string;
  claim: string;
  stage: Stage;
  before: number | null;
  after: number | null;
  clue: string | null;
  result: WarmResult | null;
}

interface Who {
  /** What the Away button sends. */
  id: number;
  name: string;
  /** Sitting rounds out: not paired until they are back. */
  away: boolean;
  score: Score;
  answered: number;
  of: number;
  updated: number;
  moved: number;
  lines: number;
}

interface Tally {
  statement: Statement;
  n: number;
  mean: number;
  spread: number;
  first_mean: number;
  updated: number;
}

interface Picked {
  statement: Statement;
  n: number;
}

interface Picking {
  pickers: number;
  limit: number;
  picked: Picked[];
}

interface HostView {
  join: string;
  /** The wall's key, for `/w/<key>`. */
  wall: string;
  people: Present[];
  mode: Mode;
  scale: Scale;
  topics: TopicChoice[];
  custom: string;
  side: number;
  sides: number[];
  minutes: number;
  lengths: number[];
  deck: number;
  locked: boolean;
  phase: Phase;
  ends: number;
  round: RoundView | null;
  rounds: number;
  players: Who[];
  contested: Tally[];
  shifted: Tally[];
  popular: Tally[];
  answers: number;
  updates: number;
  leaders: Leader[];
  kudos_points: number;
  awards: Award[];
  warmup: WarmHost | null;
  warmups: number;
  calibrated: Calibrated[];
  picking: Picking | null;
}

interface Seen {
  name: string;
  answer: number;
}

interface Square {
  statement: Statement;
  answer: number | null;
  first: number | null;
  marked: boolean;
  picked: boolean;
  count: number;
  others: Seen[];
}

interface Partner {
  name: string;
  answer: number;
  /** Only once your own guess and answer are in. */
  after: number | null;
  guess: number | null;
  kudos: boolean;
  /** Whether they are done too, so their answer is final. */
  done: boolean;
}

interface Mine {
  number: number;
  spot: string;
  statement: Statement;
  partners: Partner[];
  before: number;
  after: number | null;
  /** Your answer and guesses are in: the answer stays as the round's. */
  done: boolean;
}

interface Opposite {
  name: string;
  statement: Statement;
  mine: number;
  theirs: number;
  gap: number;
}

interface PlayerView {
  name: string;
  /** Sitting rounds out: not paired until back, or until they answer. */
  away: boolean;
  mode: Mode;
  side: number;
  phase: Phase;
  ends: number;
  round: number;
  squares: Square[];
  topics: [string, string][];
  group: Mine | null;
  waiting: boolean;
  opposites: Opposite[];
  lines: number;
  updated: number;
  moved: number;
  players: number;
  score: Score;
  rank: number | null;
  winners: Leader[];
  awards: Award[];
  warmup: WarmMine | null;
  calibration: [number, number] | null;
  next: number[];
  picks: number;
}

/** What each mode is called on the facilitator's screen. */
const MODES: Record<Mode, (side: number) => string> = {
  bingo: (side) => `A ${side}×${side} bingo card each`,
  open: () => "Answer anything",
  free: () => "Free for all",
  pick: () => "Everyone picks the statements",
};

const PHASES: Record<Phase, string> = {
  picking: "Picking",
  answering: "Answering",
  warmup: "Warm-up",
  talking: "Talking",
  updating: "Where do you stand now?",
  finished: "Finished",
};

/** The seven points of the agreement scale, 1 to 7. */
const AGREE = [
  "strongly disagree",
  "disagree",
  "somewhat disagree",
  "neutral",
  "somewhat agree",
  "agree",
  "strongly agree",
];

/** An answer in words: "6 · agree" or "35%". */
function said(statement: Statement, value: number | null): string {
  if (value === null) return "not answered";
  if (statement.chance) return value === 50 ? "50% · no idea either way" : `${value}%`;
  return `${value} · ${AGREE[Math.min(7, Math.max(1, Math.round(value))) - 1]}`;
}

/** The same, as short as it goes: "6" or "35%". */
function brief(statement: Statement, value: number): string {
  return statement.chance ? `${value}%` : String(value);
}

/** Where an answer sits between its two ends, 0 to 1, for colouring. */
function lean(statement: Statement, value: number): number {
  return statement.chance ? value / 100 : (value - 1) / 6;
}

/** Bits, signed: "+0.48 bits". */
function bitsText(bits: number | null): string {
  if (bits === null) return "no answer";
  return `${bits >= 0 ? "+" : "−"}${Math.abs(bits).toFixed(2)} bits`;
}

/** A probability as a whole percent. */
function pct(p: number): string {
  return `${Math.round(p * 100)}%`;
}

/** What somebody's evening came to, in a sentence. */
function scoreText(score: Score): string {
  if (!score.total) return "No points yet.";
  const parts = [
    score.together ? `${score.together} for closing gaps` : "",
    score.listening ? `${score.listening} for guessing your partners` : "",
    score.kudos ? `${score.kudos} × “learned something”` : "",
  ].filter((part) => part !== "");
  return `${score.total} points: ${parts.join(", ")}.`;
}

/** The prizes, as cards. */
function awardCards(awards: Award[]): HTMLElement {
  return el(
    "div",
    { className: "awards" },
    ...awards.map((a) =>
      el(
        "div",
        { className: "award" },
        el("p", { className: "eyebrow" }, a.title),
        el("p", { className: "award-names" }, a.names.join(" & ")),
        el("p", { className: "note" }, `${a.why} · ${a.detail}`),
      ),
    ),
  );
}

/** A 0–100% slider that reports as it moves and sends when let go. */
function percentSlider(start: number, unset: boolean, onSend: (v: number) => void): [HTMLInputElement, HTMLElement] {
  const value = el("span", { className: "value big-value" }, unset ? "not answered" : `${start}%`);
  const input = el("input", { type: "range", min: "0", max: "100", step: "1", className: `scale${unset ? " unset" : ""}` });
  input.value = String(start);
  input.addEventListener("input", () => {
    input.classList.remove("unset");
    value.textContent = `${input.value}%`;
  });
  onRelease(input, onSend);
  return [input, value];
}

/** Calls `onSend` when a slider is let go, and `onEnd` whenever the
 *  finger leaves it.
 *
 *  A range input fires no change event when it is let go where it
 *  started. Without the release below, a slider tapped where its thumb
 *  already sits, which is 50% or 4 on one not answered yet, would never
 *  send, and whatever waits for the change, like a panel that is not
 *  redrawn under a finger, would wait for good. A touch the browser takes
 *  over to scroll the page ends the hold and sends nothing. */
function onRelease(input: HTMLInputElement, onSend: (v: number) => void, onEnd: () => void = () => {}): void {
  let down = false;
  input.addEventListener("pointerdown", () => {
    down = true;
  });
  input.addEventListener("pointercancel", () => {
    down = false;
    onEnd();
  });
  input.addEventListener("change", () => {
    down = false;
    onEnd();
    onSend(Number(input.value));
  });
  input.addEventListener("pointerup", () => {
    // A change, when there is one, comes with the same release, so this
    // waits for it and only sends when there was none.
    setTimeout(() => {
      if (!down) return;
      down = false;
      input.dispatchEvent(new Event("input"));
      onEnd();
      onSend(Number(input.value));
    }, 0);
  });
}

/** Runs a countdown off the server's clock into every element given. */
function ticking(now: () => number, ends: () => number | null, into: () => HTMLElement[]): void {
  setInterval(() => {
    const end = ends();
    for (const node of into()) {
      node.textContent = end === null ? "" : countdown(end - now());
    }
  }, 250);
}

/** A suggestion form, which anybody can use and nobody reads back. */
function suggestions(form: string, field: string, chance: string, done: string): void {
  onSubmit(form, async () => {
    const input = byId<HTMLInputElement>(field);
    await api("POST", "/api/suggest", {
      text: input.value,
      chance: byId<HTMLInputElement>(chance).checked,
    });
    input.value = "";
    byId<HTMLInputElement>(chance).checked = false;
    hide(done, false);
    fail("");
  });
}

/** Pressed-state buttons, one of which is chosen. */
function chips<T>(
  id: string,
  options: { value: T; label: string; on: boolean }[],
  pick: (value: T) => void,
  disabled = false,
): void {
  const box = byId(id);
  box.replaceChildren(
    ...options.map((o) => {
      const b = el("button", { type: "button", className: "chip", disabled }, o.label);
      b.setAttribute("aria-pressed", String(o.on));
      b.addEventListener("click", () => pick(o.value));
      return b;
    }),
  );
}

// ---------------------------------------------------------------- start

function start(): void {
  show("start");
  onClick("open", async () => {
    const keys = await api<{ host: string; join: string }>("POST", "/api/rooms");
    location.href = here(`/h/${keys.host}`);
  });
  suggestions("suggest-start", "idea-start", "idea-start-chance", "idea-start-done");
}

// ----------------------------------------------------------------- host

function host(key: string): void {
  show("host");
  // Wide, for a laptop; the projector has a screen of its own, the wall.
  document.body.classList.add("wall-screen");
  const at = (path: string): string => `/api/h/${key}${path}`;
  let current: HostView | null = null;
  /** The setup as the facilitator is editing it, before it is sent. */
  let draft: { mode: Mode; scale: Scale; topics: string[]; side: number } | null = null;

  const send = async (method: string, path: string, body?: unknown): Promise<void> => {
    const result = await api<{ now: number; view: HostView }>(method, at(path), body);
    feed.accept(result);
    fail("");
  };

  const drawSetup = (view: HostView): void => {
    hide("setup", view.locked);
    hide("locked", !view.locked);
    const chosen = view.topics.filter((t) => t.chosen).map((t) => t.name);
    text(
      "locked-note",
      `${MODES[view.mode](view.side)}, from ${plural(view.deck, "statement", "statements")}: ${chosen.join(", ") || "your own"}. People have started answering, so this stays as it is.`,
    );
    if (view.locked) return;
    draft ??= {
      mode: view.mode,
      scale: view.scale,
      topics: view.topics.filter((t) => t.chosen).map((t) => t.key),
      side: view.side,
    };
    const d = draft;
    chips<Mode>(
      "modes",
      [
        { value: "bingo", label: "Bingo card", on: d.mode === "bingo" },
        { value: "open", label: "Answer anything", on: d.mode === "open" },
        { value: "free", label: "Free for all", on: d.mode === "free" },
        { value: "pick", label: "Everyone picks", on: d.mode === "pick" },
      ],
      (mode) => {
        d.mode = mode;
        drawSetup(view);
      },
    );
    text(
      "mode-note",
      d.mode === "bingo"
        ? "Everyone is dealt a grid from the topics below. What they talk about is marked; a full line is a bingo."
        : d.mode === "open"
          ? "Everyone sees every statement from the topics below and answers as many as they like."
          : d.mode === "pick"
            ? "Before anyone answers, everyone picks up to three statements from the topics below that they would like to talk about. When you close the picks, the room answers every statement somebody picked, the most picked first."
            : "Everyone browses every statement on their own time. Each shows how many people have answered it, the most answered first, and who said what once you have answered it yourself. Phones point people at whoever disagrees with them most; rounds are optional.",
    );
    chips<Scale>(
      "scales",
      [
        { value: "written", label: "As written", on: d.scale === "written" },
        { value: "agree", label: "All agree / disagree, 1–7", on: d.scale === "agree" },
        { value: "chance", label: "All probabilities", on: d.scale === "chance" },
      ],
      (scale) => {
        d.scale = scale;
        drawSetup(view);
      },
    );
    text(
      "scale-note",
      d.scale === "written"
        ? "Most statements are agree or disagree, from 1 to 7; questions starting “What is the chance” ask for a probability."
        : d.scale === "agree"
          ? "Every statement is agree or disagree. Probability questions become claims: “A fish feels pain.”"
          : "Every statement is answered with how confident people are that it is true, from 0 to 100%, where 50% means no idea either way. Recommended: a probability means the same to everybody.",
    );
    chips<string>(
      "topics",
      view.topics.map((t) => ({
        value: t.key,
        label: `${t.name} (${t.size})`,
        on: d.topics.includes(t.key),
      })),
      (topic) => {
        d.topics = d.topics.includes(topic)
          ? d.topics.filter((k) => k !== topic)
          : [...d.topics, topic];
        drawSetup(view);
      },
    );
    hide("sides-set", d.mode !== "bingo");
    chips<number>(
      "sides",
      view.sides.map((s) => ({ value: s, label: `${s}×${s}`, on: d.side === s })),
      (side) => {
        d.side = side;
        drawSetup(view);
      },
    );
    const custom = byId<HTMLTextAreaElement>("custom");
    if (document.activeElement !== custom && custom.dataset.sent !== view.custom) {
      custom.value = view.custom;
      custom.dataset.sent = view.custom;
    }
    text(
      "setup-note",
      `In use now: ${MODES[view.mode](view.side).toLowerCase()}, ${plural(view.deck, "statement", "statements")}. This can change until somebody answers.`,
    );
  };

  onSubmit("setup", async () => {
    if (!draft) return;
    await send("PUT", "/setup", {
      ...draft,
      custom: byId<HTMLTextAreaElement>("custom").value,
    });
  });

  /** A distance on the shared scale, in the statement's own units. */
  const gapWords = (statement: Statement, gap: number): string =>
    statement.chance
      ? `${Math.round(gap / 2)} points`
      : plural(Math.round((gap * 3) / 100), "step", "steps");

  const gapNote = (g: GroupView): string => `${gapWords(g.statement, g.gap)} apart`;

  /** One pair, as the wall shows it: where they meet, who, and on what. */
  const pairCard = (g: GroupView, phase: Phase): HTMLElement =>
    el(
      "div",
      { className: `pair${g.settled ? " settled" : ""}` },
      el("p", { className: "pair-spot" }, g.spot),
      el(
        "p",
        { className: "pair-names" },
        ...g.people.flatMap((p, i) => [
          i ? " · " : "",
          el("span", { className: phase === "updating" && p.done ? "done" : "" }, p.name),
        ]),
      ),
      el("p", { className: "pair-statement" }, g.statement.text),
      el(
        "p",
        { className: "note" },
        g.settled && g.closed !== null
          ? `${g.people.map((p) => `${said(g.statement, p.before)} → ${said(g.statement, p.after)}`).join(" · ")} · ${
              g.closed ? `+${g.closed} points each` : "no closer"
            }`
          : `${g.people.map((p) => said(g.statement, p.before)).join(" vs ")} · ${gapNote(g)}`,
      ),
    );

  /** The room's answers to a warm-up question, as dots along 0–100%. */
  const dots = (answers: number[], truth: boolean): HTMLElement =>
    el(
      "div",
      { className: "dots" },
      el("span", { className: `dots-end${truth ? "" : " right"}` }, "0% · false"),
      el("span", { className: `dots-end${truth ? " right" : ""}` }, "true · 100%"),
      ...answers.map((a) => {
        const d = el("i", {});
        d.style.left = `${a}%`;
        return d;
      }),
    );

  /** The top of the board, large. */
  const topBoard = (view: HostView, n: number): HTMLElement | null =>
    view.leaders.length
      ? el(
          "ol",
          { className: "wall-board" },
          ...view.leaders.slice(0, n).map((l) =>
            el("li", {}, el("b", {}, l.name), el("span", {}, `${l.total}`)),
          ),
        )
      : null;

  /** The part of the screen meant for the wall, by what is going on. */
  const drawWall = (view: HostView): void => {
    const wall = byId("wall");
    const w = view.warmup;
    if (view.phase === "warmup" && w) {
      const o = w.outcome;
      wall.replaceChildren(
        el("p", { className: "eyebrow" }, `Warm-up ${w.number} · a question with a right answer`),
        el("p", { className: "wall-setup" }, w.setup),
        el("p", { className: "wall-claim" }, w.claim),
        o
          ? el(
              "div",
              {},
              el("p", { className: `wall-verdict ${o.truth ? "good" : "poor"}` }, o.truth ? "True" : "False"),
              dots(o.answers, o.truth),
              el(
                "p",
                { className: "wall-line" },
                `The room said ${o.mean_before === null ? "–" : `${Math.round(o.mean_before)}%`} before the clues, ${
                  o.mean_after === null ? "–" : `${Math.round(o.mean_after)}%`
                } after. The room scored ${bitsText(o.room)} on average; the Bayes bot ${bitsText(o.bot)}.`,
              ),
              view.calibrated.length
                ? el(
                    "ol",
                    { className: "wall-board" },
                    ...view.calibrated.slice(0, 6).map((c) =>
                      el("li", { className: c.bot ? "bot" : "" }, el("b", {}, c.bot ? "🤖 Bayes bot" : c.name), el("span", {}, bitsText(c.bits))),
                    ),
                  )
                : "",
            )
          : el(
              "p",
              { className: "wall-line" },
              w.stage === "before"
                ? `How likely is it, from the setup alone? ${w.before} of ${plural(view.players.length, "person has", "people have")} answered.`
                : `Clues are on your phones. Compare with your neighbours, then answer again. ${w.after} of ${view.players.length} answered.`,
            ),
      );
      return;
    }
    if (view.round && (view.phase === "talking" || view.phase === "updating")) {
      const talking = view.phase === "talking";
      const done = view.round.groups.filter((g) => g.settled).length;
      wall.replaceChildren(
        el(
          "div",
          { className: "wall-head" },
          el("p", { className: "eyebrow" }, `Round ${view.round.number}`),
          talking
            ? el("p", { className: "clock wall-clock", id: "h-clock" })
            : el(
                "p",
                { className: "wall-line" },
                `Phones out: say where you stand, guess where your partner landed, and if you learned something. ${done} of ${plural(view.round.groups.length, "pair", "pairs")} done.`,
              ),
        ),
        el("div", { className: "pairs" }, ...view.round.groups.map((g) => pairCard(g, view.phase))),
        view.round.waiting.length
          ? el("p", { className: "note" }, `Not paired: ${view.round.waiting.join(", ")}. They need to answer statements somebody else has answered.`)
          : "",
      );
      return;
    }
    if (view.phase === "picking" && view.picking) {
      const p = view.picking;
      wall.replaceChildren(
        el("p", { className: "eyebrow" }, "Before we start"),
        el("p", { className: "wall-claim" }, `Pick up to ${p.limit} statements you would like to talk about`),
        el(
          "p",
          { className: "wall-line" },
          `${p.pickers} of ${plural(view.players.length, "person has", "people have")} picked, ${plural(p.picked.length, "statement", "statements")} so far.`,
        ),
        p.picked.length
          ? el(
              "ol",
              { className: "tallies" },
              ...p.picked.slice(0, 8).map((t) =>
                el("li", {}, el("span", {}, t.statement.text), el("small", {}, plural(t.n, "pick", "picks"))),
              ),
            )
          : "",
      );
      return;
    }
    if (view.phase === "finished") {
      wall.replaceChildren(
        el("p", { className: "eyebrow" }, "That was the evening"),
        el("h1", {}, "The prizes"),
        view.awards.length ? awardCards(view.awards) : el("p", { className: "note" }, "No prizes: nobody finished a round."),
        topBoard(view, 5) ? el("h2", {}, "Most points") : "",
        topBoard(view, 5) ?? "",
      );
      return;
    }
    wall.replaceChildren(
      el("p", { className: "eyebrow" }, view.rounds ? `After round ${view.rounds}` : "Speed updating"),
      el("p", { className: "wall-claim" }, view.rounds ? "Answer a few more, the next round is coming" : "Scan the code and say where you stand"),
      el(
        "p",
        { className: "wall-line" },
        `${plural(view.players.length, "person", "people")} in, ${plural(view.answers, "answer", "answers")} so far.`,
      ),
      topBoard(view, 5) ?? "",
    );
  };

  /** The facilitator's buttons, kept to one bar. */
  /** The groups of the round just over that have not all answered and
   *  guessed. Starting the next round, or ending the evening, drops what
   *  they have not sent yet. */
  const unfinished = (view: HostView): number =>
    view.phase === "updating" && view.round ? view.round.groups.filter((g) => !g.settled).length : 0;

  /** Sets a button's words, or, while it is asking to be tapped again,
   *  the words it goes back to. */
  const label = (id: string, words: string): void => {
    const button = byId(id);
    if (button.dataset.armed === "1") button.dataset.said = words;
    else button.textContent = words;
  };

  /** Runs `then`, first asking for a second tap while groups are still
   *  answering. */
  const unlessUnfinished = (id: string, ask: (left: number) => string, then: () => Promise<void>) => async () => {
    const button = byId(id);
    const left = current ? unfinished(current) : 0;
    if ((left || button.dataset.armed === "1") && !confirmTap(button, ask(left))) return;
    await then();
  };

  const drawControls = (view: HostView): void => {
    const talking = view.phase === "talking";
    const warm = view.phase === "warmup" ? view.warmup : null;
    text("h-phase", warm ? `Warm-up ${warm.number}` : PHASES[view.phase]);
    const left = unfinished(view);
    label(
      "start",
      (view.rounds ? `Start round ${view.rounds + 1}` : "Start the first round") +
        (left ? ` · ${plural(left, "pair", "pairs")} still answering` : ""),
    );
    hide("start", talking || view.phase === "finished" || view.phase === "picking" || warm !== null);
    hide("close-picks", view.phase !== "picking");
    hide("stop", !talking);
    hide("extend", !(talking || (view.phase === "updating" && view.rounds > 0)));
    hide("finish", view.phase === "finished" || warm !== null);
    hide("reopen", view.phase !== "finished");
    hide("warm-start", !(view.phase === "answering" || (warm !== null && warm.stage === "revealed")));
    byId("warm-start").textContent = warm ? "Next warm-up question" : view.warmups ? "Another warm-up question" : "Warm-up question";
    hide("warm-next", warm === null || warm.stage === "revealed");
    byId("warm-next").textContent = warm?.stage === "before" ? "Deal the clues" : "Reveal the answer";
    hide("warm-end", warm === null);
    chips<number>(
      "lengths",
      view.lengths.map((m) => ({ value: m, label: `${m} min`, on: m === view.minutes })),
      (minutes) => void send("PUT", "/minutes", { minutes }).catch((e: unknown) => fail(String(e))),
    );
  };

  const tallyList = (title: string, tallies: Tally[], line: (t: Tally) => string): HTMLElement | null =>
    tallies.length
      ? el(
          "section",
          { className: "panel" },
          el("h2", {}, title),
          el(
            "ol",
            { className: "tallies" },
            ...tallies.map((t) =>
              el("li", {}, el("span", {}, t.statement.text), el("small", {}, line(t))),
            ),
          ),
        )
      : null;

  const round1 = (v: number): string => String(Math.round(v));

  /** Marks somebody away, so they are not paired, or back. For people
   *  who left without saying; their phone can say it itself. */
  const awayButton = (p: Who): HTMLElement => {
    const b = el(
      "button",
      { type: "button", className: `chip small${p.away ? " on" : ""}` },
      p.away ? "Away" : "In",
    );
    b.setAttribute("aria-pressed", String(p.away));
    b.setAttribute("aria-label", p.away ? `${p.name} is away. Bring them back` : `Mark ${p.name} away`);
    b.title = p.away ? "Not paired. Tap when they are back." : "Tap if they have left, so they are not paired.";
    b.addEventListener("click", () => {
      b.disabled = true;
      send("PUT", "/away", { id: p.id, away: !p.away })
        .catch(failed)
        .finally(() => {
          b.disabled = false;
        });
    });
    return b;
  };

  const drawBoard = (view: HostView): void => {
    const people = el(
      "section",
      { className: "panel" },
      el("h2", {}, "People"),
      el(
        "p",
        { className: "note" },
        `${plural(view.answers, "answer", "answers")} so far, ${plural(view.updates, "update", "updates")} of ten points or more.`,
      ),
      el(
        "div",
        { className: "scroll" },
        el(
          "table",
          {},
          el(
            "thead",
            {},
            el(
              "tr",
              {},
              el("th", {}, "Name"),
              el("th", {}, "Answered"),
              el("th", {}, "Updated"),
              el("th", {}, "Points"),
              view.mode === "bingo" ? el("th", {}, "Lines") : null,
              el("th", {}, "Paired"),
            ),
          ),
          el(
            "tbody",
            {},
            ...view.players.map((p) =>
              el(
                "tr",
                {},
                el("td", {}, p.name),
                el("td", {}, `${p.answered} of ${p.of}`),
                el("td", {}, String(p.updated)),
                el("td", {}, String(p.score.total)),
                view.mode === "bingo" ? el("td", {}, p.lines ? `${p.lines} 🎉` : "0") : null,
                el("td", {}, awayButton(p)),
              ),
            ),
          ),
        ),
      ),
    );
    const scale = (t: Tally): string =>
      t.statement.chance ? `${round1(t.mean)}% on average` : `average ${said(t.statement, Math.round(t.mean))}`;
    const board = view.leaders.length
      ? el(
          "section",
          { className: "panel" },
          el("h2", {}, "Points so far"),
          el(
            "p",
            { className: "note" },
            `Points for the gap closed with a partner (the same for both, whoever moved), for guessing where a partner landed (up to 100 a guess), and ${view.kudos_points} each time a partner learned something.`,
          ),
          el(
            "ol",
            { className: "tallies leaders" },
            ...view.leaders.slice(0, 10).map((l, i) =>
              el(
                "li",
                { className: i === 0 ? "first" : "" },
                el("span", {}, el("b", {}, l.name)),
                el("small", {}, scoreText(l)),
              ),
            ),
          ),
        )
      : null;
    byId("board").replaceChildren(
      ...[
        board,
        tallyList("Where the room is most split", view.contested, (t) =>
          `${plural(t.n, "answer", "answers")}, ${scale(t)}, spread ${round1(t.spread)}`,
        ),
        view.mode === "free"
          ? tallyList("What people are answering most", view.popular, (t) =>
              `${plural(t.n, "answer", "answers")}, ${scale(t)}, spread ${round1(t.spread)}`,
            )
          : null,
        tallyList("What people changed their minds on", view.shifted, (t) =>
          `${plural(t.updated, "person", "people")} moved; the average went from ${
            t.statement.chance ? `${round1(t.first_mean)}%` : t.first_mean.toFixed(1)
          } to ${t.statement.chance ? `${round1(t.mean)}%` : t.mean.toFixed(1)}`,
        ),
        view.players.length ? people : null,
      ].filter((n): n is HTMLElement => n !== null),
    );
  };

  const draw = (view: HostView): void => {
    current = view;
    shareLink("join-share", joinLink(view.join));
    wallButton("wall-button", view.wall);
    text("who", connectedCount(view.people));
    // A removal answers with nothing; the stream brings the new view.
    roster("people", view.people, {
      remove: (person) => api("POST", at("/remove"), { id: person.id }),
    });
    text("where", view.rounds ? `Round ${view.rounds}` : "");
    drawSetup(view);
    drawWall(view);
    drawControls(view);
    drawBoard(view);
  };

  onClick(
    "start",
    unlessUnfinished(
      "start",
      (left) => `${plural(left, "pair has", "pairs have")} not finished guessing. Start anyway?`,
      () => send("POST", "/round"),
    ),
  );
  onClick("close-picks", () => send("POST", "/picks/close"));
  onClick("stop", () => send("POST", "/stop"));
  onClick("extend", () => send("POST", "/extend"));
  onClick(
    "finish",
    unlessUnfinished(
      "finish",
      (left) => `${plural(left, "pair has", "pairs have")} not finished guessing. End anyway?`,
      () => send("POST", "/finish"),
    ),
  );
  onClick("reopen", () => send("POST", "/reopen"));
  onClick("warm-start", async () => {
    // From a revealed question, "next" ends it and asks another.
    if (current?.phase === "warmup") await send("POST", "/warmup/end");
    await send("POST", "/warmup");
  });
  onClick("warm-next", () => send("POST", "/warmup/next"));
  onClick("warm-end", () => send("POST", "/warmup/end"));

  const feed = watch<HostView>(at(""), draw);
  showConnection(feed);
  void keepAwake();
  ticking(
    feed.now,
    () => (current && current.phase === "talking" ? current.ends : null),
    () => {
      const c = document.getElementById("h-clock");
      return c ? [c] : [];
    },
  );
}


// --------------------------------------------------------------- player

/** One statement's row: the text, a slider, and what it says. */
interface Row {
  box: HTMLElement;
  slider: HTMLInputElement;
  value: HTMLElement;
  /** Says whether the answer is in. */
  tag: HTMLElement;
  mark: HTMLButtonElement | null;
  /** In the free mode: how many answered, and who once you have. */
  crowd: HTMLElement | null;
  statement: Statement;
}

type Sort = "popular" | "unanswered" | "topic";

function player(key: string): void {
  show("player");
  const at = (path: string): string => `/api/p/${key}${path}`;
  let current: PlayerView | null = null;
  const rows = new Map<number, Row>();
  let builtFor = "";
  /** A slider being dragged is not moved by an update arriving. */
  let holding: HTMLInputElement | null = null;

  const accept = (r: { now: number; view: PlayerView }): void => {
    feed.accept(r);
    fail("");
  };

  /** What was sent last, so the thing showing it can move once. */
  let justSent: string | null = null;
  const pulse = <T extends HTMLElement>(key: string, node: T): T => {
    if (justSent === key) landed(node);
    return node;
  };
  /** Takes a reply to a send and redraws with it, even when the room's
   *  stream got there first, so the confirmation always shows. */
  const sentAs = (key: string, r: { now: number; view: PlayerView }): void => {
    justSent = key;
    accept(r);
    if (current) draw(current);
    justSent = null;
  };

  const answer = async (id: number, value: number | null): Promise<void> => {
    const r = await api<{ now: number; view: PlayerView }>("PUT", at("/answer"), { id, value });
    sentAs(`s${id}`, r);
  };

  /** Whether a statement's answer is in: always open to change until the
   *  evening ends. */
  const tagFor = (tag: HTMLElement, answered: boolean, finished: boolean): void => {
    tag.className = `sent-tag${answered ? " in" : ""}`;
    tag.textContent = !answered ? "" : finished ? "🔒 Final" : "✓ Sent";
    tag.title = answered && !finished ? "Sent when you let go. You can change it whenever you like." : "";
  };

  const pickIt = async (id: number, picked: boolean): Promise<void> => {
    accept(await api("PUT", at("/pick"), { id, picked }));
  };

  /** In the pick mode, before answering: each statement's pick button. */
  const pickButtons = new Map<number, HTMLButtonElement>();

  const markIt = async (id: number, marked: boolean): Promise<void> => {
    accept(await api("PUT", at("/mark"), { id, marked }));
  };

  const run = (p: Promise<void>): void => {
    p.catch(failed);
  };

  /** A slider for one statement, sending when let go. */
  const slider = (s: Statement, onSend: (v: number) => void, label: HTMLElement): HTMLInputElement => {
    const input = el("input", {
      type: "range",
      min: s.chance ? "0" : "1",
      max: s.chance ? "100" : "7",
      step: "1",
      className: "scale unset",
    });
    input.setAttribute("aria-label", s.text);
    input.addEventListener("pointerdown", () => {
      holding = input;
    });
    input.addEventListener("input", () => {
      input.classList.remove("unset");
      label.textContent = said(s, Number(input.value));
    });
    onRelease(input, onSend, () => {
      holding = null;
    });
    return input;
  };

  const ends = (s: Statement): HTMLElement =>
    el(
      "div",
      { className: "ends" },
      el("span", {}, s.chance ? "0% · surely false" : "1 · disagree"),
      ...(s.chance ? [el("span", {}, "50% · no idea")] : []),
      el("span", {}, s.chance ? "surely true · 100%" : "agree · 7"),
    );

  /** Builds the list once per deck; later views only set its values. */
  const build = (view: PlayerView): void => {
    rows.clear();
    pickButtons.clear();
    const names = new Map(view.topics);
    const byTopic = new Map<string, Square[]>();
    for (const sq of view.squares) {
      const list = byTopic.get(sq.statement.topic) ?? [];
      list.push(sq);
      byTopic.set(sq.statement.topic, list);
    }
    if (view.phase === "picking") {
      const choice = (sq: Square): HTMLElement => {
        const b = el("button", { type: "button", className: "chip small" }, "Pick");
        b.addEventListener("click", () => run(pickIt(sq.statement.id, b.getAttribute("aria-pressed") !== "true")));
        pickButtons.set(sq.statement.id, b);
        return el(
          "div",
          { className: "statement-row", id: `s-${sq.statement.id}` },
          el("p", { className: "statement" }, sq.statement.text),
          el("div", { className: "row-foot" }, el("span", {}), b),
        );
      };
      byId("statements").replaceChildren(
        ...[...byTopic.entries()].map(([topic, squares], i) =>
          el(
            "details",
            { className: "panel topic", open: i === 0 },
            el("summary", {}, el("span", {}, names.get(topic) ?? topic), el("small", { id: `count-${topic}` })),
            ...squares.map(choice),
          ),
        ),
      );
      return;
    }
    const make = (sq: Square): HTMLElement => {
      const s = sq.statement;
      const value = el("span", { className: "value" }, said(s, sq.answer));
      const tag = el("span", {});
      const input = slider(s, (v) => run(answer(s.id, v)), value);
      const clear = el("button", { type: "button", className: "chip small" }, "Clear");
      clear.addEventListener("click", () => run(answer(s.id, null)));
      let mark: HTMLButtonElement | null = null;
      if (view.mode === "bingo") {
        mark = el("button", { type: "button", className: "chip small" }, "Talked about it");
        const m = mark;
        m.addEventListener("click", () => run(markIt(s.id, m.getAttribute("aria-pressed") !== "true")));
      }
      const box = el(
        "div",
        { className: "statement-row", id: `s-${s.id}` },
        el("p", { className: "statement" }, s.text),
        el(
          "p",
          { className: "note" },
          `${names.get(s.topic) ?? s.topic}${sq.picked ? " · your pick" : ""}${
            s.chance ? (s.text.endsWith("?") ? " · a probability" : " · how confident are you that this is true?") : ""
          }`,
        ),
        input,
        ends(s),
        el("div", { className: "row-foot" }, el("span", {}, value, " ", tag), el("span", {}, mark ?? "", clear)),
      );
      const crowd = view.mode === "free" ? el("p", { className: "crowd" }) : null;
      if (crowd) box.append(crowd);
      rows.set(s.id, { box, slider: input, value, tag, mark, crowd, statement: s });
      return box;
    };

    const list = byId("statements");
    if (view.mode === "bingo") {
      list.replaceChildren(
        el(
          "section",
          { className: "panel" },
          el("h2", {}, "Where do you stand?"),
          el("p", { className: "note" }, "Drag each slider: an answer is sent when you let go, and you can change it whenever you like."),
          ...view.squares.map(make),
        ),
      );
      return;
    }
    if (view.mode === "free") {
      const shelf = el("div", { id: "shelf" }, ...view.squares.map(make));
      list.replaceChildren(
        el("div", { className: "chips", id: "tabs" }),
        el("section", { className: "panel", id: "quick" }),
        el(
          "section",
          { className: "panel", id: "browse" },
          el("h2", {}, "Browse"),
          el(
            "p",
            { className: "note" },
            "Answer whatever you like, whenever you like. You see how many people answered each one; who, and what they said, once you have answered it too.",
          ),
          el("div", { className: "chips", id: "sorts" }),
          el("div", { className: "chips", id: "filters" }),
          shelf,
        ),
      );
      arrange(view);
      tabs();
      return;
    }
    if (view.mode === "pick") {
      list.replaceChildren(
        el(
          "section",
          { className: "panel" },
          el("h2", {}, "The room's picks"),
          el("p", { className: "note" }, "Every statement somebody picked, the most picked first. Answer as many as you like."),
          ...view.squares.map(make),
        ),
      );
      return;
    }
    // In the open mode, one folding section per topic.
    list.replaceChildren(
      el(
        "p",
        { className: "note intro" },
        "Answer as many as you like. The more you answer, the better the match.",
      ),
      ...[...byTopic.entries()].map(([topic, squares], i) => {
        const d = el(
          "details",
          { className: "panel topic", open: i === 0 },
          el("summary", {}, el("span", {}, names.get(topic) ?? topic), el("small", { id: `count-${topic}` })),
          ...squares.map(make),
        );
        return d;
      }),
    );
  };

  let sort: Sort = "popular";
  let only = "all";
  let tab: "quick" | "browse" = "quick";

  const tabs = (): void => {
    chips<"quick" | "browse">(
      "tabs",
      [
        { value: "quick", label: "Quick fire", on: tab === "quick" },
        { value: "browse", label: "Browse everything", on: tab === "browse" },
      ],
      (next) => {
        tab = next;
        tabs();
        if (current) arrange(current);
      },
    );
    hide("quick", tab !== "quick");
    hide("browse", tab !== "browse");
  };

  // The quick-fire card. It stays on one statement until the player
  // answers and moves on, or skips, however the feed's order changes.
  let card: number | null = null;
  let cardBuilt: number | null = null;
  let cardSlider: HTMLInputElement | null = null;
  let cardValue: HTMLElement | null = null;
  let cardCrowd: HTMLElement | null = null;
  let cardNext: HTMLButtonElement | null = null;
  let cardCount: HTMLElement | null = null;
  let cardTag: HTMLElement | null = null;

  const pick = (view: PlayerView): void => {
    card = view.next.find((id) => id !== card) ?? null;
  };

  const drawCard = (view: PlayerView): void => {
    const box = document.getElementById("quick");
    if (!box) return;
    const answered = view.squares.filter((sq) => sq.answer !== null).length;
    if (card === null) pick(view);
    const sq = view.squares.find((q) => q.statement.id === card);
    if (!sq) {
      cardBuilt = null;
      box.replaceChildren(
        el("h2", {}, "That is all of them"),
        el(
          "p",
          { className: "note" },
          `You have answered ${answered} and skipped the rest. Browse everything to change an answer or go back to one you skipped.`,
        ),
      );
      return;
    }
    if (cardBuilt !== sq.statement.id) {
      cardBuilt = sq.statement.id;
      const s = sq.statement;
      cardValue = el("span", { className: "value" }, said(s, sq.answer));
      cardSlider = slider(s, (v) => run(answer(s.id, v)), cardValue);
      cardCrowd = el("p", { className: "crowd" });
      const skip = el("button", { type: "button", className: "button" }, "Skip");
      skip.addEventListener("click", () =>
        run(
          (async () => {
            accept(await api("PUT", at("/skip"), { id: s.id }));
            if (current) {
              pick(current);
              drawCard(current);
            }
          })(),
        ),
      );
      cardNext = el("button", { type: "button", className: "button primary" }, "Next");
      cardNext.addEventListener("click", () => {
        if (!current) return;
        pick(current);
        drawCard(current);
      });
      cardCount = el("p", { className: "eyebrow" });
      box.replaceChildren(
        cardCount,
        el("p", { className: "statement big-statement" }, s.text),
        el(
          "p",
          { className: "note" },
          `${new Map(view.topics).get(s.topic) ?? s.topic}${
            s.chance ? (s.text.endsWith("?") ? " · a probability" : " · how confident are you that this is true?") : ""
          }`,
        ),
        cardSlider,
        ends(s),
        el("div", { className: "row-foot" }, el("span", {}, cardValue, " ", (cardTag = el("span", {})))),
        cardCrowd,
        el("div", { className: "actions" }, skip, cardNext),
      );
    }
    if (cardSlider && cardSlider !== holding) {
      cardSlider.value = String(sq.answer ?? (sq.statement.chance ? 50 : 4));
      cardSlider.classList.toggle("unset", sq.answer === null);
      if (cardValue) cardValue.textContent = said(sq.statement, sq.answer);
    }
    if (cardCrowd) cardCrowd.textContent = crowdText(sq);
    if (cardTag) {
      tagFor(cardTag, sq.answer !== null, false);
      pulse(`s${sq.statement.id}`, cardTag);
    }
    if (cardCount) cardCount.textContent = `Quick fire · ${answered} answered`;
    if (cardNext) cardNext.disabled = sq.answer === null;
  };

  /** Orders and filters the free mode's statements. Only when asked, not
   *  on every update, so nothing jumps under a finger. */
  const arrange = (view: PlayerView): void => {
    const names = new Map(view.topics);
    chips<Sort>(
      "sorts",
      [
        { value: "popular", label: "Most answered", on: sort === "popular" },
        { value: "unanswered", label: "Not answered by me", on: sort === "unanswered" },
        { value: "topic", label: "By topic", on: sort === "topic" },
      ],
      (next) => {
        sort = next;
        if (current) arrange(current);
      },
    );
    const topics = [...new Set(view.squares.map((sq) => sq.statement.topic))];
    chips<string>(
      "filters",
      [
        { value: "all", label: "All topics", on: only === "all" },
        ...topics.map((t) => ({ value: t, label: names.get(t) ?? t, on: only === t })),
      ],
      (next) => {
        only = next;
        if (current) arrange(current);
      },
    );
    const order = [...view.squares];
    const byCount = (a: Square, b: Square): number =>
      b.count + (b.answer !== null ? 1 : 0) - (a.count + (a.answer !== null ? 1 : 0));
    if (sort === "popular") order.sort(byCount);
    if (sort === "unanswered") {
      order.sort((a, b) => Number(a.answer !== null) - Number(b.answer !== null) || byCount(a, b));
    }
    const shelf = document.getElementById("shelf");
    if (!shelf) return;
    for (const sq of order) {
      const row = rows.get(sq.statement.id);
      if (!row) continue;
      row.box.hidden = only !== "all" && sq.statement.topic !== only;
      shelf.append(row.box);
    }
  };

  /** Who else answered, most opposed first. */
  const crowdText = (sq: Square): string => {
    if (sq.answer === null) {
      return sq.count
        ? `${plural(sq.count, "person has", "people have")} answered. Answer it to see who.`
        : "Nobody has answered this yet.";
    }
    if (!sq.others.length) return "Nobody else has answered this yet.";
    const mine = sq.answer;
    const sorted = [...sq.others].sort(
      (a, b) => Math.abs(b.answer - mine) - Math.abs(a.answer - mine),
    );
    const shown = sorted
      .slice(0, 6)
      .map((o) => `${o.name} ${brief(sq.statement, o.answer)}`);
    const more = sorted.length > 6 ? ` and ${sorted.length - 6} more` : "";
    const mean = sq.others.reduce((sum, o) => sum + o.answer, 0) / sq.others.length;
    const average = sq.statement.chance ? `${Math.round(mean)}%` : mean.toFixed(1);
    return `${plural(sq.others.length, "other", "others")}, furthest from you first: ${shown.join(", ")}${more}. Their average: ${average}.`;
  };

  const drawGrid = (view: PlayerView): void => {
    hide("card-panel", view.mode !== "bingo");
    if (view.mode !== "bingo") return;
    const grid = byId("grid");
    grid.style.gridTemplateColumns = `repeat(${view.side}, 1fr)`;
    grid.replaceChildren(
      ...view.squares.map((sq, i) => {
        const cell = el(
          "button",
          { type: "button", className: `cell${sq.marked ? " marked" : ""}${sq.answer === null ? " empty" : ""}` },
          el("span", { className: "n" }, String(i + 1)),
          el("span", { className: "t" }, sq.statement.text),
        );
        if (sq.answer !== null) {
          cell.style.setProperty("--lean", String(lean(sq.statement, sq.answer)));
        }
        cell.addEventListener("click", () =>
          document.getElementById(`s-${sq.statement.id}`)?.scrollIntoView({ behavior: "smooth", block: "center" }),
        );
        return cell;
      }),
    );
    const answered = view.squares.filter((s) => s.answer !== null).length;
    text(
      "card-note",
      view.lines
        ? `Bingo! ${plural(view.lines, "line", "lines")}. ${answered} of ${view.squares.length} answered.`
        : `${answered} of ${view.squares.length} answered. Talk about a full line for a bingo.`,
    );
  };

  /** A guess being dragged but not yet locked in, by partner. */
  const guessDraft = new Map<string, number>();

  const sendGuess = async (name: string, value: number): Promise<void> => {
    const r = await api<{ now: number; view: PlayerView }>("PUT", at("/guess"), { name, value });
    sentAs(`guess:${name}`, r);
  };

  const sendKudos = async (name: string, given: boolean): Promise<void> => {
    accept(await api("PUT", at("/kudos"), { name, given }));
  };

  const sendWarm = async (value: number): Promise<void> => {
    const r = await api<{ now: number; view: PlayerView }>("PUT", at("/warm"), { value });
    sentAs("warm", r);
  };

  /** A calibration question: before the clue, with it, and the answer. */
  const drawWarm = (box: HTMLElement, view: PlayerView, w: WarmMine): void => {
    const head = [
      el("p", { className: "eyebrow" }, `Warm-up ${w.number} · a question with a right answer`),
      el("p", { className: "note" }, w.setup),
      el("p", { className: "statement big-statement" }, w.claim),
    ];
    if (w.stage === "revealed" && w.result) {
      const r = w.result;
      const lines = [
        `Before your clue you said ${w.before === null ? "nothing" : `${w.before}%`}: ${bitsText(r.bits_before)}.`,
        `After it you said ${w.after === null ? "nothing" : `${w.after}%`}: ${bitsText(r.bits_after)}.`,
        `${w.clue ?? ""} From the start the setup gives, Bayes says ${pct(r.ideal)} after that clue${
          r.ideal_from_yours === null ? "" : `, and ${pct(r.ideal_from_yours)} from where you started`
        }.`,
      ];
      box.replaceChildren(
        ...head,
        el("p", { className: `verdict-line ${r.truth ? "good" : "poor"}` }, r.truth ? "It was true." : "It was false."),
        ...lines.map((line) => el("p", { className: "note" }, line)),
        view.calibration
          ? el(
              "p",
              {},
              `Your warm-up so far: ${bitsText(view.calibration[0])}. The Bayes bot: ${bitsText(view.calibration[1])}.`,
            )
          : "",
        el(
          "p",
          { className: "note" },
          "Bits against a coin flip: 50% scores nothing, sure and right nearly a bit, sure and wrong costs a lot. Saying what you believe is the best you can do.",
        ),
      );
      return;
    }
    const clue = w.stage === "clue";
    const current = clue ? w.after : w.before;
    const [input, value] = percentSlider(current ?? (clue ? w.before ?? 50 : 50), current === null, (v) =>
      run(sendWarm(v)),
    );
    box.replaceChildren(
      ...head,
      clue && w.clue ? el("p", { className: "clue" }, `Your clue: ${w.clue}`) : "",
      el(
        "h3",
        {},
        clue ? "How likely is it now?" : "How likely is it, before any clue?",
      ),
      el(
        "p",
        { className: "note" },
        clue
          ? "Other people have different clues. Compare with the people near you if you like, then answer."
          : "Everybody has a clue coming. First say how likely it is from the setup alone.",
      ),
      input,
      el("div", { className: "ends" }, el("span", {}, "0% · surely false"), el("span", {}, "surely true · 100%")),
      el("div", { className: "row-foot" }, value),
      clue && w.before !== null ? el("p", { className: "note" }, `Your answer before the clue, ${w.before}%, is locked in.`) : "",
      pulse(
        "warm",
        current === null
          ? sentBox("waiting", "Not sent yet", "Move the slider. Your answer is sent when you let go.")
          : sentBox(
              "sent",
              `✓ Sent: ${current}%`,
              clue
                ? "You can still change it until the host reveals the answer."
                : "You can still change it until the host deals the clues.",
            ),
      ),
    );
  };

  /** After a round: say where you stand, guess your partner, and thank.
   *  A partner's new answer is shown only once yours and your guesses
   *  are in, and from then on yours stays as the round's. */
  const drawAfter = (box: HTMLElement, view: PlayerView, g: Mine): void => {
    const s = g.statement;
    const guesses = g.partners.map((p) => {
      if (p.guess !== null) {
        const landed = !g.done
          ? `You see where ${p.name} landed once you have said where you stand.`
          : p.after === null
            ? `${p.name} has not said yet.`
            : `${p.name} says ${said(s, p.after)}${p.done ? "" : ", and may still change it"}.`;
        return el(
          "div",
          { className: "step done" },
          pulse(`guess:${p.name}`, sentBox("locked", `🔒 Your guess for ${p.name}: ${said(s, p.guess)}`, landed)),
        );
      }
      const draft = guessDraft.get(p.name) ?? p.answer;
      const value = el("span", { className: "value" }, said(s, draft));
      const input = el("input", {
        type: "range",
        min: s.chance ? "0" : "1",
        max: s.chance ? "100" : "7",
        step: "1",
        className: "scale",
      });
      input.value = String(draft);
      input.addEventListener("pointerdown", () => {
        holding = input;
      });
      input.addEventListener("input", () => {
        guessDraft.set(p.name, Number(input.value));
        value.textContent = said(s, Number(input.value));
      });
      // A guess is only sent by the button.
      onRelease(input, () => {}, () => {
        holding = null;
      });
      const lock = el("button", { type: "button", className: "button primary" }, `Lock in my guess`);
      lock.addEventListener("click", () => run(sendGuess(p.name, Number(input.value))));
      return el(
        "div",
        { className: "step" },
        el("p", {}, el("b", {}, `Where do you think ${p.name} stands now?`), ` They started at ${said(s, p.answer)}.`),
        input,
        ends(s),
        el("div", { className: "row-foot" }, value),
        sentBox(
          "waiting",
          "Not locked in yet",
          "Your guess counts once you press Lock in, and cannot be changed after that. You see their answer once your guess and your own answer are in; the closer, the more points.",
        ),
        el("div", { className: "actions" }, lock),
      );
    });

    const guessed = g.partners.every((p) => p.guess !== null);
    let standing: (HTMLElement | string)[];
    if (g.done && g.after !== null) {
      standing = [
        pulse(
          `s${s.id}`,
          sentBox(
            "locked",
            `🔒 Your answer this round: ${said(s, g.after)}`,
            "It counts as it was when your guess and answer were both in. Changing it on your card now does not change the round.",
          ),
        ),
      ];
    } else {
      const mineValue = el("span", { className: "value" }, said(s, g.after ?? g.before));
      const mine = slider(s, (v) => run(answer(s.id, v)), mineValue);
      mine.value = String(g.after ?? g.before);
      mine.classList.remove("unset");
      const same = el("button", { type: "button", className: "button" }, "Where I was");
      same.addEventListener("click", () => run(answer(s.id, g.before)));
      standing = [
        mine,
        ends(s),
        el("div", { className: "row-foot" }, mineValue),
        pulse(
          `s${s.id}`,
          g.after === null
            ? sentBox(
                "waiting",
                "Not sent yet",
                guessed
                  ? "Move the slider, or press Where I was. It is sent when you let go, and then it stays as your answer this round."
                  : "Move the slider, or press Where I was. It is sent when you let go.",
              )
            : sentBox("sent", `✓ Sent: ${said(s, g.after)}`, "You can change it until you lock in your guess."),
        ),
        el("div", { className: "actions" }, same),
      ];
    }

    const thanks = g.partners.map((p) => {
      const b = el(
        "button",
        { type: "button", className: `chip${p.kudos ? " on" : ""}` },
        p.kudos ? `✓ I learned something from ${p.name}` : `I learned something from ${p.name}`,
      );
      b.setAttribute("aria-pressed", String(p.kudos));
      b.addEventListener("click", () => run(sendKudos(p.name, !p.kudos)));
      return b;
    });

    box.replaceChildren(
      el("p", { className: "eyebrow" }, `Round ${view.round} is over · ${g.spot}`),
      el("h2", {}, "Before you go"),
      el("p", { className: "statement" }, s.text),
      el("h3", {}, "1. Where do you stand now?"),
      el("p", { className: "note" }, `You started at ${said(s, g.before)}.`),
      ...standing,
      el("h3", {}, g.partners.length > 1 ? "2. Where do they stand?" : `2. Where does ${g.partners[0]?.name ?? "your partner"} stand?`),
      ...guesses,
      el("h3", {}, "3. Did you learn something?"),
      el("div", { className: "chips" }, ...thanks),
      el(
        "p",
        { className: "note" },
        "Points come from closing the gap between you, whoever moved, from guessing well, and from partners who learned something from you.",
      ),
    );
  };

  const drawNow = (view: PlayerView): void => {
    const box = byId("now");
    const g = view.group;
    if (view.phase === "warmup" && view.warmup) {
      drawWarm(box, view, view.warmup);
      return;
    }
    if (view.phase === "talking" && g) {
      const s = g.statement;
      box.replaceChildren(
        el("p", { className: "eyebrow" }, `Round ${view.round}`),
        el("p", { className: "clock", id: "p-clock" }),
        el("p", { className: "note" }, "Meet at"),
        el("p", { className: "spot" }, g.spot),
        el("p", { className: "big" }, `with ${g.partners.map((p) => p.name).join(" and ")}`),
        el("p", { className: "statement" }, s.text),
        el(
          "div",
          { className: "facts" },
          el("div", { className: "fact" }, el("span", {}, "You"), el("b", {}, said(s, g.before))),
          ...g.partners.map((p) =>
            el("div", { className: "fact" }, el("span", {}, p.name), el("b", {}, said(s, p.answer))),
          ),
        ),
        el(
          "p",
          { className: "note" },
          "Take turns saying why you think what you think. Look for what would change your mind, not for how to win. Afterwards you guess where your partner landed.",
        ),
      );
      return;
    }
    if (view.phase === "updating" && g) {
      drawAfter(box, view, g);
      return;
    }
    if (view.waiting) {
      box.replaceChildren(
        el("p", { className: "eyebrow" }, `Round ${view.round}`),
        el("h2", {}, view.away ? "You are sitting this round out" : "You were not paired this round"),
        el(
          "p",
          { className: "note" },
          view.away
            ? "Press I'm back above when you want to be paired again."
            : "Nobody was left, or nobody had answered the same statements as you. Answer some more, or go and find somebody below.",
        ),
      );
      return;
    }
    if (view.phase === "picking") {
      const mine = view.squares.filter((sq) => sq.picked);
      box.replaceChildren(
        el("p", { className: "eyebrow" }, `Picking · ${mine.length} of ${view.picks}`),
        el("h2", {}, "What would you like to talk about?"),
        el(
          "p",
          { className: "note" },
          `Pick up to ${view.picks} statements below. When the host closes the picks, everybody answers the statements somebody picked, the most picked first.`,
        ),
        mine.length
          ? el("ol", { className: "tallies" }, ...mine.map((sq) => el("li", {}, el("span", {}, sq.statement.text))))
          : "",
      );
      return;
    }
    if (view.phase === "finished") {
      box.replaceChildren(
        el("p", { className: "eyebrow" }, "That was the evening"),
        el("h2", {}, view.updated ? `You updated on ${plural(view.updated, "statement", "statements")}` : "You held your ground"),
        el("p", {}, scoreText(view.score) + (view.rank ? ` Place ${view.rank} in the room.` : "")),
        view.calibration
          ? el("p", { className: "note" }, `Warm-up: ${bitsText(view.calibration[0])}, against the Bayes bot's ${bitsText(view.calibration[1])}.`)
          : "",
        view.awards.length ? el("h3", {}, "The prizes") : "",
        view.awards.length ? awardCards(view.awards) : "",
        view.winners.length
          ? el(
              "div",
              {},
              el("h3", {}, "Most points"),
              el(
                "ol",
                { className: "tallies leaders" },
                ...view.winners.map((l, i) =>
                  el(
                    "li",
                    { className: i === 0 ? "first" : "" },
                    el("span", {}, el("b", {}, l.name), i === 0 ? " 🏆" : ""),
                    el("small", {}, `${l.total} points`),
                  ),
                ),
              ),
            )
          : "",
      );
      return;
    }
    box.replaceChildren(
      el("p", { className: "eyebrow" }, view.round ? `Between rounds` : "Answering"),
      el("h2", {}, view.round ? "Waiting for the next round" : "Say where you stand"),
      view.round ? el("p", { className: "note" }, scoreText(view.score) + (view.rank ? ` Place ${view.rank} in the room.` : "")) : "",
      el(
        "p",
        { className: "note" },
        view.mode === "bingo"
          ? "Answer the statements on your card. When a round starts you are paired with whoever disagrees with you most, preferring statements that get you closer to a bingo line."
          : view.mode === "free"
            ? "Browse and answer whatever you have a view on. The more people answer the same statements, the better the matches below get: go and talk to whoever disagrees with you most."
            : "Answer the statements you have a view on. When a round starts you are paired with whoever disagrees with you most.",
      ),
    );
  };

  const drawOpposites = (view: PlayerView): void => {
    const box = byId("opposites");
    hide("opposites", view.opposites.length === 0);
    box.replaceChildren(
      el("h2", {}, "Who disagrees with you most"),
      el("p", { className: "note" }, "You do not have to wait for a round. Go and find one of them."),
      el(
        "ol",
        { className: "tallies" },
        ...view.opposites.map((o) =>
          el(
            "li",
            {},
            el("span", {}, el("b", {}, o.name), ` on “${o.statement.text}”`),
            el("small", {}, `You: ${said(o.statement, o.mine)} · ${o.name}: ${said(o.statement, o.theirs)}`),
          ),
        ),
      ),
    );
  };

  /** Sitting the next rounds out, or back. Not while a round is being
   *  talked, when it would only take effect at the next one anyway. */
  const drawAway = (view: PlayerView): void => {
    hide("away-bar", view.phase === "finished" || (view.phase === "talking" && !view.away));
    byId("away-bar").classList.toggle("on", view.away);
    text(
      "away-note",
      view.away
        ? "You are sitting rounds out, and are not paired. Answering a statement brings you back too."
        : "Leaving, or need a break? Say so, and nobody waits for you at a meeting spot.",
    );
    text("away", view.away ? "I'm back" : "Sit the next round out");
  };

  const draw = (view: PlayerView): void => {
    current = view;
    text("you", `You are ${view.name} · ${plural(view.players, "person", "people")} in the room`);
    drawAway(view);
    // How the evening works, once per phone, until the first round.
    hide("explainer", explained || view.round > 0 || view.phase === "finished");
    text("where", view.round ? `Round ${view.round}` : "");
    const picking = view.phase === "picking";
    const shape = `${view.mode}:${picking}:${view.squares.map((s) => s.statement.id).join(",")}`;
    if (shape !== builtFor) {
      builtFor = shape;
      build(view);
    }
    const finished = view.phase === "finished";
    for (const sq of view.squares) {
      const row = rows.get(sq.statement.id);
      if (!row) continue;
      row.slider.disabled = finished;
      if (row.slider !== holding) {
        row.slider.value = String(sq.answer ?? (sq.statement.chance ? 50 : 4));
        row.slider.classList.toggle("unset", sq.answer === null);
        row.value.textContent = said(sq.statement, sq.answer);
      }
      tagFor(row.tag, sq.answer !== null, finished);
      pulse(`s${sq.statement.id}`, row.tag);
      if (row.mark) row.mark.setAttribute("aria-pressed", String(sq.marked));
      if (row.crowd) row.crowd.textContent = crowdText(sq);
      row.box.classList.toggle("answered", sq.answer !== null);
    }
    if (picking) {
      const full = view.squares.filter((sq) => sq.picked).length >= view.picks;
      const counts = new Map<string, number>();
      for (const sq of view.squares) {
        const b = pickButtons.get(sq.statement.id);
        if (b) {
          b.setAttribute("aria-pressed", String(sq.picked));
          b.textContent = sq.picked ? "✓ Picked" : "Pick";
          b.disabled = full && !sq.picked;
        }
        if (sq.picked) counts.set(sq.statement.topic, (counts.get(sq.statement.topic) ?? 0) + 1);
      }
      for (const topic of new Set(view.squares.map((sq) => sq.statement.topic))) {
        const n = document.getElementById(`count-${topic}`);
        const c = counts.get(topic);
        if (n) n.textContent = c ? `${c} picked` : "";
      }
    }
    if (view.mode === "open") {
      const counts = new Map<string, [number, number]>();
      for (const sq of view.squares) {
        const c = counts.get(sq.statement.topic) ?? [0, 0];
        counts.set(sq.statement.topic, [c[0] + (sq.answer !== null ? 1 : 0), c[1] + 1]);
      }
      for (const [topic, [done, all]] of counts) {
        const n = document.getElementById(`count-${topic}`);
        if (n) n.textContent = `${done} of ${all}`;
      }
    }
    // The panel at the top is rebuilt only when what it shows changes,
    // so a slider in it is not reset under a finger.
    const nowKey = JSON.stringify([
      view.phase,
      view.round,
      view.group,
      view.waiting,
      view.away,
      view.updated,
      view.moved,
      view.mode,
      view.score,
      view.rank,
      view.winners,
      view.awards,
      view.warmup,
      view.calibration,
      picking ? view.squares.filter((sq) => sq.picked).map((sq) => sq.statement.id) : null,
    ]);
    // Not while a slider in it is under a finger: it is drawn again when
    // the finger lets go and the next update arrives.
    if (nowKey !== drawnNow && !byId("now").contains(holding)) {
      drawnNow = nowKey;
      drawNow(view);
    }
    drawOpposites(view);
    drawGrid(view);
    if (view.mode === "free") drawCard(view);
  };
  let drawnNow = "";

  suggestions("suggest-form", "idea", "idea-chance", "idea-done");
  onClick("away", async () => {
    if (current) accept(await api("PUT", at("/away"), { away: !current.away }));
  });

  let explained = stored(EXPLAINED, false);
  byId("explained").addEventListener("click", () => {
    explained = true;
    persist(EXPLAINED, true);
    hide("explainer", true);
  });

  const feed = watch<PlayerView>(at(""), draw);
  showConnection(feed);
  void keepAwake();
  ticking(
    feed.now,
    () => (current && current.phase === "talking" ? current.ends : null),
    () => {
      const c = document.getElementById("p-clock");
      return c ? [c] : [];
    },
  );
}

// ----------------------------------------------------------------- wall
//
// The projector's screen, `/w/<key>`. The QR code and who is in come from
// `rooms`; this is the game's part. Everybody in the room sees it, so the
// server sends no names but the top of the board, and an answer only once
// the group it belongs to has settled, sorted among the others.

interface WallMeeting {
  spot: string;
  statement: Statement;
}

interface WallSettled {
  statement: Statement;
  groups: number;
  before: number[];
  after: number[];
  closed: number;
}

interface WallRound {
  number: number;
  on: boolean;
  meetings: WallMeeting[];
  people: number;
  done: number;
  waiting: number;
  results: WallSettled[];
}

interface WallWarm {
  number: number;
  setup: string;
  claim: string;
  stage: Stage;
  before: number;
  after: number;
  outcome: Outcome | null;
  board: Calibrated[];
}

interface WallGame {
  phase: Phase;
  mode: Mode;
  ends: number;
  people: number;
  answering: number;
  answers: number;
  round: WallRound | null;
  picking: Picking | null;
  warmup: WallWarm | null;
  board: { name: string; total: number }[];
  awards: Award[];
}

/** Answers as dots along their scale, with its two ends named. */
function scaleDots(statement: Statement, values: number[], label: string): HTMLElement {
  const [low, high] = statement.chance ? ["0%", "100%"] : ["1 · disagree", "agree · 7"];
  return el(
    "div",
    { className: "wall-dots" },
    el("span", { className: "wall-dots-label" }, label),
    el(
      "div",
      { className: "dots" },
      el("span", { className: "dots-end" }, low),
      el("span", { className: "dots-end" }, high),
      ...values.map((v) => {
        const dot = el("i", {});
        dot.style.left = `${lean(statement, v) * 100}%`;
        return dot;
      }),
    ),
  );
}

/** How one statement came out: everybody's answers before and after, as
 *  dots, and how far the groups closed. */
function settledCard(r: WallSettled): HTMLElement {
  return el(
    "div",
    { className: "wall-result" },
    el("p", { className: "wall-result-statement" }, r.statement.text),
    scaleDots(r.statement, r.before, "Before"),
    scaleDots(r.statement, r.after, "After"),
    el(
      "p",
      { className: "note" },
      `${plural(r.groups, "group", "groups")} · ${r.closed ? `closed ${r.closed} points` : "no closer"}${r.groups > 1 ? " on average" : ""}`,
    ),
  );
}

function wallBoard(board: { name: string; total: number }[]): HTMLElement | string {
  return board.length
    ? el(
        "div",
        {},
        el("h2", {}, "Most points"),
        el("ol", { className: "wall-board" }, ...board.map((l) => el("li", {}, el("b", {}, l.name), el("span", {}, String(l.total))))),
      )
    : "";
}

function wallScreen(key: string): void {
  let current: WallGame | null = null;
  let clock = false;
  const draw = (game: WallGame | null, slot: HTMLElement, feed: Feed): void => {
    current = game;
    if (!clock) {
      clock = true;
      ticking(
        feed.now,
        () => (current && current.phase === "talking" ? current.ends : null),
        () => {
          const c = document.getElementById("w-clock");
          return c ? [c] : [];
        },
      );
    }
    if (!game) {
      slot.replaceChildren();
      return;
    }
    slot.replaceChildren(...wallParts(game));
    const c = document.getElementById("w-clock");
    if (c) c.textContent = countdown(game.ends - feed.now());
  };
  wall<WallGame | null>(key, draw);
}

function wallParts(game: WallGame): (HTMLElement | string)[] {
  const w = game.warmup;
  if (game.phase === "warmup" && w) {
    const o = w.outcome;
    return [
      el("p", { className: "eyebrow" }, `Warm-up ${w.number} · a question with a right answer`),
      el("p", { className: "wall-setup" }, w.setup),
      el("p", { className: "wall-claim" }, w.claim),
      o
        ? el(
            "div",
            {},
            el("p", { className: `wall-verdict ${o.truth ? "good" : "poor"}` }, o.truth ? "True" : "False"),
            scaleDots({ id: 0, text: "", topic: "", chance: true }, o.answers, "The room"),
            el(
              "p",
              { className: "wall-line" },
              `The room said ${o.mean_before === null ? "–" : `${Math.round(o.mean_before)}%`} before the clues, ${
                o.mean_after === null ? "–" : `${Math.round(o.mean_after)}%`
              } after. The room scored ${bitsText(o.room)} on average; the Bayes bot ${bitsText(o.bot)}.`,
            ),
            w.board.length
              ? el(
                  "ol",
                  { className: "wall-board" },
                  ...w.board.map((c) => el("li", { className: c.bot ? "bot" : "" }, el("b", {}, c.bot ? "🤖 Bayes bot" : c.name), el("span", {}, bitsText(c.bits)))),
                )
              : "",
          )
        : el(
            "p",
            { className: "wall-line" },
            w.stage === "before"
              ? `How likely is it, from the setup alone? ${w.before} of ${plural(game.people, "person has", "people have")} answered.`
              : `Clues are on your phones. Compare with your neighbours, then answer again. ${w.after} of ${game.people} answered.`,
          ),
    ];
  }
  const r = game.round;
  if (r && r.on) {
    const talking = game.phase === "talking";
    return [
      el(
        "div",
        { className: "wall-head" },
        el("p", { className: "eyebrow" }, `Round ${r.number}`),
        talking ? el("p", { className: "clock wall-clock", id: "w-clock" }) : "",
      ),
      el(
        "p",
        { className: "wall-line" },
        talking
          ? "Your phone says where to meet and who to find. Talk about the statement on your spot."
          : `Phones out: say where you stand now, guess where your partner landed, and whether you learned something. ${r.done} of ${plural(r.people, "person", "people")} done.`,
      ),
      r.results.length ? el("div", { className: "wall-results" }, ...r.results.map(settledCard)) : "",
      el(
        "div",
        { className: "pairs wall-meetings" },
        ...r.meetings.map((m) =>
          el("div", { className: "pair" }, el("p", { className: "pair-spot" }, m.spot), el("p", { className: "pair-statement" }, m.statement.text)),
        ),
      ),
      r.waiting ? el("p", { className: "note" }, `${plural(r.waiting, "person is", "people are")} not paired this round: answer statements somebody else has answered.`) : "",
    ];
  }
  if (game.phase === "picking" && game.picking) {
    const p = game.picking;
    return [
      el("p", { className: "eyebrow" }, "Before we start"),
      el("p", { className: "wall-claim" }, `Pick up to ${p.limit} statements you would like to talk about`),
      el("p", { className: "wall-line" }, `${p.pickers} of ${plural(game.people, "person has", "people have")} picked, ${plural(p.picked.length, "statement", "statements")} so far.`),
      p.picked.length
        ? el(
            "ol",
            { className: "tallies wall-tallies" },
            ...p.picked.slice(0, 8).map((t) => el("li", {}, el("span", {}, t.statement.text), el("small", {}, plural(t.n, "pick", "picks")))),
          )
        : "",
    ];
  }
  if (game.phase === "finished") {
    return [
      el("p", { className: "eyebrow" }, "That was the evening"),
      el("h1", {}, "The prizes"),
      game.awards.length ? awardCards(game.awards) : el("p", { className: "note" }, "No prizes: nobody finished a round."),
      wallBoard(game.board),
    ];
  }
  return [
    el("p", { className: "eyebrow" }, r ? `After round ${r.number}` : "Speed updating"),
    el("p", { className: "wall-claim" }, r ? "Answer a few more, the next round is coming" : "Scan the code and say where you stand"),
    el(
      "p",
      { className: "wall-line" },
      `${game.answering} of ${plural(game.people, "person has", "people have")} answered, ${plural(game.answers, "answer", "answers")} so far.`,
    ),
    r && r.results.length ? el("div", { className: "wall-results" }, ...r.results.map(settledCard)) : "",
    wallBoard(game.board),
  ];
}

const where = route();
if (where.kind === "host") host(where.key);
else if (where.kind === "join") joining("updating", where.key);
else if (where.kind === "player") player(where.key);
else if (where.kind === "wall") wallScreen(where.key);
else start();
