// One room for an evening, in a browser.
//
// The host's screen shows the join code and lets the host pick the game.
// A player's screen shows the game the room is playing, in a frame that
// moves to the next game when the host switches. The games themselves are
// the same pages they are on their own servers.

import {
  api,
  byId,
  esc,
  hide,
  html,
  joining,
  joinLink,
  opening,
  plural,
  qrInto,
  route,
  show,
  showJoin,
  text,
  watch,
} from "./rooms";

interface Game {
  id: string;
  title: string;
}

interface Playing {
  id: string;
  title: string;
  url: string | null;
}

interface HostView {
  join: string;
  people: string[];
  games: Game[];
  playing: Playing | null;
}

interface PlayerView {
  name: string;
  people: number;
  playing: Playing | null;
}

/** One line about each game, for the host choosing between them. */
const ABOUT: Record<string, string> = {
  petrov: "Two sides, one missile each, and false alarms that look real.",
  numbers: "Everyone picks a number, and a rule decides who wins.",
  estimate: "Give a range you are 90% sure of. Learn how sure you should be.",
  updating: "Pair up with whoever disagrees with you most, and talk.",
};

/** The names, for the start page, which has no room to ask. */
const NAMES: Record<string, string> = {
  petrov: "Petrov Day",
  numbers: "Number games",
  estimate: "Estimation",
  updating: "Speed updating",
};

/** Points a frame at a page, unless it is already there, so that a
 *  redraw does not reload the game. */
function aim(id: string, url: string | null): void {
  const frame = byId<HTMLIFrameElement>(id);
  frame.hidden = url === null;
  if (url !== null && frame.dataset.url !== url) {
    frame.dataset.url = url;
    frame.src = url;
  }
  document.body.classList.toggle("playing", url !== null);
}

function start(): void {
  html(
    "game-list",
    Object.entries(ABOUT)
      .map(([id, about]) => `<li><b>${esc(NAMES[id] ?? id)}.</b> ${esc(about)}</li>`)
      .join(""),
  );
  opening();
}

// ------------------------------------------------------------------- host

function host(key: string): void {
  show("host");
  document.body.classList.add("wide");
  const at = (path: string) => `/api/h/${encodeURIComponent(key)}${path}`;
  let miniFor = "";

  const play = async (tool: string | null): Promise<void> => {
    const body = await api<{ view: HostView }>("POST", at("/play"), { tool });
    draw(body.view);
  };

  function draw(view: HostView): void {
    const playing = view.playing;
    text("where", playing ? `Playing ${playing.title}` : "You are the host");

    // The join code: big in the lobby, small above a game.
    showJoin(view.join);
    const link = joinLink(view.join);
    if (miniFor !== link) {
      miniFor = link;
      qrInto("mini-qr", link);
      const mini = byId<HTMLAnchorElement>("mini-link");
      mini.textContent = link.replace(/^https?:\/\//, "");
      mini.href = link;
    }
    hide("lobby", playing !== null);
    hide("strip", playing === null);

    const count =
      view.people.length === 0
        ? "Nobody has joined yet"
        : `${plural(view.people.length, "person", "people")} in the room`;
    text("people-count", count);
    text("mini-count", count);
    html(
      "people",
      view.people.map((name) => `<li>${esc(name)}</li>`).join(""),
    );

    html(
      "games",
      `${view.games
        .map(
          (game) => `<button class="game${playing?.id === game.id ? " on" : ""}" data-tool="${esc(game.id)}" aria-pressed="${playing?.id === game.id}">
            <b>${esc(game.title)}</b>
            <span>${esc(ABOUT[game.id] ?? "")}</span>
          </button>`,
        )
        .join("")}${
        playing
          ? `<button class="game lobby-button" data-tool="" aria-pressed="false"><b>Back to the lobby</b><span>Show the join code again.</span></button>`
          : ""
      }`,
    );
    byId("games").classList.toggle("compact", playing !== null);
    aim("host-frame", playing?.url ?? null);
  }

  byId("games").addEventListener("click", (event) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>("button[data-tool]");
    if (!button || button.getAttribute("aria-pressed") === "true") return;
    const tool = button.dataset.tool || null;
    button.disabled = true;
    play(tool)
      .catch((e: unknown) => {
        byId("error").textContent = e instanceof Error ? e.message : String(e);
        byId("error").hidden = false;
      })
      .finally(() => {
        button.disabled = false;
      });
  });

  watch<HostView>(at(""), draw);
}

// ----------------------------------------------------------------- player

function player(key: string): void {
  show("player");
  const at = (path: string) => `/api/p/${encodeURIComponent(key)}${path}`;

  function draw(view: PlayerView): void {
    const playing = view.playing;
    text(
      "where",
      playing ? `${view.name} · ${playing.title}` : `You are ${view.name}`,
    );
    hide("waiting", playing !== null && playing.url !== null);
    text(
      "waiting-count",
      playing && playing.url === null
        ? `The room is playing ${playing.title}. Joining you in…`
        : `${plural(view.people, "person is", "people are")} in the room.`,
    );
    aim("player-frame", playing?.url ?? null);
  }

  watch<PlayerView>(at(""), draw);
}

const where = route();
if (where.kind === "host") host(where.key);
else if (where.kind === "join") joining("hall", where.key);
else if (where.kind === "player") player(where.key);
else start();
