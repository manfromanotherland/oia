# macOS inspector design: Tag input and Liquid Glass

Research date: 2026-10-02. Sources are current Apple Human Interface Guidelines, Apple developer documentation, and WWDC25 sessions. Recommendations below are design inferences from those sources, not Apple rules for Óia specifically.

Status: research only. The user considers Photos and Sketch acceptable but wants a more distinctive aesthetic with exceptional finish. Things establishes a quality bar but is not the desired style. No visual direction has been selected or implemented.

## Native control considerations

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

### Pixelmator Pro: cohesive translucent panels

Apple's [Pixelmator Pro Mac interface guide](https://support.apple.com/en-ca/guide/pixelmator-pro/pix96e754af4/mac) shows a translucent right tool pane with grouped controls, compact typography and restrained control backgrounds. The pane changes to show settings for the selected tool. Borrow the coherent panel material and the quieter surfaces inside it; Óia has fewer controls and can keep its existing section structure. This is the strongest reference here for the overall panel finish.

![Pixelmator Pro Mac interface](https://ipcdn-web.apple.com/assets/v2/web/2b3dd883-4db8-4afb-85db-9c8da475b4fc)

### Sketch: consistent fields and section hierarchy

The [Sketch Inspector](https://www.sketch.com/docs/interface-and-settings/the-mac-app-interface/the-inspector/) documentation, updated April 2026, shows compact rounded fields, small secondary icons and clear sections. Borrow its consistency between field geometry, type size and alignment. Óia needs fewer controls, so it can preserve more breathing room. The documentation image uses a signed CDN URL; use the page to retrieve the current image rather than depending on an expiring link.

## Conventional implementation proposal

This is a proposal; the application has not been restyled as part of this research.

| Element | Proposed treatment |
| --- | --- |
| Tag input | Native rounded text field with system focus styling and suggestions. |
| Tags | Quiet tokens with a subtle neutral fill and no individual glass highlights; retain `tag`, `sparkles`, and the hover remove icon. |
| Section labels | One consistent small semibold style for Tags, Colours and Details. Current source uses different sizes for Details and the other headings. |
| Metadata | Secondary labels, regular-weight values, and separators between groups instead of every row. |
| Secondary actions | Standard compact controls or plain icon/text actions; keep Delete visually secondary and retain its card-deletion confirmation. |
| Pane and toolbar | Let their structural system material carry the depth. Keep the existing board and toolbar direction. |

The baseline above establishes control hierarchy. It does not settle Óia's visual identity.

## More distinctive aesthetic references

### Screen Studio: matte, spacious editing controls

Its [background settings guide](https://screen.studio/guide/background) includes a close-up of a dark settings panel: generous spacing, consistent symbols, understated section labels, low-contrast controls and one focused accent. The panel reads as one coherent surface. This is a reference for a modern matte Inspector, independent of the purple marketing background surrounding the panel.

![Screen Studio background controls](https://cdn.sanity.io/images/2ltoq22u/production/87d27ef535d8133201ac161b2e6366d663f7cb9a-2560x1920.png?w=1600&q=80&fit=max&auto=format)

### Raycast: precise typography and a unified input family

Raycast's [2026 redesign](https://www.raycast.com/blog/the-new-raycast) describes a refreshed macOS interface with selective Liquid Glass. Its [TagPicker documentation](https://developers.raycast.com/api-reference/user-interface/form#formtagpicker) shows tokens, an input and suggestions with coordinated geometry and surfaces. The TagPicker screenshot was inspected separately; its exact app version is unspecified. Borrow the relationship between the field, tokens and menu, while preserving Óia's current Tag behavior and provenance symbols.

![Raycast app](https://www.raycast.com/uploads/blog-new-raycast/app.png)

### Craft: soft surfaces and an integrated content frame

Craft's [September 2026 Mac update](https://www.craft.do/blog/craft-update-3-6-5) shows sidebars integrated into the window surface with an inset document, restrained navigation and coordinated corner radii. It offers a softer, more atmospheric direction. The product screenshot combines Mac and iPad; it is an overall surface reference, not a close-up of Mac Tag entry.

![Craft Mac and iPad surfaces](https://www.craft.do/images/content/blog/craft-update-3-6-5/image-2.jpg)

These are alternative visual directions to compare. The choice should follow the user's taste rather than treating native platform conventions as the aesthetic target.
