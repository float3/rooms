// Reading what a player types, the same way the server does in
// src/answer.rs, so the phone can show how an answer reads before it is
// sent. The server reads it again and has the last word.

export type Answers = "whole" | "decimal" | "expression";

export type Reading = { value: number } | { error: string };

const MAX_TEXT = 80;
const MAX_DEPTH = 32;

class Refused extends Error {}

const isDigit = (c: string | undefined): boolean => c !== undefined && c >= "0" && c <= "9";
const isLetter = (c: string | undefined): boolean => c !== undefined && /^[A-Za-z]$/.test(c);

/** Digit groups split by `by`: one to three digits first, then threes. */
function grouped(text: string, by: string): boolean {
  const groups = text.split(by);
  const first = groups[0];
  return (
    groups.length > 1 &&
    /^[1-9][0-9]{0,2}$/.test(first) &&
    groups.slice(1).every((g) => /^[0-9]{3}$/.test(g))
  );
}

/** One number as written: `1,001` is a thousand and one, `1,5` one and a half. */
function literal(written: string): number {
  const bad = new Refused("That number is written in a way this cannot read.");
  const comma = written.lastIndexOf(",");
  const dot = written.lastIndexOf(".");
  let thousands = "";
  let point = "";
  if (comma >= 0 && dot >= 0) [thousands, point] = comma > dot ? [".", ","] : [",", "."];
  else if (comma >= 0) [thousands, point] = grouped(written, ",") ? [",", ""] : ["", ","];
  else if (dot >= 0)
    [thousands, point] = written.split(".").length > 2 && grouped(written, ".") ? [".", ""] : ["", "."];
  let whole = written;
  let fraction: string | null = null;
  if (point) {
    const at = written.lastIndexOf(point);
    whole = written.slice(0, at);
    fraction = written.slice(at + 1);
  }
  if (thousands && whole.includes(thousands) && !grouped(whole, thousands)) throw bad;
  const digits = thousands ? whole.split(thousands).join("") : whole;
  if (!/^[0-9]*$/.test(digits)) throw bad;
  if (fraction !== null) {
    if (!/^[0-9]*$/.test(fraction) || (digits === "" && fraction === "")) throw bad;
  } else if (digits === "") throw bad;
  return Number(`${digits || "0"}.${fraction || "0"}`);
}

class Reader {
  private at = 0;
  constructor(private readonly text: string) {}

  peek(): string | undefined {
    while (this.at < this.text.length && /\s/.test(this.text[this.at])) this.at++;
    return this.text[this.at];
  }

  bump(): void {
    this.at++;
  }

  done(): boolean {
    return this.peek() === undefined;
  }

  signedLiteral(): number {
    let sign = 1;
    for (let c = this.peek(); c === "-" || c === "+" || c === "−"; c = this.peek()) {
      if (c !== "+") sign = -sign;
      this.bump();
    }
    const c = this.peek();
    if (isDigit(c) || c === "." || c === ",") return sign * this.number();
    throw new Refused("Type a number.");
  }

  number(): number {
    const start = this.at;
    while (isDigit(this.text[this.at]) || this.text[this.at] === "." || this.text[this.at] === ",") this.at++;
    let value = literal(this.text.slice(start, this.at));
    const exponent = /^[eE]([-+]?[0-9]+)/.exec(this.text.slice(this.at));
    if (exponent) {
      value *= 10 ** Number(exponent[1]);
      this.at += exponent[0].length;
    }
    return value;
  }

  sum(depth: number): number {
    let value = this.product(depth);
    for (;;) {
      const c = this.peek();
      if (c === "+") {
        this.bump();
        value += this.product(depth);
      } else if (c === "-" || c === "−") {
        this.bump();
        value -= this.product(depth);
      } else return value;
    }
  }

  product(depth: number): number {
    let value = this.signed(depth);
    for (;;) {
      const c = this.peek();
      if (c === "*" || c === "×" || c === "·") {
        this.bump();
        value *= this.signed(depth);
      } else if (c === "/" || c === "÷" || c === ":") {
        this.bump();
        const by = this.signed(depth);
        if (by === 0) throw new Refused("That divides by nought.");
        value /= by;
      } else if (c === "(" || c === "π" || c === "√" || isLetter(c)) {
        value *= this.power(depth);
      } else return value;
    }
  }

  signed(depth: number): number {
    if (depth > MAX_DEPTH) throw new Refused("That nests deeper than this will work out.");
    const c = this.peek();
    if (c === "-" || c === "−") {
      this.bump();
      return -this.signed(depth + 1);
    }
    if (c === "+") {
      this.bump();
      return this.signed(depth + 1);
    }
    return this.power(depth);
  }

  power(depth: number): number {
    const base = this.atom(depth);
    if (this.peek() === "^") {
      this.bump();
      return base ** this.signed(depth + 1);
    }
    return base;
  }

  atom(depth: number): number {
    if (depth > MAX_DEPTH) throw new Refused("That nests deeper than this will work out.");
    const c = this.peek();
    if (c === "(") {
      this.bump();
      const value = this.sum(depth + 1);
      if (this.peek() !== ")") throw new Refused("A bracket is not closed.");
      this.bump();
      return value;
    }
    if (isDigit(c) || c === "." || c === ",") return this.number();
    if (c === "π") {
      this.bump();
      return Math.PI;
    }
    if (c === "√") {
      this.bump();
      return root(this.power(depth + 1));
    }
    if (isLetter(c)) {
      const start = this.at;
      while (isLetter(this.text[this.at])) this.at++;
      const name = this.text.slice(start, this.at).toLowerCase();
      if (name === "pi") return Math.PI;
      if (name === "e") return Math.E;
      if (name === "sqrt" || name === "ln" || name === "log" || name === "abs") {
        const x = this.power(depth + 1);
        if (name === "sqrt") return root(x);
        if (name === "abs") return Math.abs(x);
        if (x <= 0) throw new Refused("A logarithm needs a number above nought.");
        return name === "ln" ? Math.log(x) : Math.log10(x);
      }
      throw new Refused("That has a word in it this does not know.");
    }
    if (c === ")") throw new Refused("There is a closing bracket with nothing open.");
    if (c === undefined) throw new Refused("That ends too soon.");
    throw new Refused("That has a sign in it this does not know.");
  }
}

function root(x: number): number {
  if (x < 0) throw new Refused("A square root needs a number of at least nought.");
  return Math.sqrt(x);
}

/** Reads what a player typed as this kind of answer. */
export function read(answers: Answers, typed: string): Reading {
  const text = typed.trim();
  if (text === "") return { error: "Type a number first." };
  if ([...text].length > MAX_TEXT) return { error: "That is longer than an answer can be." };
  try {
    const reader = new Reader(text);
    let value: number;
    if (answers === "expression") {
      value = reader.sum(0);
      if (!reader.done())
        throw new Refused(
          reader.peek() === ")"
            ? "There is a closing bracket too many."
            : "Part of that is not something this can work out.",
        );
    } else {
      value = reader.signedLiteral();
      if (!reader.done()) throw new Refused("This round takes a plain number, not a sum.");
    }
    if (!isFinite(value)) return { error: "That does not come to a number." };
    if (answers === "whole" && !Number.isInteger(value)) return { error: "This round takes whole numbers." };
    return { value };
  } catch (e) {
    if (e instanceof Refused) return { error: e.message };
    throw e;
  }
}
