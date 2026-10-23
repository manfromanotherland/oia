# SwiftUI skills for Óia

Research date: 2026-10-23. Both recommended skills were subsequently installed with the user's
authorization. This research compares actual `SKILL.md` instructions and their references,
rather than popularity or repository descriptions. Isolated compiler checks verified two
problematic companion examples. The research did not control applications or verify live UI
appearance.

## Recommendation

**Gambadio's `macos-golden-gate-design` is the best fit for the current native Mac UI problem.** It explicitly starts with standard SwiftUI/AppKit controls and system chrome, requires checking real SDK symbols, and demands validation of active, key, main, and inactive windows alongside appearance, tint, interaction, and accessibility states. It explicitly rejects declaring a design finished from static code or one screenshot. Those instructions address the failure in our verification process more directly than a general SwiftUI checklist. This is stronger guidance, not a guarantee that installing it fixes the image filter. [Actual skill](https://github.com/gambadio/macos-golden-gate-design/blob/b7068fcaf5b857fcebef36399e8f64843d2ced8e/SKILL.md), [native implementation reference](https://github.com/gambadio/macos-golden-gate-design/blob/b7068fcaf5b857fcebef36399e8f64843d2ced8e/references/swiftui-implementation.md)

Its macOS 27 design defaults must remain subordinate to Óia's product constraints: retain the sidebar-free media board, meaningful content cards, and macOS 15 fallbacks. Its own source hierarchy places the user's constraints first. Its live-app and Figma workflows do not authorize computer control in this conversation. [Source precedence and design contract](https://github.com/gambadio/macos-golden-gate-design/blob/b7068fcaf5b857fcebef36399e8f64843d2ced8e/SKILL.md#source-precedence)

**AvdLee's `swiftui-expert-skill` supplies the broader coding foundation.** It prefers native SwiftUI, permits necessary AppKit bridges, preserves availability fallbacks, and provides dedicated macOS scenes, window styling, and views references. [Skill entry point](https://github.com/AvdLee/SwiftUI-Agent-Skill/blob/204dba7c67256a8b707ee3fbc5759a824191974f/skills/swiftui-expert-skill/SKILL.md), [macOS window styling](https://github.com/AvdLee/SwiftUI-Agent-Skill/blob/204dba7c67256a8b707ee3fbc5759a824191974f/skills/swiftui-expert-skill/references/macos-window-styling.md), [macOS views and AppKit interop](https://github.com/AvdLee/SwiftUI-Agent-Skill/blob/204dba7c67256a8b707ee3fbc5759a824191974f/skills/swiftui-expert-skill/references/macos-views.md)

**Do not follow its custom segmented-control example for this task.** Its Liquid Glass chapter is primarily for iOS 26 and builds a “segmented control” from separate glass buttons, accent tint, and animated glass identities. That is a custom composition, with exactly the distinction from one native segmented control that matters here. The same chapter's useful availability, modifier-order, and container explanations do not establish native macOS selection artwork. [Actual Liquid Glass reference](https://github.com/AvdLee/SwiftUI-Agent-Skill/blob/204dba7c67256a8b707ee3fbc5759a824191974f/skills/swiftui-expert-skill/references/liquid-glass.md#segmented-control)

## Candidate comparison

| Candidate and inspected source | Useful content for Óia | Limits relevant to this failure |
| --- | --- | --- |
| **gambadio/macos-golden-gate-design** — `b7068fcaf5b857fcebef36399e8f64843d2ced8e`; skill at repository root | Native macOS controls, restrained glass, exact SDK/API verification, and explicit live window/interaction validation. | macOS 27-specific design defaults need Óia's product and availability constraints. Does not supply a proven recipe for this selector. [Skill and required validation](https://github.com/gambadio/macos-golden-gate-design/blob/b7068fcaf5b857fcebef36399e8f64843d2ced8e/SKILL.md#6-verify-live-behavior) |
| **petekp/agent-skills** — `skills/macos-app-design` | Native-first Mac design guidance, justified custom chrome, keyboard/focus/accessibility, and performance considerations. | Useful lighter checklist, but broader macOS 26/Tahoe guidance lacks Gambadio's detailed glass/API and active/inactive verification workflow. [Skill](https://github.com/petekp/agent-skills/blob/main/skills/macos-app-design/SKILL.md), [Mac design guide](https://github.com/petekp/agent-skills/blob/main/skills/macos-app-design/references/macos-design-guide.md) |
| **kmshdev/claude-swift-toolkit** — `plugins/apple-design-system/skills/macos-app-design` | The inspected Mac design guide is identical to petekp's, with additional workflow dependencies. | No substantive Mac-guide advantage here. Its linked Liquid Glass skill has a `Color.cyan.fill(...)` example that fails the installed Swift compiler and iOS-only availability guards that fail Óia's macOS 15 target. Do not import the wider bundle as trusted platform truth. [Mac skill](https://github.com/kmshdev/claude-swift-toolkit/blob/main/plugins/apple-design-system/skills/macos-app-design/SKILL.md), [companion glass skill](https://github.com/kmshdev/claude-swift-toolkit/blob/main/plugins/apple-design-system/skills/apple-liquid-glass-design/SKILL.md) |
| **AvdLee/SwiftUI-Agent-Skill** — `204dba7c67256a8b707ee3fbc5759a824191974f`; skill path `skills/swiftui-expert-skill` | Dedicated macOS window/control interop references; native-first operating rules; self-contained previews; bundled Instruments recording and analysis workflows. | Its glass reference is iOS-oriented and includes the custom button-based selector above. Its preview guidance does not establish active-window macOS glass optics. [Previews](https://github.com/AvdLee/SwiftUI-Agent-Skill/blob/204dba7c67256a8b707ee3fbc5759a824191974f/skills/swiftui-expert-skill/references/previews.md), [trace recording](https://github.com/AvdLee/SwiftUI-Agent-Skill/blob/204dba7c67256a8b707ee3fbc5759a824191974f/skills/swiftui-expert-skill/references/trace-recording.md) |
| **twostraws/SwiftUI-Agent-Skill** — `1eac100257af9068d5f041c1a1540b1790233716`; skill path `swiftui-pro` | Thorough review routing for APIs, data flow, performance, accessibility, and layout. Explicitly preserves the project's deployment target and separates unrelated modernization from requested changes. | Current assumptions and adaptive-layout examples are predominantly iOS 27/iPhone Duo. Useful system-container guidance, but the reviewed entry point and design references do not supply a macOS native-glass selector recipe. Some rules, such as one type per file, are style preferences rather than platform requirements. [Skill](https://github.com/twostraws/SwiftUI-Agent-Skill/blob/1eac100257af9068d5f041c1a1540b1790233716/swiftui-pro/SKILL.md), [design](https://github.com/twostraws/SwiftUI-Agent-Skill/blob/1eac100257af9068d5f041c1a1540b1790233716/swiftui-pro/references/design.md), [resizability](https://github.com/twostraws/SwiftUI-Agent-Skill/blob/1eac100257af9068d5f041c1a1540b1790233716/swiftui-pro/references/resizability.md) |
| **Dimillian/Skills** — `05ba982bfeb0d77d3c97d4542b0ee15034d05f84`; separate `swiftui-ui-patterns`, `swiftui-liquid-glass`, and `swiftui-performance-audit` skills | UI patterns explicitly prefer native controls and a Picker for discrete choices; validation asks for builds, important preview states, and accessibility. Performance audit distinguishes suspected causes from trace-backed findings and repeats captures to compare results. | Liquid Glass skill is explicitly iOS 26+, not an AppKit control guide. The reviewed control/glass references do not explain native lenses, dividers, or toolbar versus content rendering. [Native controls](https://github.com/Dimillian/Skills/blob/05ba982bfeb0d77d3c97d4542b0ee15034d05f84/swiftui-ui-patterns/references/controls.md), [UI patterns](https://github.com/Dimillian/Skills/blob/05ba982bfeb0d77d3c97d4542b0ee15034d05f84/swiftui-ui-patterns/SKILL.md), [Liquid Glass skill](https://github.com/Dimillian/Skills/blob/05ba982bfeb0d77d3c97d4542b0ee15034d05f84/swiftui-liquid-glass/SKILL.md), [performance audit](https://github.com/Dimillian/Skills/blob/05ba982bfeb0d77d3c97d4542b0ee15034d05f84/swiftui-performance-audit/SKILL.md) |

## Apple sources remain authoritative for the control

Apple says standard framework controls adopt the current design, and explains that toolbar glass is managed by the toolbar itself. For custom AppKit glass, the control must be inside `NSGlassEffectView.contentView`, rather than a sibling above a glass background. These are useful constraints; they do not guarantee that a content-area control gets identical toolbar artwork. [Adopting Liquid Glass](https://developer.apple.com/documentation/technologyoverviews/adopting-liquid-glass), [Build an AppKit app with the new design](https://developer.apple.com/videos/play/wwdc2025/310/)

Apple documents `.tabs` as a deliberately different macOS visual treatment for navigation compared with value selection. It should not be selected merely to hide dividers or assumed to reproduce the existing toolbar's selected lens. [TabsPickerStyle](https://developer.apple.com/documentation/swiftui/tabspickerstyle)

This Mac's Xcode also ships Apple's own [SwiftUI specialist instructions](</Applications/Xcode.app/Contents/PlugIns/IDEIntelligenceChat.framework/Versions/A/Resources/swiftui-specialist.idechatprompttemplate>), [Liquid Glass reference](</Applications/Xcode.app/Contents/PlugIns/IDEIntelligenceChat.framework/Versions/A/Resources/AdditionalDocumentation/SwiftUI-Implementing-Liquid-Glass-Design.md>), and [toolbar reference](</Applications/Xcode.app/Contents/PlugIns/IDEIntelligenceChat.framework/Versions/A/Resources/AdditionalDocumentation/SwiftUI-New-Toolbar-Features.md>). These are local Xcode resources, not a verified installable Codex skill package. Consult them in place; this note does not redistribute their contents.

## Installation

Both skills are installed in `~/.codex/skills` and available for the next turn:

- `macos-golden-gate-design` — native macOS design and validation guidance.
- `swiftui-expert-skill` — SwiftUI implementation, state, performance, and platform references.

The installation used these pinned sources. Gambadio's skill is at repository root, so
`--path .` needs an explicit destination name:

```sh
python3 "$HOME/.codex/skills/.system/skill-installer/scripts/install-skill-from-github.py" \
  --repo gambadio/macos-golden-gate-design \
  --path . --name macos-golden-gate-design \
  --ref b7068fcaf5b857fcebef36399e8f64843d2ced8e
```

AvdLee's actual skill directory is under `skills/`:

```sh
python3 "$HOME/.codex/skills/.system/skill-installer/scripts/install-skill-from-github.py" \
  --repo AvdLee/SwiftUI-Agent-Skill \
  --path skills/swiftui-expert-skill \
  --ref 204dba7c67256a8b707ee3fbc5759a824191974f
```

The installed entry points are [macos-golden-gate-design](</Users/ed/.codex/skills/macos-golden-gate-design/SKILL.md>) and [swiftui-expert-skill](</Users/ed/.codex/skills/swiftui-expert-skill/SKILL.md>). The path/name behavior was checked against the [local Codex installer](</Users/ed/.codex/skills/.system/skill-installer/scripts/install-skill-from-github.py>); AvdLee also publishes [canonical installation instructions](https://github.com/AvdLee/SwiftUI-Agent-Skill/blob/204dba7c67256a8b707ee3fbc5759a824191974f/llms.txt).

## Decision for Óia

Keep SwiftUI for scenes, view composition, state, and ordinary native controls, with narrow AppKit
bridges where a specific platform capability needs them. Retain the existing Rust core for shared
library, query, and persistence behavior. A full AppKit rewrite has no demonstrated benefit for
the reported control and shortcut problems.

The app already uses a native SwiftUI window, unified toolbar, segmented scope picker, search,
and menu commands. Its custom masonry layout is justified by the mixed visual board: geometry
is calculated before image decoding, and only the materialized card window becomes views.
Existing AppKit bridges provide continuous reader text selection, native video layers, and the
optional Original webpage. These are bounded platform integrations rather than reasons to move
the whole app to AppKit. See [app scenes](../../macos/Sources/Oia/App/OiaApp.swift),
[board controls](../../macos/Sources/Oia/Features/Oia/OiaLibraryView.swift),
[masonry integration](../../macos/Sources/Oia/Features/Oia/LazyMasonryBoard.swift), and
[reader selection](../../macos/Sources/Oia/Features/Reader/Markdown/SelectableTextView.swift).

The main scope control and floating image filters now use the same `LibraryFilterPicker`, whose
body contains only a SwiftUI `Picker`, `.pickerStyle(.segmented)`, and `.labelsHidden()`. The
floating caller adds native `.controlSize(.extraLarge)`, `.fixedSize()`, and one outer
`.glassEffect(.regular, in: .capsule)` where available. The framework owns all selection drawing,
tracking, and animation. There is no custom AppKit picker bridge, selected lens, segment geometry,
bezel override, tint, or custom selection animation. See the [shared picker and floating caller](../../macos/Sources/Oia/Features/Oia/FloatingImageTypePicker.swift).

Sharing the native primitive keeps behavior consistent without recreating a selection surface.
Toolbar and content contexts can still affect native rendering; shared source and unshown-window
checks do not establish identical active-window artwork. Live appearance remains unverified under
the code-only constraint.
Use native menu commands for the accepted A/G/P/U image-filter shortcuts, with typing and modal
guards, rather than depending on board focus. Keep the iOS-oriented custom segmented recipe
excluded from this implementation.

## What the skills do not validate

Gambadio supplies the most relevant validation workflow; it does not perform that validation merely by being installed. Compilation, binding tests, intrinsic geometry, and an unshown-window bitmap establish different facts from actual glass appearance, pointer interaction, and motion. Under the user's code-only restriction, those visual checks remain for the user; report them as unverified rather than repeating another styling implementation on a fixture's authority.
