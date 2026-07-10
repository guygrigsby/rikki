# 14. Rename to nevla

Status: Accepted (2026-07-10)

## Context

The language was named rikki on 2026-07-02 (ADR 0007), after
Rikki-Tikki-Tavi, the mongoose in Kipling's Jungle Book story; the
runner binary tk came from Tikki. The story was loved in childhood and
the name was chosen as an homage.

Kipling's story does not survive adult scrutiny. The standard
postcolonial reading of Rikki-Tikki-Tavi is imperial allegory: the
domesticated mongoose loyally defends an English family's bungalow
garden against the native cobras, the garden standing in for colonized
India kept safe for its colonizers. Kipling wrote "The White Man's
Burden"; the subtext is not an accident of modern reading. Once seen,
it cannot be unseen, and a project name is an endorsement renewed every
time it is typed.

The project is pre-users (v0.1.x), which is the cheapest moment a
rename will ever have. The break-early tenet exists for exactly this
shape of decision.

## Decision

Rename everything:

- language and setup binary: `nevla`, Hindi (नेवला) for mongoose. The
  story's failure was the colonial gaze on India; naming the animal in
  its own language walks the other direction.
- runner binary: `nv`.
- file extension: `.nv`.
- The mascot stays: the purple mongoose was never the problem, the
  story was.

Transparency over quiet history-editing: this ADR states the reason
plainly, the book's mascot page carries the same account, and
historical documents (prior ADRs, dated specs and plans) keep the old
name as written. They are records; falsifying them would repeat the
mistake in miniature.

## Consequences

- Every user-facing surface renames: crate, binaries, extension, PyPI
  package (`nevla`), Homebrew formula, playground, book, spec.
- The old PyPI package `rikki-lang` gets one final release whose
  description points here and says why.
- The playground domain moves off `rikki.aeryx.ai`; DNS is manual.
- The gputex holder protocol value `framework` changes to `"nevla"`,
  coordinated with gputex's PROTOCOL.md.
- Third rename (ichor, mongoose, rikki, nevla). Each had cause; the
  version stamp and break-early tenet are the honesty mechanism, and
  the cost stays acceptable precisely while there are no users to
  strand.
