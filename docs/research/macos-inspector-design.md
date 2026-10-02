# macOS inspector design: Tag input and Liquid Glass

Research date: 2026-10-02. Sources are current Apple Human Interface Guidelines, Apple developer documentation, and WWDC25 sessions. Recommendations below are design inferences from those sources, not Apple rules for Óia specifically.

## Recommendation

Keep the inline **Add a tag…** control as a native, rounded text field with its own clear boundary. Do **not** add a separate Liquid Glass effect to the field simply because its Inspector is glass. The Inspector is the structural surface; the text field is a small, precise input inside it. Apple says system controls adopt the current design automatically, advises sparing use of custom glass, and cautions against layering glass controls on one another. [Materials](https://developer.apple.com/design/human-interface-guidelines/materials), [Adopting Liquid Glass](https://developer.apple.com/documentation/technologyoverviews/adopting-liquid-glass)

Use the persistent **Tags** heading plus the field's short placeholder, and keep the native completion menu. A Tag name is the short, specific input for which Apple recommends a text field; a separate label remains useful after placeholder text disappears. SwiftUI's `textInputSuggestions` with `textInputCompletion` presents completion choices for text input. Choosing a suggestion fills the field; Óia's Return action then adds the Tag. [Text fields](https://developer.apple.com/design/human-interface-guidelines/text-fields), [textInputSuggestions](https://developer.apple.com/documentation/swiftui/view/textinputsuggestions%28_%3A%29), [textInputCompletion](https://developer.apple.com/documentation/swiftui/view/textinputcompletion%28_%3A%29)

## Separate the surfaces

| Region | Apple guidance | Implication for Óia |
| --- | --- | --- |
| Toolbar | Navigation, search, and frequent actions belong in the toolbar. The system groups interactive items on Liquid Glass; informational titles should not acquire a button-like glass backing. Prefer standard toolbar components and a restrained, usually monochrome treatment over colorful content. [Toolbars](https://developer.apple.com/design/human-interface-guidelines/toolbars), [Build an AppKit app with the new design](https://developer.apple.com/videos/play/wwdc2025/310/) | Let the system style toolbar controls. Reserve a prominent tint for an actual primary action, rather than the Inspector's routine Tag input. |
| Inspector | On macOS, a system inspector has an **edge-to-edge** glass treatment beside content, while a navigation sidebar floats over content. Apple's standard split-view/inspector APIs supply these treatments. Compact Mac controls retain rounded rectangles and work in dense inspectors. [Build an AppKit app with the new design](https://developer.apple.com/videos/play/wwdc2025/310/), [Build a SwiftUI app with the new design](https://developer.apple.com/videos/play/wwdc2025/323/), [Adopting Liquid Glass](https://developer.apple.com/documentation/technologyoverviews/adopting-liquid-glass) | Keep Inspector rows and the Tag field legible on the pane; avoid adding a second glass pane behind each field. Use standard control sizing and spacing rather than fixed heights. |
| Reading and media content | Apple positions Liquid Glass as a functional layer for navigation and controls, distinct from the content layer, and says not to apply it throughout content. Standard materials can organize content underneath. [Materials](https://developer.apple.com/design/human-interface-guidelines/materials), [Layout](https://developer.apple.com/design/human-interface-guidelines/layout) | Keep article text, photos, and displayed metadata visually primary. Do not make content labels look like floating controls. |

Óia currently presents its leading Inspector as the first `NavigationSplitView` column, rather than SwiftUI's `.inspector` attachment, in [OiaReadingOverlay.swift](../../macos/Sources/Oia/Features/Oia/OiaReadingOverlay.swift). Apple's automatic edge-to-edge *inspector* appearance is therefore a design reference, not a verified description of Óia's rendered pane. Apple's [SwiftUI inspector example](https://developer.apple.com/videos/play/wwdc2025/323/) uses `.inspector(isPresented:)`.

## Control hierarchy in the Inspector

Apple recommends only one or two prominent buttons per view and uses prominence to mark the most likely action. A destructive role should communicate destruction without becoming the primary action. For Óia, the Tag field needs a normal field border and focus state; **Delete** remains a separate destructive action, and routine Tag chips need no primary tint. These are design inferences from [Buttons](https://developer.apple.com/design/human-interface-guidelines/buttons), [Liquid Glass color](https://developer.apple.com/design/human-interface-guidelines/color), and [Build an AppKit app with the new design](https://developer.apple.com/videos/play/wwdc2025/310/).

The current source already uses a rounded native `TextField` and `textInputSuggestions` for [Tag entry](../../macos/Sources/Oia/Features/Tags/InspectorTagInput.swift). [Tag pills](../../macos/Sources/Oia/Features/Oia/InspectorGlass.swift) receive individual custom glass effects. Apple advises limiting custom glass to important top-level controls and avoiding crowded or overlapping glass elements, so the pills are worth reviewing in the full Inspector rather than treating their finish as a reason to glass the input too. This is a review recommendation, not a measured usability result. [Materials](https://developer.apple.com/design/human-interface-guidelines/materials), [Adopting Liquid Glass](https://developer.apple.com/documentation/technologyoverviews/adopting-liquid-glass)

## Visual references

These screenshots were inspected from their publishers' own documentation in the browser. They are references for hierarchy and control styling, not specifications for copying a complete interface.

### Apple Photos: compact metadata and inline keywords

The [Photos Info window](https://support.apple.com/en-ie/guide/photos/phta8b25fa42/mac) combines editable title, caption and keyword rows with compact metadata on one quiet surface. Its screenshot shows subtle separators and an understated **Add a Keyword** row. Apple's [keyword guide](https://support.apple.com/en-ie/guide/photos/pht8d0ad5198/mac) confirms prior-keyword suggestions and Return to add a keyword. Borrow the separation between editable fields and read-only facts; Óia can retain a clearer rounded field boundary for discoverability.

![Apple Photos Info window](https://ipcdn-web.apple.com/assets/v2/web/11c23372-dd0a-4694-92f0-8579c4dd989d)

### Ulysses: understated headings and keyword entry

The [Mac Dashboard](https://help.ulysses.app/the-dashboard/dashboard) puts keywords, creation dates and other document facts in a narrow inspector. The September 2026 screenshot shows muted section labels, aligned date rows and a lightweight **Add keyword** field. Borrow its restrained typography and section spacing; show Óia's user/machine distinction through its existing icons.

![Ulysses Mac Dashboard](https://static.helpjuice.com/helpjuice_production/uploads/upload/image/16213/direct/1789107601469/Screenshot%202026-09-11%20at%2008.19.45.png)

### Sketch: consistent fields and section hierarchy

The [Sketch Inspector](https://www.sketch.com/docs/interface-and-settings/the-mac-app-interface/the-inspector/) documentation, updated April 2026, shows compact rounded fields, small secondary icons and clear sections. Borrow its consistency between field geometry, type size and alignment. Óia needs fewer controls, so it can preserve more breathing room. The documentation image uses a signed CDN URL; use the page to retrieve the current image rather than depending on an expiring link.

## Proposed direction for Óia

This is a proposal; the application has not been restyled as part of this research.

| Element | Proposed treatment |
| --- | --- |
| Tag input | Native rounded text field with system focus styling and suggestions. |
| Tags | Quiet tokens with a subtle neutral fill and no individual glass highlights; retain `tag`, `sparkles`, and the hover remove icon. |
| Section labels | One consistent small semibold style for Tags, Colours and Details. Current source uses different sizes for Details and the other headings. |
| Metadata | Secondary labels, regular-weight values, and separators between groups instead of every row. |
| Secondary actions | Standard compact controls or plain icon/text actions; keep Delete visually secondary and retain its card-deletion confirmation. |
| Pane and toolbar | Let their structural system material carry the depth. Keep the existing board and toolbar direction. |
