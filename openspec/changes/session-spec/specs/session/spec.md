# session

## ADDED Requirements

### Requirement: The sim is a deterministic fold over two tapes
A session SHALL be fully determined by the card plus two tapes: the input tape (one `Inputs` record per tick — recorded channel VALUES) and the command tape (program changes — `add`/`swap`, entity capacity changes — stamped with their tick). There SHALL be no hidden host state: even the stock player rig enters the timeline as a command-tape entry.

#### Scenario: Replay from tapes
- **WHEN** the same card and tapes are folded twice
- **THEN** every tick's world state and render output are identical

### Requirement: Any tick is reachable exactly
Scrubbing SHALL reach any past tick as nearest-snapshot + re-step, with command-tape entries re-applied at their recorded ticks — so scrubbing across an `add`/`swap` boundary reproduces the post-change timeline exactly.

#### Scenario: Scrub across a swap
- **WHEN** the user seeks to a tick before a recorded `swap` and steps forward past it
- **THEN** the sim re-applies the swap at its recorded tick and subsequent states match the original run

### Requirement: Resume after rewind branches
Resuming from a rewound position SHALL truncate the future of both tapes (a branch); the discarded future is not recoverable through the session.

#### Scenario: Rewind and resume
- **WHEN** the user scrubs backward and resumes
- **THEN** ticks after the resume point are newly computed and the old future no longer exists on the tapes

### Requirement: Input bindings are host configuration outside the replay
Key/axis/constant bindings SHALL be host-side configuration invisible to cards; the input tape records resulting channel values only, so replays and scrubbing are unaffected by how a value was produced.

#### Scenario: Rebinding mid-session
- **WHEN** the user rebinds a key between two runs of the same tape
- **THEN** replay output is identical, because the tape carries values, not keys

### Requirement: Snapshots are a cache, never semantics
Snapshot cadence, thinning, and retention SHALL NOT affect observable behavior — only scrub latency. The tick-0 baseline SHALL always survive thinning, so every tick stays reachable.

#### Scenario: Thinned history
- **WHEN** old snapshots are auto-thinned to logarithmic density
- **THEN** seeking to any tick still yields the identical state, merely via a longer re-step
