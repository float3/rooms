# numbers

number games for a room of people, at
[numbers.rationality-munich.com](https://numbers.rationality-munich.com).

everyone picks a number, and the rule decides who wins. under every rule
here, what a number is worth depends on what everybody else picked.

built on [rooms](../README.md).

## the rules

| | |
| --- | --- |
| Closest to a fraction of the average | 1/3, 1/2, 2/3, or the average itself |
| Closest to a fraction of the middle number | the same fractions |
| The lowest number nobody else picked | whole numbers; some rounds have no winner |
| The highest number nobody else picked | |
| The highest number below the average | |
| The number the most people pick | nobody wins if no two people pick the same number |
| The number furthest from everybody else's | measured to the nearest other pick |
| The highest number, if the total is no more than a limit | over the limit, nobody wins |

the first one is the Keynesian beauty contest. the pages state the rules
and nothing else: working out what to pick is the game.

## what a number can be

the host chooses per round: whole numbers, decimals, or expressions such as
`1,001/3`, `2^10 - 1` or `sqrt(2)`. the three rules that count matching
picks take whole numbers, or expressions that come out whole. a comma
between groups of three digits is a thousands separator (`1,001` is a
thousand and one); any other comma is a decimal point (`1,5` is one and a
half). the phone shows what an answer reads as before it is sent, and the
reveal shows each sum as it was typed.

the phone also says plainly whether your number is in: not sent yet, sent
and still changeable, changed but not sent, or locked once the host stops
taking numbers.

## nobody sees a number until the reveal

including the host. until the host reveals the round, every screen gets
the rule, the range, and a count of who has picked. then every number
appears at once, with the winner, the average, the middle number and the
number the room was aiming at.

## the wall

the host's screen has "Open the wall view": a read-only page for a
projector, with the QR code, who is in, the rule in large type and how many
have picked. at the reveal it shows the numbers as unnamed dots along the
range, the winning ones in gold, and the top three of the evening by name.
it never puts a name next to a number.

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
nix run github:float3/rooms#numbers
```

| | |
| --- | --- |
| `NUMBERS_ADDR` | what to listen on. `127.0.0.1:8098`. |
| `NUMBERS_STATE` | where the rooms are kept. `rooms.json`. |

the NixOS module, `nixosModules.numbers` in the workspace flake, takes a port and the domains to serve on:

```nix
services.numbers = {
  enable = true;
  port = 8098;
  domains = ["numbers.example.com"];
};
```

rooms are forgotten a day after anyone last looked at one.

## development

the page is TypeScript in `client/`, compiled into the binary by the build
script with [rooms-client](../rooms-client). `esbuild`
bundles it and `tsc` checks it, both from the dev shell. no JavaScript is
written by hand here.

```sh
nix develop --command cargo test -p numbers
```
