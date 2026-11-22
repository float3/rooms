# buttons

red or blue, for a room of people.

everybody presses one of two buttons on their phone. if more than a share
the host sets (half by default) press blue, everybody who pressed a button
lives. if not, only the ones who pressed red do.

red lives whatever the room does. blue lives only if enough of the room
presses it too. the page states the rule and nothing else: working out
what to press is the game.

built on [rooms](../README.md).

## the rule

- blue has to get more than the share, so at 50% exactly half is not
  enough. the count is in whole presses: two of four is not more than
  half, three of four is.
- whoever presses nothing is not counted, on either side.
- the host sets the share per round, from 0 to 99. at 0 a single blue
  press is enough.
- a press can be changed until the host stops taking presses.

## nobody sees a press until the reveal

including the host. until the host reveals the round, every screen gets
the rule and a count of who has pressed. then the split appears, with who
pressed what and who lived.

## the wall

the host's screen has "Open the wall view": a read-only page for a
projector, with the QR code, who is in, the rule in large type and how
many have pressed. at the reveal it shows the split against the share blue
needed. it never puts a name next to a button, and has no scoreboard,
because who lived through a round says what they pressed.

## joining

a name already held by a connected phone is refused ("That name is
taken"), so two people called Alex do not become one player. a phone that
lost its link gets back in with the key it kept, or by its name once it has
been gone a few seconds. the host can take somebody out from the list of
who is in; their link stops working.

## four links

| | |
| --- | --- |
| `/h/<key>` | runs the room. Keep it; it is the only way back in. |
| `/j/<key>` | joins it. This is the one on the wall. |
| `/p/<key>` | one player's own, kept by their phone. |
| `/w/<key>` | the wall. It shows only what the whole room may see, and runs nothing. |

the key in the link is the only access control: 24 random characters, and
no accounts. see [rooms](../README.md) for what that
does and does not protect.

## running it

```sh
nix run github:float3/rooms#buttons
```

| | |
| --- | --- |
| `BUTTONS_ADDR` | what to listen on. `127.0.0.1:8106`. |
| `BUTTONS_STATE` | where the rooms are kept. `rooms.json`. |

the NixOS module, `nixosModules.buttons` in the workspace flake, takes a port and the domains to serve on:

```nix
services.buttons = {
  enable = true;
  port = 8106;
  domains = ["buttons.example.com"];
};
```

the hall serves the same game under `/buttons`.

rooms are forgotten a day after anyone last looked at one.

## development

the page is TypeScript in `client/`, compiled into the binary by the build
script with [rooms-client](../rooms-client). `esbuild`
bundles it and `tsc` checks it, both from the dev shell. no JavaScript is
written by hand here.

```sh
nix develop --command cargo test -p buttons
```
