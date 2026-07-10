# nevla design

The single source for brand and visual decisions. Append decisions here;
don't scatter them through code comments.

## Mascot

Nevli the mongoose (`logo.png`; editable source `logo.af`, Affinity
Designer). The mascot is the brand: it appears in the README (top right,
200px, below the title so GitHub's h1 rule doesn't cut her) and the
playground header. Licensed CC BY 4.0 (see `art/LICENSE`); the code is MIT
and stays that way.

Named Rikki until 2026-07-10; renamed with the language over the Kipling
story's colonial subtext (ADR 0014, and the book's mascot page has the
full account). Same artwork: the animal was never the problem.

## Color

- Brand: `#C86FB9` (decided 2026-07-09), the mascot's purple. Use it for
  primary actions, links, focus rings, and accents; never for body text.
- Hover/active darkens the brand: `#a94f9a`.
- Dark surfaces are plum-tinted, not gray: background `#1a1418`, panels
  `#241c22`, text `#ece5ea`.
- Light surfaces: background `#faf7f9`, panels `#ffffff`, text `#2a2028`.
- Errors: `#d4526e`, one color for compile and runtime; the label carries
  the distinction.
- Both themes ship; `prefers-color-scheme` decides.

## Type

System stacks only, no webfonts: UI in the platform sans, code in the
platform mono. Code is the point of every nevla surface; it gets the
visual priority.

## Playground (decided 2026-07-09)

- Frontend-only WASM on GitHub Pages; no backend, ever. Sharing encodes
  the program into the URL fragment.
- A plain textarea editor until it hurts; no editor framework, no CDN
  dependencies. The page is fully self-contained. It hurt once already:
  compile errors were unfindable, so the textarea gained a synced line
  gutter that highlights diagnostic lines in the error color (wrap off,
  so gutter and text can never drift).
- The python bridge is honestly absent: the py example runs and shows the
  real "python is not available in this build" error rather than hiding
  the boundary.
