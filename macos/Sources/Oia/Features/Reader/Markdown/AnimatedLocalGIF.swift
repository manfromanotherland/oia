// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

/// Plays a saved GIF from disk. The still preview remains visible while AppKit
/// loads the original representation, which retains frame timing and looping.
struct AnimatedLocalGIF: View {
    let url: URL
    let fallback: NSImage
    let contentMode: ContentMode

    @State private var animatedImage: NSImage?

    private struct LoadedImage: @unchecked Sendable {
        let value: NSImage?
    }

    var body: some View {
        Group {
            if let animatedImage {
                AnimatedGIFImageView(image: animatedImage, contentMode: contentMode)
            } else {
                Image(nsImage: fallback)
                    .resizable()
                    .interpolation(.high)
                    .aspectRatio(contentMode: contentMode)
            }
        }
        .task(id: url) {
            animatedImage = await Task.detached(priority: .utility) {
                LoadedImage(value: NSImage(contentsOf: url))
            }.value.value
        }
        .onDisappear { animatedImage = nil }
    }
}

private struct AnimatedGIFImageView: NSViewRepresentable {
    let image: NSImage
    let contentMode: ContentMode

    func makeNSView(context: Context) -> NSImageView {
        let view = NSImageView()
        view.animates = true
        view.imageAlignment = .alignCenter
        view.imageScaling = contentMode == .fill ? .scaleProportionallyUpOrDown : .scaleProportionallyDown
        return view
    }

    func updateNSView(_ view: NSImageView, context: Context) {
        if view.image !== image { view.image = image }
        view.imageScaling = contentMode == .fill ? .scaleProportionallyUpOrDown : .scaleProportionallyDown
    }
}

extension URL {
    var isLocalGIF: Bool {
        isFileURL && pathExtension.caseInsensitiveCompare("gif") == .orderedSame
    }
}
