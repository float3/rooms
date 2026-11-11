# updating

[![CI](https://github.com/float3/updating/actions/workflows/ci.yml/badge.svg)](https://github.com/float3/updating/actions/workflows/ci.yml)

speed updating for a room of people, at
[updating.rationality-munich.com](https://updating.rationality-munich.com).

everyone says where they stand on a set of statements, then talks to
whoever disagrees with them most, a few minutes at a time, and says where
they stand afterwards. the format comes from EA groups, who run it on
paper; thanks to Noah Birnbaum for telling us about it.

built on [rooms](https://github.com/float3/rooms).

## how an evening goes

the facilitator opens a room, picks topics and puts the join link on the
wall. everyone scans it and gives a name, then answers statements:

| | |
| --- | --- |
| as a probability, the default | how confident you are that it is true, 0 to 100%; 50% means no idea either way |
| as agreement | from 1, strongly disagree, to 7, strongly agree |

most statements can be put either way, so the facilitator chooses. a room
starts with all probabilities: people we asked found an explicit
probability easier to understand than a scale, and arguing about how
likely something is makes for better conversations than whether you agree
with it. the other choices are all agreement, where a question like "What
is the chance that a fish feels pain?" becomes the claim "A fish feels
pain.", and as written, mostly agreement with some probabilities. the
agreement scale has seven points because seven-point scales are the ones
with evidence behind them.

in the **bingo** mode each person is dealt a grid of statements, 3×3 to
5×5. the statements they talk about are marked, and a full line is a
bingo. in the **open** mode everybody sees every statement from the chosen
topics and answers as many as they like.

in the **pick** mode nobody answers at first: everybody sees the whole
pool from the chosen topics and picks up to three statements they would
like to talk about, and the wall shows what is being picked. when the
facilitator closes the picks, the room plays every statement somebody
picked, once however many picked it, the most picked first and ties in
the order of the pool. people who picked nothing, or join later, answer
the same statements as everybody else. the picks cannot be closed while
nothing is picked.

the **free-for-all** mode is for browsing on your own time, over an
evening or longer. every statement shows how many people have answered
it, and the most answered come first, so a room with a large deck
converges on the statements people care about. you see who answered a
statement, and what they said, only after answering it yourself: knowing
it first would anchor your answer. each phone keeps pointing its owner at
whoever disagrees with them most ("Josh, go and talk to Jake"), and the
facilitator can still run timed rounds when the room wants them.

by default the free-for-all mode deals statements one at a time, quick
fire: answer and see straight away who else answered it and where they
stand, or skip it. the next card leans towards what others have answered,
most of all where they disagree, and towards the topics you keep
answering, with a little noise so two phones do not show the same order;
every fourth card is one few people have answered, so new statements get
their first answers too. browsing everything by topic is the other tab.

the facilitator starts a round. the server pairs everybody with the person
they disagree with most, on the statement they are furthest apart on, and
a clock runs: two to ten minutes. when it runs out, each phone asks how
much the conversation changed their mind, with the slider where they left
it. then the next round, with somebody new.

between rounds each phone also lists the people who disagree with it
most, and on what, so people can go and find each other by themselves.

## pairing

every possible pair is scored by the statement they are furthest apart on,
preferring statements neither has talked about yet, and pairs who have
already talked come last. pairs are taken greedily, biggest disagreement
first, then repaired: wherever swapping partners seats two more people, the
swap is made. with an odd number, the one left over joins the pair whose
statement they are furthest from, as a three. people who have answered
nothing anybody else has answered are told they were not paired, and why.

somebody away is not paired, and not suggested to anybody to go and find.
a phone says so with "sit the next round out", and is back with "I'm
back" or by answering anything. the host can mark somebody away in the
people table, for whoever left without saying. nobody is marked away
because their phone dropped its connection: phones sleep in pockets in
the middle of a conversation. somebody who has left for good the host
can also remove.

both scales are compared on one from 0 to 200: from 0% to 100% is as far
apart as from 1 to 7. one step of agreement is 33 points, one percentage
point is 2. an answer that moves 20 points, one step or ten percentage
points, counts as an update.

## who wins

points for three things, over every round:

| | |
| --- | --- |
| closing the gap | how far apart you and your partner were before, less after, on the scale under pairing. both of you get it, whoever moved: the conversation is a joint task, not a debate |
| guessing your partner | after a round each phone asks where your partner stands now, before showing it. 100 for exactly right, falling with the square of the miss, nothing from fifty percentage points (three steps) away. squared error is a proper score for a best guess, so the way to score is to guess what you really expect |
| "I learned something from you" | one tap per partner per round, worth 25 to the partner |

moving somebody towards you is no longer the score: that made the
evening a debate, and it paid to start from an extreme. it is still
counted, as one prize among several. at the end the wall shows the
prizes: best listener, most worth listening to, best conversation, open
mind (who moved furthest towards their partners), most persuasive, and
in the bingo mode, bingo.

the wall shows a pair's new answers only once both of them have answered
and guessed, because the guesses are made blind. a phone shows you your
partner's new answer only once you have said where you stand and locked
in your guess, and from then on your answer this round stays as it was:
you can still change your answer on your card, but not the round's. so
nobody sets their own number while looking at their partner's.

each pair gets a meeting spot, an animal: "🦊 Fox". it is large on both
phones and on the wall, so two people can find each other in a full room.

in the bingo mode, pairing still goes by disagreement first, but between
statements about as contested it prefers one that brings either person
closer to a line, most of all the square that completes one. without
that, a 4×4 line needed four rounds on exactly the right statements, and
nearly nobody got one.

## the warm-up

before the statements, the facilitator can ask a few questions that have
a right answer. each is a puzzle with exact odds: a bag picked by a coin,
a medical test with a known false alarm rate, a coin that may be a trick
one. the answer really is drawn at random, and each phone is dealt its
own clue drawn from it: a ball from the bag, a test result, a few flips.

everybody says how likely the claim is from the setup alone, then again
with their clue, and may compare clues with their neighbours. then the
answer comes out. answers are scored in bits against a coin flip,
log2(p / 50%) for the chance you gave what turned out true: 50% scores
nothing, sure and right nearly a bit, sure and wrong a lot. each phone
also shows where Bayes would have been with its clue, from the true start
and from where its owner started.

the Bayes bot is par: it starts from the true prior, gets a clue like
everybody else, and updates exactly. it is on the warm-up board with the
room.

## the wall

the facilitator's screen has "Open the wall view": a separate page,
`/w/<key>`, for the projector. it has its own key, which runs nothing, so
its address can be on the screen. it shows the join code and who is in;
the warm-up question and, once revealed, how the room answered; during a
round the clock and every group's spot and statement, but not who is in
which group; then how many are done, and for each statement whose groups
have all settled, everybody's answers before and after as unnamed dots;
the top of the board; at the end, the prizes. it never shows anybody's
answer next to their name.

the facilitator's own screen has one slim bar of buttons, the same
overview with names and answers, the join link, and who is in, with a
dot for whose phone is connected and a button to take somebody out. the
room's settings and the detailed statistics fold away underneath.

## statements

167 in `src/statements.rs`, by topic: effective altruism, AI
safety, animals and veganism, metaethics, ethics, consciousness and mind,
metaphysics, knowledge and rationality, society and politics, the future,
and a warm-up set. a bingo room samples about twice a card's worth, so
that cards overlap enough for people to have statements in common.

the facilitator can add their own, one per line; a line starting with `%`
asks for a probability.

anybody can suggest a statement from the start page or from their phone.
suggestions are appended to a file on the server, one JSON line each, and
nothing serves them back:

```sh
sudo cat /var/lib/private/updating/suggestions.jsonl
```

## four links

| | |
| --- | --- |
| `/h/<key>` | runs the room. Keep it; it is the only way back in. |
| `/j/<key>` | joins it. This is the one on the wall. |
| `/p/<key>` | one player's own, kept by their phone. |
| `/w/<key>` | the wall, for a projector. It shows and changes nothing secret. |

a name held by a phone that is connected is refused, so two people
called Alex stay two people; a phone that kept its link goes straight
back in. joining and presence are `rooms::lobby`'s.

the key in the link is the only access control: 24 random characters and
no accounts. see [rooms](https://github.com/float3/rooms) for what that
does and does not protect.

the facilitator sees everything. a player is sent their own answers, and
once paired, where their partners stand on the statement they were given:
that is the conversation. between rounds they are sent the five people
who disagree with them most, with both answers on that one statement.

## running it

```sh
nix run github:float3/updating
```

| | |
| --- | --- |
| `UPDATING_ADDR` | what to listen on. `127.0.0.1:8102`. |
| `UPDATING_STATE` | where the rooms are kept. `rooms.json`. |
| `UPDATING_SUGGESTIONS` | where suggested statements go. `suggestions.jsonl`. |

the NixOS module takes a port and the domains to serve on:

```nix
services.updating = {
  enable = true;
  port = 8102;
  domains = ["updating.example.com"];
};
```

rooms are forgotten a day after anyone last looked at one.

## development

```sh
nix develop --command cargo test
```

the page is TypeScript in `client/`, compiled into the binary by the build
script with [rooms-client](https://github.com/float3/rooms). no
JavaScript is written by hand here.
