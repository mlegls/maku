## ADDED Requirements

### Requirement: Compatibility surface removal is corpus-gated
Before removing a compatibility spelling or interpretation, the project SHALL inventory its canonical replacement and scan checked-in cards, translations, library sources, documentation snippets, web-demo copies, and embedded test-card fixtures. Repository consumers SHALL migrate to canonical forms, while tests intentionally covering removed behavior SHALL become rejection or migration-diagnostic tests. Implementation paths labeled legacy but still required by canonical language behavior MUST NOT be removed solely by this audit.

#### Scenario: Unused alias is removed
- **WHEN** `value-or` or an old Touhou `spawn-*` alias has a documented canonical replacement and no remaining repository consumer after migration
- **THEN** the alias is removed, canonical examples pass, and use of the removed spelling produces an actionable diagnostic

#### Scenario: Canonical construct has legacy backing
- **WHEN** canonical `pather` behavior still depends on the trace cache or canonical motion evaluation still depends on migration-state fallback
- **THEN** that implementation remains until separate equivalence tests and caller migration prove it redundant

#### Scenario: Direct render-field compatibility differs from library metadata
- **WHEN** auditing `:facing`, `:opacity`, or `:pts`
- **THEN** direct legacy render-map interpretation is distinguished from valid current entity metadata translated by a genre library, and only the selected compatibility interpretation is removed

### Requirement: Published examples use canonical language
All `.maku` examples, translations, tutorials, browser demo cards, and package documentation included in a release SHALL use canonical current language and library forms except examples explicitly labeled as migration or rejection cases.

#### Scenario: Release corpus scan
- **WHEN** the release compatibility scanner runs
- **THEN** it reports no unlabelled use of the selected removed forms across source files and extracted documentation examples
