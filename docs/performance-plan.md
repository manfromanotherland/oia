# Óia performance plan

September 24, 2026. Source audit at `8034ce3`.

The primary acceptance criterion is fast, consistently smooth board scrolling, at both slow and fast scroll speeds. The immediate
reproduction is specific: starting a scroll turns every visible image blurry and
frame drops begin. This plan prioritizes that transition, then sustained scrolling,
then other interactions. No app implementation or runtime profiling was performed
for this audit; the costs below are confirmed code paths, not measured frame costs.

## What the previous work established

The referenced **Improve image scroll performance** task landed `77ac55d`. Its
160px initial previews, 120ms idle refinement, 1024px board cap, visibility gating,
separate decode lanes, request coalescing, and exact-size cache are still present.
That task reported 184 passing tests and a Debug build, but explicitly did not
verify 120 Hz frame pacing. Preserve the useful bounds and cancellation behavior.

The current policy has a concrete problem: `LocalReadingImage.swift:59` selects
the lightweight bitmap whenever scrolling starts, including for cards that already
have a display-quality bitmap. Line 30 also changes interpolation. Refinement task
identity changes at line 108. All these depend on the same observable scroll flag.
`AssetPreviewPresentation` retains both variants, so selecting the smaller bitmap
does not release the retained display bitmap. The old test at
`AssetPreviewLoadPlanTests.swift:125` explicitly expects lightweight selection.

This explains the observed blur. The implementation work below removes the confirmed
image swaps and repeated updates. The user judges the resulting scrolling behavior;
code-level measurements do not establish presentation-frame timing.

## 1. Verify performance without taking over the screen

Do not launch, focus, drive, or record the user's app or screen. The user tests
builds himself. Notify him when a new normal Debug build is available, without
opening it. A 120 FPS metric is not a blocker or a substitute for his assessment.

Use source inspection, deterministic code-only regression tests and isolated CPU
benchmarks. Keep tests that create native windows out of automated runs. Prioritize
removing work rather than trading image quality for scroll responsiveness. Preserve
before/after evidence for duplicated work, image identity, decode counts, bounded
queues/caches, metadata-only layout invalidation, main-actor responsiveness and
interactive database reads during background filesystem work.

The optional scroll replay tooling is developer-invoked only; do not run it during
this task. Earlier synthetic replay data does not establish a frame-rate result.

## 2. Remove the scroll-start image downgrade

This is the first implementation checkpoint.

- Keep the best already-presented board image stable for the same asset and size.
  Scroll phase must not change its bitmap, interpolation or geometry.
- Use a 160px preview or existing color placeholder for newly entering uncached
  cards. Defer their display-quality work until scrolling settles.
- Stop reading the scroll flag in the render path. When a display variant already
  satisfies the request, exit refinement planning before reading that flag too.
  Scrolling then changes pending work rather than every resolved image.
- On warm re-entry, use a suitable cached board display variant immediately.
  Do not force another lightweight-to-display cycle. Preserve board/detail size
  separation: a large detail decode must not become a board preview.
- Prevent identical display variants from being republished after every idle
  transition. Preserve cancellation of stale queued requests and asset identity
  checks. Already-running synchronous decodes may finish into cache; they must
  not publish an obsolete result into a recycled card.

Replace the downgrade assertion with regressions for stable image identity across
scroll start/stop, low-cost loading of new cards, a single idle upgrade, warm re-entry,
asset replacement, offscreen release and rapid reversals. Use the real load-plan
and presentation boundary; these tests protect presentation and scheduling behavior.

Pass gate: zero image replacements or new decode requests caused solely by starting
to scroll a fully refined viewport. Verify this in the presentation/scheduling regressions and ask for feedback on the available build; continue removing the remaining costs below.

## 3. Make scrolling update only the cards that change

LazyLayoutKit 0.3.0 currently writes viewport state and creates/assigns a new
`placed` array on every geometry callback, even if membership is unchanged:
`SwiftUI/LazyLayoutView.swift:423` and `:659`. This is confirmed allocation/state
work; the redundant state publications are covered by a regression test.

- Keep continuously changing raw scroll geometry outside broad observable view
  state. Publish the materialized window only when its membership, content
  revision or geometry revision changes.
- Retain a bounded overscan region and recompute membership when its boundary is
  crossed. Preserve anchoring, accessibility, selection and programmatic scrolling.
- Use the board's existing frames to drive one visibility/prefetch coordinator.
  Today each media card also installs `CardViewportVisibilityModifier`
  (`AssetPreviewPipeline.swift:31`). Send per-card visibility transitions only;
  retain stricter actual-viewport gating for playback.
- Measure window-query time, cell creation rate and retained cells separately.
  `.items(80)` targets roughly 80 total materialized cells, not 80 extra cells.
  Blindly reducing that number can increase construction churn.

Pass gate: zero placed-array publications while scrolling within an unchanged
materialization window; work scales with entering/leaving cards, not total library
size. Compare update counts and main-actor work on the same corpus.
Any dependency change must be a reproducible pinned patch/fork or local package,
never an edit left inside `build/SourcePackages/checkouts`.

## 4. Make cold media cheap without causing idle bursts

The board currently uses an in-memory cache (`AssetPreviewPipeline.swift:172`)
configured for 256 entries and a 128MiB cost target. That is not a total-process
memory limit. A miss still opens the original: raster downsampling, SVG parsing and
rasterization, or AVFoundation frame extraction for posterless video.

- Add disposable per-device derived thumbnails and video posters, outside the
  synced library. Key by validated source identity/fingerprint, a small set of
  pixel tiers and renderer version. Handle missing, corrupt and changed sources.
- Generate them in bounded background jobs; the board must not wait for a complete
  library backfill. While a preview is missing, keep the existing placeholder and
  queue only work relevant to the viewport or a small directional prefetch region.
- Separate board and detail cache budgets. Bound disk space, retained decoded
  bytes and pending jobs. Prefer measured size tiers over nearly identical entries
  for every pixel width during resizing.
- Centralize priority: visible initial previews, near-viewport previews, idle
  refinement, then unrelated maintenance. Cancel obsolete prefetch after reversals.
  Pace image publication as well as decoding; serial decode lanes alone do not
  prevent many cached images from being published in the same frame.

Spotlight's thumbnail cache is a useful existing implementation reference; board
rendering must remain independent of Spotlight availability or indexing progress.
Core retains ownership of source identity and invalidation rules; platform image
rendering remains a disposable presentation concern.

Pass gate: no original-media reads on the warm scroll path, bounded cold work,
no burst of replacements at idle, and stable memory under repeated reversals.
Source inspection confirms original-media decoding on cache misses, so this phase
is included in the implementation.

## 5. Remove layout, video and background interference where measured

These are additional concrete costs addressed from the code audit.

| Area | Current implementation | Planned correction and gate |
| --- | --- | --- |
| Video phase transitions | `AutoplayVideoCard.swift:39` removes the player view at scroll start; visible players start/resume immediately at idle. | Pause with stable view/layer identity; budget and stagger playback startup after settling. Preserve playback position, use pure admission/cancellation tests, and release offscreen players. |
| Duplicate geometry | `LazyMasonryBoard.swift:219` computes all frames for navigation from the view body, then LazyLayoutKit solves them again. | Share one immutable geometry snapshot between rendering, visibility and navigation. One solve per geometry revision; none for scroll offsets, selection, or content edits that do not change geometry/membership. |
| Text/resize work | `OiaCardTextMetrics.swift:17` clears all heights on width changes; measurement is main-actor isolated at line 34. | Prepare immutable metrics with safe font/thread ownership, cancel stale resize requests, and publish one current snapshot. Keep measured and rendered typography identical; do not move existing AppKit work blindly to a detached task. |
| Card compositing | `OiaCardView.swift:72` layers material, clipping and overlays; hover menus remain constructed at zero opacity. | Use diagnostic material/overlay toggles with identical geometry to isolate GPU/render cost. Optimize the responsible layer only if measured; preserve the intended appearance for the user's review. |
| Board refresh | `Readings.swift:151` publishes the entire result; `ReadingQuery.swift:39` requests all rows. | Separate stable ordered IDs, per-row content revisions and geometry revisions. Apply changed-row updates without repeated all-library work. Mapping is already off-main; naive pagination is not the first fix. |
| Filesystem/indexing | `FolderWatcher.swift:60` discards changed paths; routine `CoreBridge.sync()` shares an actor with interactive reads. Full scanning reads Markdown and hashes preview assets. | Reconcile changed readings with full-scan recovery for startup/overflow. Keep filesystem authority, external-writer safety and short database critical sections. Measure queue wait, scanned bytes and lock duration. |
| Optional analysis | Visual analysis requests all pending staging work before batching; some filesystem staging occurs under the database lock. | Bound issuance and move staging/revalidation outside long-held locks; delay optional work during interaction without starving reconciliation. Verify bounded issuance and interactive reads with deterministic filesystem and database tests. |

Search, filters, card-size changes, Gallery and reader images are included in the
implementation. Local text search should publish before optional semantic lookup;
Gallery should reuse indexed order, and reader images should share bounded decoding.

## Delivery and fallback

Deliver measurement support, stable image presentation, and scroll-window changes
as separate coherent checkpoints. Run the relevant code-only correctness tests and benchmarks after each checkpoint, and build the normal runnable Debug app. Notify the user without opening it.
Commit narrowly in the repository's existing style; do not push as part of this plan.
Continue through the implementation work and use the user's scrolling feedback as acceptance.

If the user still finds the completed board slow and further evidence locates the
remaining cost in SwiftUI cell lifecycle or layout overhead, investigate a bounded
`NSCollectionView` prototype against the same data, geometry, media cache and
interactions. Adopt it only if it wins that comparison while preserving behavior.
There is no current evidence requiring a wholesale app rewrite.

Apple's [SwiftUI Instruments guidance](https://developer.apple.com/videos/play/wwdc2025/306/)
supports measuring expensive updates and their causes in an optimized build.
Its [render-loop explanation](https://developer.apple.com/videos/play/tech-talks/10855/)
is the basis for separating application work from missed presentation deadlines.


## Delivered implementation and evidence

The implementation is committed in scoped checkpoints and the normal Debug app
has been rebuilt throughout. Later validation is code-only; the user owns the
scrolling and visual acceptance check.

| Area | Delivered behavior | Evidence |
| --- | --- | --- |
| Image presentation | Resolved images keep their bitmap during scrolling. Warm cached images bypass busy original-media decoders. | Presentation, cancellation, cache priority and source-replacement regressions. |
| Board updates | Raw viewport movement stays outside broad observable state. A retained window publishes only changed membership; per-card visibility uses the shared layout. | Ten small scroll updates went from ten redundant window publications to zero in the earlier hosted regression. |
| Layout | One geometry snapshot serves rendering/navigation; metadata-only edits keep geometry. Preparation yields and cancels obsolete generations. | Pure preparation and identity tests, including filtering from 100 items to two while old geometry remains active. |
| Text sizing | Three recent widths remain cached with bounded entry counts. | Optimized 10,000-quote CPU benchmark: returning to a prior width fell from 1.114 seconds to 9.7 milliseconds. |
| Media work | Validated per-device disk previews, split 128 MiB decoded-cache budget, 512 MiB disk budget, bounded directional prefetch and paced idle refinement. | Disk corruption/replacement/eviction tests and bounded/coalesced decode tests. |
| Video | Stable player layers pause during scrolling and starts are staggered. As of `da80978`, the production scheduler defaults to `Int.max`; its active-player count is not capped. | Pure admission/cancellation tests and source-generation validation. See [the board responsiveness research](board-responsiveness-research.md) for the video-heavy measurement gate. |
| Search/Gallery/reader | Fresh search shows local matches before semantic enrichment; equal snapshots skip publication; Gallery reuses indexed order; reader decoding shares bounded scheduling. | Search ordering/stale-result tests, Gallery deletion/optimistic-edit tests and independent exhaustive neighbor checks. |
| Reconciliation | Precise file events scan affected reading folders; recovery events request a full scan. Cached reads remain available during filesystem work. | Public Rust API tests for scan/write serialization, source changes and interactive reads. |
| Optional indexing | SQL limits precede asset staging; cursors advance past failures; hash lookups use an index. Work waits for 180 ms of scroll idle at bounded boundaries. | Batch staging-count, query-plan, failure-cursor and interaction-gate regressions. |

Final validation: 210 hostless macOS tests, six pure package tests, 356 Rust unit
tests and ten contract tests passed. Two live network tests stayed ignored.
Window/rendering suites were excluded from the final run. Clippy with warnings
denied passed; scoped SwiftLint passed with length/style warnings. The normal
Debug build succeeded.

The text benchmark still spends about 1.1–1.3 seconds overall preparing a cold
10,000-quote width, spread across cooperative batches. One cold batch measured
63.9 ms, so batching is a soft scheduling budget, not a maximum-duration guarantee.
These are workload-specific measurements, not a claim about observed app smoothness.

Material diagnostics are opt-in and preserve normal appearance by default. No
compositing redesign or collection-view replacement was justified by the code-only
verification. Those remain conditional follow-ups if the user finds the completed
board slow.
