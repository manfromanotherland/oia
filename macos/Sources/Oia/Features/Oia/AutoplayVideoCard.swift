// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import AVFoundation
import SwiftUI

/// A video card that keeps its local poster in the layout while playing muted
/// video above it. The player is prepared on first entry, released offscreen,
/// and recreated at its saved playback position when the card returns.
struct AutoplayVideoCard: View {
    @Environment(\.assetContentGeneration) private var contentGeneration
    let row: ReadingRow
    let libraryURL: URL?
    let cardSize: CGSize
    let playbackPositions: VideoPlaybackPositionStore
    var maxPixel: CGFloat = 800
    var isInViewport = false
    let scrollState: BoardScrollState
    var autoplayEnabled = true
    var reduceMotion = false
    var scenePhase: ScenePhase = .active

    @State private var loadedMediaKey: String?
    @State private var playback: CardVideoPlayback?
    @State private var loadedSourceFingerprint: AssetPreviewSourceFingerprint?
    @State private var validatedMediaGeneration: UInt64?

    var body: some View {
        LocalReadingImage(
            row: row,
            libraryURL: libraryURL,
            fallbackAspectRatio: row.standaloneMediaAspectRatio ?? 16 / 9,
            maxPixel: maxPixel,
            contentMode: .fit,
            loadsProgressively: true,
            isVisible: isInViewport,
            scrollState: scrollState
        )
        .frame(width: cardSize.width, height: cardSize.height)
        .clipped()
        .overlay {
            if let playback {
                CardVideoPlayerLayer(player: playback.player)
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
            }
        }
        .clipped()
        .task(id: playbackTaskID) {
            await synchronizePlayback()
        }
        .onDisappear {
            pausePlayback()
            releasePlayback()
        }
    }

    private var shouldAutoplay: Bool {
        autoplayEnabled
            && isInViewport
            && !reduceMotion
            && scenePhase == .active
    }

    private var playbackTaskID: String {
        "\(mediaKey):autoplay=\(shouldAutoplay):generation=\(contentGeneration)"
    }

    private var mediaKey: String {
        "\(libraryURL?.path ?? ""):\(row.id):\(row.mediaUrl ?? "")"
    }

    @MainActor
    private func synchronizePlayback() async {
        let requestedMediaKey = mediaKey
        if loadedMediaKey != requestedMediaKey {
            pausePlayback(mediaKey: loadedMediaKey)
            releasePlayback()
            loadedMediaKey = requestedMediaKey
        }

        guard shouldAutoplay else {
            pausePlayback()
            releasePlayback()
            return
        }
        guard await revalidatePlayback(requestedMediaKey: requestedMediaKey) else { return }
        if let playback, playback.player.rate > 0 {
            return
        }

        do {
            try await Task.sleep(for: .milliseconds(220))
        } catch {
            return
        }
        guard !Task.isCancelled, shouldAutoplay, loadedMediaKey == requestedMediaKey else { return }

        if let playback {
            await resumePlayback(playback)
            return
        }

        await loadAndPlay(requestedMediaKey: requestedMediaKey)
    }

    @MainActor
    private func resumePlayback(_ playback: CardVideoPlayback) async {
        guard await playbackPositions.scheduler.waitForStartTurn(),
              !Task.isCancelled, shouldAutoplay,
              self.playback === playback else { return }
        playback.player.play()
    }

    @MainActor
    private func loadAndPlay(requestedMediaKey: String) async {
        guard let url = playbackURL,
              let lease = await playbackPositions.scheduler.acquire() else { return }
        var leaseTransferred = false
        defer {
            if !leaseTransferred {
                playbackPositions.scheduler.release(lease)
            }
        }
        guard await playbackPositions.scheduler.waitForStartTurn(), !Task.isCancelled else { return }
        guard let prepared = await playableAsset(at: url),
              !Task.isCancelled, loadedMediaKey == requestedMediaKey, shouldAutoplay else { return }
        let loadedPlayback = CardVideoPlayback(
            item: AVPlayerItem(asset: prepared.asset),
            scheduler: playbackPositions.scheduler,
            lease: lease
        )
        playback = loadedPlayback
        loadedSourceFingerprint = prepared.fingerprint
        validatedMediaGeneration = contentGeneration
        leaseTransferred = true
        restorePosition(of: loadedPlayback.player, mediaKey: requestedMediaKey)
        loadedPlayback.player.play()
    }

    @MainActor
    private func playableAsset(at url: URL) async -> PreparedVideoAsset? {
        let fingerprint = await Task.detached(priority: .utility) {
            AssetPreviewSourceFingerprint.read(at: url)
        }.value
        guard let fingerprint, !Task.isCancelled else { return nil }

        let asset = AVURLAsset(url: url)
        do {
            guard try await asset.load(.isPlayable),
                  try await !(asset.loadTracks(withMediaType: .video)).isEmpty,
                  !Task.isCancelled
            else { return nil }
            let current = await Task.detached(priority: .utility) {
                AssetPreviewSourceFingerprint.read(at: url)
            }.value
            guard current == fingerprint, !Task.isCancelled else { return nil }
            return PreparedVideoAsset(asset: asset, fingerprint: fingerprint)
        } catch {
            // The saved poster remains the card's offline/failure presentation.
            return nil
        }
    }

    @MainActor
    private func revalidatePlayback(requestedMediaKey: String) async -> Bool {
        guard playback != nil, validatedMediaGeneration != contentGeneration else { return true }
        let generation = contentGeneration
        let url = playbackURL
        let fingerprint = await Task.detached(priority: .utility) {
            url.flatMap { AssetPreviewSourceFingerprint.read(at: $0) }
        }.value
        guard !Task.isCancelled, loadedMediaKey == requestedMediaKey,
              contentGeneration == generation else { return false }
        if fingerprint != loadedSourceFingerprint {
            pausePlayback()
            releasePlayback()
        }
        validatedMediaGeneration = generation
        return true
    }

    private struct PreparedVideoAsset {
        let asset: AVURLAsset
        let fingerprint: AssetPreviewSourceFingerprint
    }

    @MainActor
    private func restorePosition(of player: AVPlayer, mediaKey: String) {
        guard let position = playbackPositions.position(for: mediaKey) else { return }
        player.seek(to: position, toleranceBefore: .zero, toleranceAfter: .zero)
    }

    @MainActor
    private func pausePlayback(mediaKey: String? = nil) {
        guard let playback else { return }
        playback.player.pause()
        guard let mediaKey = mediaKey ?? loadedMediaKey else { return }
        playbackPositions.save(playback.player.currentTime(), for: mediaKey)
    }

    @MainActor
    private func releasePlayback() {
        playback?.stop()
        playback = nil
        loadedSourceFingerprint = nil
        validatedMediaGeneration = nil
    }

    private var playbackURL: URL? {
        guard let reference = row.localVideoAssetReference else { return nil }
        let baseURL = AssetImageLoader.readingFolderURL(
            libraryURL: libraryURL,
            readingID: row.id
        )
        return AssetImageLoader.localURL(source: reference, assetBaseURL: baseURL)
    }
}

@MainActor
final class VideoPlaybackPositionStore {
    let scheduler = VideoPlaybackScheduler()
    private var positions: [String: CMTime] = [:]

    func position(for mediaKey: String) -> CMTime? {
        positions[mediaKey]
    }

    func save(_ position: CMTime, for mediaKey: String) {
        let seconds = position.seconds
        guard seconds.isFinite, seconds > 0 else { return }
        positions[mediaKey] = position
    }
}

@MainActor
private final class CardVideoPlayback {
    let player: AVQueuePlayer
    private let looper: AVPlayerLooper
    private let scheduler: VideoPlaybackScheduler
    private var lease: UUID?

    init(item: AVPlayerItem, scheduler: VideoPlaybackScheduler, lease: UUID) {
        self.scheduler = scheduler
        self.lease = lease
        let player = AVQueuePlayer()
        player.isMuted = true
        player.preventsDisplaySleepDuringVideoPlayback = false
        self.player = player
        looper = AVPlayerLooper(player: player, templateItem: item)
    }

    func stop() {
        player.pause()
        player.removeAllItems()
        if let lease {
            scheduler.release(lease)
            self.lease = nil
        }
    }
}

private struct CardVideoPlayerLayer: NSViewRepresentable {
    let player: AVPlayer

    func makeNSView(context _: Context) -> PlayerView {
        let view = PlayerView()
        view.playerLayer.player = player
        return view
    }

    func updateNSView(_ view: PlayerView, context _: Context) {
        if view.playerLayer.player !== player {
            view.playerLayer.player = player
        }
    }

    static func dismantleNSView(_ view: PlayerView, coordinator _: Void) {
        view.playerLayer.player = nil
    }

    final class PlayerView: NSView {
        var playerLayer: AVPlayerLayer {
            // `makeBackingLayer()` is the sole layer factory for this view.
            layer as! AVPlayerLayer // swiftlint:disable:this force_cast
        }

        override func makeBackingLayer() -> CALayer {
            let layer = AVPlayerLayer()
            layer.videoGravity = .resizeAspect
            return layer
        }

        override init(frame frameRect: NSRect) {
            super.init(frame: frameRect)
            wantsLayer = true
        }

        @available(*, unavailable)
        required init?(coder _: NSCoder) {
            fatalError("init(coder:) has not been implemented")
        }
    }
}
