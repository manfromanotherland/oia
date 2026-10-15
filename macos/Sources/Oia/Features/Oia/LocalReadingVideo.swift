// SPDX-License-Identifier: GPL-3.0-or-later

import AVFoundation
import AVKit
import SwiftUI

/// Offline playback for a reading's local movie. Standalone video rows resolve
/// their `cuttings-asset:assets/<content-hash>.<ext>` identity; source-aware
/// articles pass an explicit attachment path. Both use the existing
/// single-file asset-path rules, so playback cannot leave the reading folder.
struct LocalReadingVideo: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    let row: ReadingRow
    let libraryURL: URL?
    var assetReference: String?
    var accessibilityName: String?
    var autoplay = true

    @State private var player: AVPlayer?
    @State private var failed = false

    var body: some View {
        ZStack {
            Color(red: 0.08, green: 0.09, blue: 0.10)

            if let player {
                VideoPlayer(player: player)
                    .accessibilityLabel("Video: \(accessibilityName ?? row.displayTitle)")
                    .accessibilityIdentifier(A11y.Detail.videoPlayer)
            } else if failed {
                ContentUnavailableView(
                    "Video unavailable",
                    systemImage: "play.slash",
                    description: Text("The saved video file could not be played.")
                )
                .foregroundStyle(.white)
                .accessibilityIdentifier(A11y.Detail.videoUnavailable)
            }
        }
        .task(id: loadKey) {
            await load()
        }
        .onDisappear {
            releasePlayer()
        }
    }

    private var loadKey: String {
        "\(row.id):\(resolvedAssetReference ?? "")"
    }

    @MainActor
    private func load() async {
        releasePlayer()
        failed = false

        guard let url = localVideoURL,
              (try? url.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true
        else {
            failed = true
            return
        }

        let asset = AVURLAsset(url: url)
        do {
            guard try await asset.load(.isPlayable),
                  try await !(asset.loadTracks(withMediaType: .video)).isEmpty,
                  !Task.isCancelled
            else {
                failed = true
                return
            }
            let loadedPlayer = AVPlayer(playerItem: AVPlayerItem(asset: asset))
            player = loadedPlayer
            if autoplay, !reduceMotion {
                loadedPlayer.play()
            }
        } catch {
            guard !Task.isCancelled else { return }
            failed = true
        }
    }

    @MainActor
    private func releasePlayer() {
        player?.pause()
        player?.replaceCurrentItem(with: nil)
        player = nil
    }

    private var localVideoURL: URL? {
        guard let reference = resolvedAssetReference else { return nil }
        let baseURL = AssetImageLoader.readingFolderURL(
            libraryURL: libraryURL, readingID: row.id
        )
        return AssetImageLoader.localURL(source: reference, assetBaseURL: baseURL)
    }

    private var resolvedAssetReference: String? {
        assetReference ?? row.localVideoAssetReference
    }
}
