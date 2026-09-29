// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// A source-aware article presentation. The reading remains an article for
/// filtering, search, and its durable Markdown body; this view only changes how
/// the indexed source profile is presented.
struct SocialPostCard: View {
    let row: ReadingRow
    let profile: ReadingSourceProfile
    let libraryURL: URL?
    let cardSize: CGSize
    let displayScale: CGFloat
    let isVisible: Bool
    let scrollState: BoardScrollState
    let playbackPositions: VideoPlaybackPositionStore
    let autoplayEnabled: Bool
    let reduceMotion: Bool
    let scenePhase: ScenePhase

    var body: some View {
        VStack(alignment: .leading, spacing: OiaCardTextMetrics.socialPostSpacing) {
            SocialPostHeader(
                row: row,
                profile: profile,
                libraryURL: libraryURL,
                isVisible: isVisible
            )
            .fixedSize(horizontal: false, vertical: true)

            Text(row.socialPostText)
                .font(Font(OiaCardTextMetrics.socialPostFont))
                .lineSpacing(OiaCardTextMetrics.socialPostLineSpacing)
                .lineLimit(OiaCardTextMetrics.socialPostLineLimit)
                .fixedSize(horizontal: false, vertical: true)

            if let attachment = profile.primaryAttachment {
                SocialPostAttachmentPreview(
                    row: row,
                    attachment: attachment,
                    libraryURL: libraryURL,
                    size: attachmentSize(for: attachment),
                    maxPixel: AssetPreviewLoadPlan.displayMaxPixel(
                        for: attachmentSize(for: attachment),
                        displayScale: displayScale
                    ),
                    isVisible: isVisible,
                    scrollState: scrollState,
                    playbackPositions: playbackPositions,
                    autoplayEnabled: autoplayEnabled,
                    reduceMotion: reduceMotion,
                    scenePhase: scenePhase
                )
            }
        }
        .padding(OiaCardTextMetrics.socialPostPadding)
        .frame(
            width: cardSize.width,
            height: cardSize.height,
            alignment: .topLeading
        )
        .clipped()
    }

    private func attachmentSize(for attachment: ReadingSourceAttachment) -> CGSize {
        let width = max(1, cardSize.width - OiaCardTextMetrics.socialPostPadding * 2)
        return CGSize(width: width, height: width / attachment.cardAspectRatio)
    }
}

struct SocialPostDetailView: View {
    @Environment(AppState.self) private var appState

    let row: ReadingRow
    let profile: ReadingSourceProfile

    @State private var bodyText: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                SocialPostHeader(
                    row: row,
                    profile: profile,
                    libraryURL: appState.libraryURL,
                    isVisible: true,
                    avatarSize: 44
                )

                Text(attributedPost)
                    .font(.title3)
                    .lineSpacing(5)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)

                ForEach(Array(profile.attachments.enumerated()), id: \.offset) { _, attachment in
                    attachmentDetail(attachment)
                }

                if let publishedAt = profile.publishedAt {
                    Text(publishedAt)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            }
            .frame(maxWidth: 680, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .center)
            .padding(.horizontal, 50)
            .padding(.vertical, 64)
        }
        .background(Color(nsColor: .textBackgroundColor))
        .task(id: contentLoadID) {
            bodyText = nil
            let generation = appState.libraryContentGeneration
            let body = await appState.getBody(id: row.id)
            guard !Task.isCancelled,
                  generation == appState.libraryContentGeneration
            else {
                return
            }
            bodyText = body
        }
    }

    @ViewBuilder
    private func attachmentDetail(_ attachment: ReadingSourceAttachment) -> some View {
        switch attachment.mediaKind {
        case .image:
            LocalReadingImage(
                row: row,
                libraryURL: appState.libraryURL,
                explicitAssetReference: attachment.asset,
                fallbackAspectRatio: attachment.intrinsicAspectRatio,
                maxPixel: 2400,
                contentMode: .fit
            )
            .aspectRatio(attachment.intrinsicAspectRatio, contentMode: .fit)
            .frame(maxWidth: .infinity)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(attachment.alt ?? "Attached image")
            .modifier(SocialAttachmentShape())

        case .video:
            LocalReadingVideo(
                row: row,
                libraryURL: appState.libraryURL,
                assetReference: attachment.asset,
                accessibilityName: attachment.alt,
                autoplay: false
            )
            .aspectRatio(attachment.intrinsicAspectRatio, contentMode: .fit)
            .frame(maxWidth: .infinity)
            .modifier(SocialAttachmentShape())

        case nil:
            EmptyView()
        }
    }

    private var contentLoadID: String {
        "\(row.id):\(appState.libraryContentGeneration)"
    }

    private var displayText: String {
        if let body = bodyText?.trimmingCharacters(in: .whitespacesAndNewlines),
           !body.isEmpty
        {
            return body
                .components(separatedBy: "\n\n<!-- oia:attachments -->")
                .first?
                .trimmingCharacters(in: .whitespacesAndNewlines) ?? body
        }
        return row.socialPostText
    }

    private var attributedPost: AttributedString {
        let options = AttributedString.MarkdownParsingOptions(
            interpretedSyntax: .inlineOnlyPreservingWhitespace
        )
        return (try? AttributedString(markdown: displayText, options: options))
            ?? AttributedString(displayText)
    }
}

private struct SocialPostHeader: View {
    let row: ReadingRow
    let profile: ReadingSourceProfile
    let libraryURL: URL?
    let isVisible: Bool
    var avatarSize: CGFloat = 34

    var body: some View {
        HStack(spacing: 10) {
            if let avatarAsset = profile.avatarAsset {
                LocalReadingImage(
                    row: row,
                    libraryURL: libraryURL,
                    explicitAssetReference: avatarAsset,
                    fallbackAspectRatio: 1,
                    maxPixel: 160,
                    contentMode: .fill,
                    isVisible: isVisible
                )
                .frame(width: avatarSize, height: avatarSize)
                .compositingGroup()
                .clipShape(Circle())
                .accessibilityHidden(true)
            }

            VStack(alignment: .leading, spacing: 1) {
                Text(row.socialPostAuthor)
                    .font(.callout.weight(.semibold))
                    .lineLimit(1)

                Text(profile.displayHandle ?? row.displaySite ?? "Social post")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer(minLength: 6)

            Text(profile.displayProvider)
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .padding(.horizontal, 7)
                .padding(.vertical, 4)
                .background(.secondary.opacity(0.09), in: Capsule())
        }
    }
}

private struct SocialPostAttachmentPreview: View {
    let row: ReadingRow
    let attachment: ReadingSourceAttachment
    let libraryURL: URL?
    let size: CGSize
    let maxPixel: CGFloat
    let isVisible: Bool
    let scrollState: BoardScrollState
    let playbackPositions: VideoPlaybackPositionStore
    let autoplayEnabled: Bool
    let reduceMotion: Bool
    let scenePhase: ScenePhase

    @ViewBuilder
    var body: some View {
        Group {
            if attachment.mediaKind == .video {
                AutoplayVideoCard(
                    row: row,
                    libraryURL: libraryURL,
                    cardSize: size,
                    playbackPositions: playbackPositions,
                    videoAssetReference: attachment.asset,
                    previewAssetReference: attachment.previewAsset,
                    previewAssetIsVideo: attachment.previewIsVideo,
                    aspectRatio: attachment.cardAspectRatio,
                    contentMode: .fill,
                    maxPixel: maxPixel,
                    isInViewport: isVisible,
                    scrollState: scrollState,
                    autoplayEnabled: autoplayEnabled,
                    reduceMotion: reduceMotion,
                    scenePhase: scenePhase
                )
            } else {
                LocalReadingImage(
                    row: row,
                    libraryURL: libraryURL,
                    explicitAssetReference: attachment.previewAsset,
                    fallbackAspectRatio: attachment.cardAspectRatio,
                    maxPixel: maxPixel,
                    contentMode: .fill,
                    loadsProgressively: true,
                    isVisible: isVisible,
                    scrollState: scrollState
                )
                .frame(width: size.width, height: size.height)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(attachment.alt ?? accessibilityFallback)
        .modifier(SocialAttachmentShape())
    }

    private var accessibilityFallback: String {
        attachment.mediaKind == .video ? "Attached video" : "Attached image"
    }
}

private struct SocialAttachmentShape: ViewModifier {
    func body(content: Content) -> some View {
        content
            .compositingGroup()
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .stroke(Color.primary.opacity(0.1), lineWidth: 1)
            }
    }
}
