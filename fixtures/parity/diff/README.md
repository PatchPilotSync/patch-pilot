Manifest diff fixtures (#15): a Windows Primary Rig rig and a macOS laptop.
Products are keyed by simple made-up IDs (`acme-verb`) because identity
resolution is not under test here.

| Product | Primary Rig (Windows) | Laptop (macOS) | Expected on laptop |
|---|---|---|---|
| Acme Verb | VST3 2.1.0 + AAX 2.1 | VST3 2.1.0 + AU | `Incomplete` (no AAX) |
| Acme Comp | VST3 3.0.1 | VST3 3.0 | `Outdated` |
| Acme Synth | VST3 `4.02` | VST3 `4.0.2` | `InParity` |
| Acme Gate | VST3 1.2 | — | `Missing` |
| Acme Delay | VST3 1.4 | VST3 1.5 | `NewerThanGolden` |
| Acme Gate JSFX | portable, hash `a…` | portable, hash `b…` | `Modified` |
| Acme Phaser | VST3 2.0 | VST3 `nightly` | `VersionUnknown` |
| Acme Tuner | — | AU 1.0.0 | `Extra` |
| Mystery Synth | — | no `product_id` | unresolved |

Diffing the other way (laptop as Primary Rig, PC as machine) makes Acme Tuner
`NotApplicable`: AU doesn't exist on Windows.
