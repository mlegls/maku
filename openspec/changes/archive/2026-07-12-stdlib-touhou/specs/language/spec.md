## MODIFIED Requirements

### Requirement: The engine ships no genre defaults
Genre vocabulary (bullet/enemy/player templates, hit/graze/shot rules, hp-death rules, phase templates) SHALL be library card code (`cards/lib/`, compile-time embedded, imported by bare name), not engine primitives. The core surface is a semantic kernel; surface vocabulary is lib macros over it, and optimization SHALL recognize macro expansion shapes, never names. Genre *data* SHALL follow the same rule: library templates carry their data tables (e.g. sprite-family → hitbox radius) as ordinary lib defs, resolved at macro time over literal meta forms, with every default overridable at the call site by the ordinary meta-merge rule (later maps win, explicit fields win over table lookups).

#### Scenario: Hand-written expansion
- **WHEN** card code hand-writes the exact form a lib macro would expand to
- **THEN** it evaluates and optimizes identically to the macro call

#### Scenario: Family hitbox default
- **WHEN** a bullet template call passes `:style {:family :gem}` with no explicit `:hitbox`
- **THEN** the primary collider radius is the lib table's `:gem` entry, and the same call with `:hitbox 0.15` uses 0.15
