# Collapsed macOS toolbar search

Research updated 2026-09-27 against Apple's documentation and the installed Xcode 27 macOS SDK.

## Supported behavior

Óia uses SwiftUI's `.searchable` with tokens and suggestions. On macOS this creates a trailing [`NSSearchToolbarItem`](https://developer.apple.com/documentation/appkit/nssearchtoolbaritem). AppKit expands it on focus and may compress it to a magnifying-glass button **when toolbar space is low**. AppKit owns the transition and the toolbar allocation. The [toolbar placement documentation](https://developer.apple.com/documentation/swiftui/searchfieldplacement/toolbar) confirms that this is the trailing macOS toolbar item.

SwiftUI's tokenized [`.searchable(text:tokens:isPresented:placement:prompt:token:)`](https://developer.apple.com/documentation/swiftui/view/searchable%28text%3Atokens%3Aispresented%3Aplacement%3Aprompt%3Atoken%3A%29) is available on macOS 14 and later, according to the installed SwiftUI interface. Apple says [`isPresented`](https://developer.apple.com/documentation/swiftui/managing-search-interface-activation) activates and dismisses search on macOS, including focus. It does not request a compact resting representation.

[`SearchToolbarBehavior.minimize`](https://developer.apple.com/documentation/swiftui/searchtoolbarbehavior/minimize) would request a button-like resting control, but the installed macOS SDK explicitly marks that member unavailable on native macOS. Only `.automatic` is available there. `preferredWidthForSearchField` controls the width **when focused**, not the compact resting state. The older `NSToolbarItem.minSize` and `maxSize` properties are deprecated; Apple recommends automatic measurement through view constraints instead.

## Why the previous workaround failed

The former `CompactSearchToolbarConfiguration` added a square-width constraint to the search field *after* SwiftUI created the toolbar item, then changed its priority during focus changes. That could make the idle field look compact, but it did not give the toolbar item a consistent allocation during AppKit's outgoing animation. The full-width presentation moved beyond the window's trailing edge before shrinking. Suppressing implicit animations and forcing layout inside the focus transaction did not change that behavior. A later constraint between the search field and a view outside its hierarchy caused a launch-time Auto Layout abort and was reverted.

Apple's [`NSSearchToolbarItem.searchField`](https://developer.apple.com/documentation/appkit/nssearchtoolbaritem/searchfield) documentation says to customize a replacement search field **before assigning it to the item**. It does not document changing the generated field's sizing constraint after SwiftUI installs it. The earlier claim that this was a reliable native compact mode was incorrect.

## Decision

Use one persistent native `.searchable` item and its public `isPresented` and `searchFocused` bindings for ⌘F and `/`. Dismiss presentation when search is empty and no longer focused. Remove the field constraint, mouse monitor, and direct begin/end interaction calls. AppKit then decides when available toolbar space calls for the compact representation and keeps responsibility for both directions of its animation. At wide window sizes the native field may remain visible at rest; macOS provides no supported always-compact switch for this control. Óia keeps native token editing and suggestions, as required by [DESIGN.md](../../DESIGN.md).
