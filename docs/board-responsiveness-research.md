# Mac card board responsiveness: evidence and rewrite decision

28 September 2026. Code reviewed at `da80978`; the existing profiling setup is
`107da3c` and `da80978`, and the hover menu change is `553c36e`. This is a
research and implementation plan. No renderer change has been made.

## Decision

**Prototype an AppKit `NSCollectionView` masonry board, then adopt it only if a
matched trace shows a material improvement.** Keep the SwiftUI window, toolbar,
Gallery/detail and native Markdown reader. Keep Rust `core` responsible for the
library, reconciliation, search and mutations. The collection view should use
the existing deterministic card geometry, stable reading IDs and disposable
preview service. Start with one reused `NSHostingView` per collection item so the
scroll container can be compared without changing card appearance. If cell
hosting still dominates active scrolling, compare a small native, layer-backed
card view using the same geometry, media and interaction contract. Ship the
fastest variant that passes the correctness and accessibility gates below.

The evidence justifies this comparison, **not** a claim that AppKit is
intrinsically faster. Óia already virtualizes its SwiftUI board. The measured
warm trace still has long app updates and frame latency, while media work and
rendering are separate possible bottlenecks. Apple documents explicit item
reuse, prefetching and visible-rectangle layout in `NSCollectionView`; it also
documents improved SwiftUI scrolling scheduling on macOS. Neither source
benchmarks Óia's two implementations
([NSCollectionView](https://developer.apple.com/documentation/appkit/nscollectionview),
[NSCollectionViewLayout](https://developer.apple.com/documentation/appkit/nscollectionviewlayout),
[What's new in SwiftUI, WWDC25](https://developer.apple.com/videos/play/wwdc2025/256/)).

This decision preserves the specified mixed masonry board. A uniform cropped
Photos grid would change the height, text and media treatment in
[DESIGN.md](../DESIGN.md#masonry-cards); it can be explored as a separate product
choice, not used as a performance shortcut in this rewrite.
Apple Photos is a qualitative responsiveness reference; its private macOS
implementation and Óia's mixed content are not a controlled A/B corpus.

## What has actually been measured

The three private traces are ignored under `macos/build/profiles/`. Do not
commit or share them: the real-library recordings can contain user paths and
content. All are Profile builds captured on a 14-inch MacBook Pro with macOS
27.0 and Xcode/Instruments 27.0. The baseline and hover-menu recordings start
from Xcode's DerivedData build; the warm recording attaches to an already idle
`macos/build` Profile app. The trace tables do not embed a Git revision or
gesture markers. These are not matched repetitions of the same scroll gesture.
The source notes and collection procedure are in
[performance-profiling.md](../macos/docs/performance-profiling.md).

| Observation | Local evidence | What it establishes |
| --- | --- | --- |
| Comparable 160-card rebuilds had 320 AppKit popup updates (4.68 ms total) before `553c36e` and 4 (0.18 ms) after it. Popup creations during board load fell from 85 (31.00 ms) to 2 (2.14 ms). | `real-grid-baseline.trace`, `real-grid-hover-menu-after.trace` | Constructing every hover action menu was wasted work; the change reduced that work. It is not a scroll-FPS comparison. |
| The hover-after launch recording lists six Hitches-table swap durations above 100 ms, including 450 ms at startup and 233.33 ms while a worker was reconciling the library and hashing preview assets. That 233 ms interval has no SwiftUI update or main-thread CPU sample. | `real-grid-hover-menu-after.trace` | Background work coincides with the delay; the trace does not show that it caused it. |
| The attached warm trace's Hitches table has no swap duration above 100 ms; its largest is 41.67 ms. Its separate frame-lifetimes table reaches 187.48 ms, with 18 of 409 frame-lifetime rows above 100 ms in the 7.772–21.190 s active UI-work interval. The median is 62.55 ms and the 95th percentile is 91.65 ms for those rows. | `real-grid-warm-scroll.trace`, exported `hitches` and `hitches-frame-lifetimes` schemas | Hitches-table duration and end-to-end frame lifetime are distinct; a modest maximum in one does not rule out long latency in the other. These rows do not establish presented FPS. The interval marks SwiftUI work, not verified scrolling throughout. |
| Two warm app-update stages reach 134.46 and 123.88 ms, coinciding with SwiftUI update groups of 69.54 and 68.82 ms. In the same active interval, deduplicated render stages have p95 14.95 ms and max 15.73 ms; GPU stages have p95 16.26 ms and max 17.23 ms. | `real-grid-warm-scroll.trace` | App-side update work warrants investigation; render/GPU cost also needs a controlled compositing comparison. Stage timings and sampled stacks do not alone prove causation. |
| Warm Time Profiler samples include image downsampling and disk-preview PNG encoding on workers, and context-menu construction plus repeated main-thread path/metadata work during `LocalReadingImage.body`. One main stack runs `__getattrlist → URL.appendingPathComponent → AssetImageLoader.readingFolderURL → LocalReadingImage.assetRequest → LocalReadingImage.body`. | `real-grid-warm-scroll.trace` | This establishes that at least some URL construction does filesystem work during body evaluation. Sample counts are inclusive and do not rank exclusive costs or prove the cause of a late frame. |

The Hitches export includes a row for each listed swap, many without a
`Potential Issue` value. Its row count and median duration must **not** be
reported as a count or median of actual hitches, and the screen's 120 Hz
capability must **not** be reported as measured 120 FPS. Apple's render-loop
explanation distinguishes event/commit work, render-server work, frame lifetime
and hitch duration
([Explore UI animation hitches and the render loop](https://developer.apple.com/videos/play/tech-talks/10855/),
[Demystify and eliminate hitches in the render phase](https://developer.apple.com/videos/play/tech-talks/10857/)).

The existing 10,000-reading synthetic fixture has a repeatable corpus hash
(`b43dcc9e2f1efc0a4d7492ee676567876dee886453cc827f046d555427bdca05`)
and exercises the real board, but its media are a small repeated set of images
and video, hard-linked into many reading folders. The `xctrace` attempts on
that fixture did not finish processing, so their reports say `incomplete` and
contain no usable frame trace. The successful `--no-trace` run at `553c36e`
verified three scroll starts/stops and changed offsets only; its
`frame_pacing_status` is `unverified`. See
[the fixture generator](../macos/scripts/generate-performance-fixture.py) and
[the replay driver](../macos/scripts/scroll-performance.py).

## Current design and pressure points

| Path | Current state | Consequence to test |
| --- | --- | --- |
| Virtualization and geometry | [`LazyMasonryBoard`](../macos/Sources/Oia/Features/Oia/LazyMasonryBoard.swift) uses `LazyLayoutView` with `.items(80)` and one prepared masonry snapshot. The overscan value targets roughly 80 **total** materialized cells. The package retains a window until membership changes, then publishes a new `placed` array to a SwiftUI `ForEach` ([window code](../macos/Packages/LazyLayoutKit/Sources/LazyLayoutKit/SwiftUI/LazyLayoutView.swift), [overscan semantics](../macos/Packages/LazyLayoutKit/Sources/LazyLayoutKit/Geometry/Overscan.swift)). | This is already viewport lazy. Reducing overscan alone may increase cell churn; measure creations, reconfigures and update groups per distance scrolled. |
| Whole-board changes | Board construction copies all rows; `LazyLayoutView` copies again and maps IDs and geometry keys for the complete result. Cooperative height preparation still visits every card in bounded main-actor batches ([board](../macos/Sources/Oia/Features/Oia/LazyMasonryBoard.swift), [preparation](../macos/Packages/LazyLayoutKit/Sources/LazyLayoutKit/SwiftUI/CooperativeLayoutPreparation.swift)). | Filter, search, resize and full-library refresh can cost O(number of matching readings), even if normal scroll only realizes a viewport window. Measure these separately from active scrolling. |
| Card hierarchy and input | [`OiaCardView`](../macos/Sources/Oia/Features/Oia/OiaCardView.swift) creates per-card geometry, gestures, context action, material, clipping, border, selection ring and kind-specific content. A simple selection currently maps all `readings` IDs ([Selection.swift](../macos/Sources/Oia/State/AppState/Selection.swift)). | Test main-thread update and event-to-visible-selection latency, and whether changing one card updates unrelated cells. The menu optimization already removed one unnecessary platform-view cost. |
| Media | [`LocalReadingImage`](../macos/Sources/Oia/Features/Oia/LocalReadingImage.swift) recomputes its validated asset URL while deriving body content and task IDs. The shared preview queues bound visible, refinement and prefetch decoding, check source fingerprints off-main, keep costed memory entries and asynchronously write disposable PNG previews ([decode queue](../macos/Sources/Oia/Features/Reader/Markdown/AssetPreviewDecodeQueue.swift), [disk cache](../macos/Sources/Oia/Features/Reader/Markdown/AssetPreviewDiskCache.swift)). | Do not replace the decoder blindly. Stage a safe asset descriptor once per row/content revision before rendering; measure main-thread filesystem calls. Test whether PNG writes or cold downsampling contend with scroll work, while keeping external-writer validation. |
| Video | Board playback starts are staggered, but the production scheduler's default limit is `Int.max` ([scheduler](../macos/Sources/Oia/Features/Oia/VideoPlaybackScheduler.swift), [store](../macos/Sources/Oia/Features/Oia/AutoplayVideoCard.swift)). | The old [performance plan](performance-plan.md) says there are at most two retained players; that statement is stale. Measure a video-heavy viewport and set an explicit active-player budget only if it preserves the specified behavior. |

Apple's WWDC25 SwiftUI session says both a long body and many short updates
can miss a frame deadline. It recommends using Update Groups, Time Profiler
over a selected update, and the Cause & Effect graph to distinguish expensive
body work from too-broad dependencies
([Optimize SwiftUI performance with Instruments](https://developer.apple.com/videos/play/wwdc2025/306/)).

## Renderer options

| Option | Advantage | Cost or limit | Role in experiment |
| --- | --- | --- | --- |
| Improve the current `LazyLayoutView` | Smallest behavior risk; already knows the masonry frames, anchor and accessibility surface. | Window changes still update hosted SwiftUI cells; app-level row copies and rich card bodies remain. Apple has improved SwiftUI scheduling, so this remains a legitimate candidate. | **Control implementation.** Apply renderer-independent media/input fixes to it and profile again. |
| SwiftUI `LazyVGrid` | Framework-managed lazy creation for a regular grid ([Apple API](https://developer.apple.com/documentation/swiftui/lazyvgrid)). | Rows and fixed columns change Óia's true mixed masonry card geometry. A generic custom SwiftUI `Layout` does not promise lazy realization ([Apple API](https://developer.apple.com/documentation/swiftui/layout)). | Exclude from behavior-preserving rewrite; revisit only with a product design change. |
| `NSCollectionView` plus reused `NSHostingView` items | Explicit item reuse, visible-item inspection, prefetch cancellation and targeted layout invalidation while reusing today's card content. Apple shows one persistent hosting view per recycled cell ([Use SwiftUI with AppKit](https://developer.apple.com/videos/play/wwdc2022/10075/)). | Hosted card body and compositing can remain expensive; focus, sizing and accessibility need deliberate bridging. | **First AppKit prototype** against the same data, geometry and cache. |
| `NSCollectionView` plus native, layer-backed card items | Most explicit control over cell reuse, bitmap publication, text/layer tree and context action construction. | Highest parity cost for mixed card kinds, SwiftUI styling and accessibility. Does not automatically solve decoding or disk contention. | Prototype the hottest card kinds only if hosted cells miss the performance gate. |

`NSCollectionViewFlowLayout` is suitable for a uniform grid but not this
variable-height masonry board. A custom `NSCollectionViewLayout` should accept
an immutable geometry snapshot and answer visible-rectangle queries from an
index, not recompute frames or scan all readings per scroll. Apple's layout API
specifies visible-rectangle attributes, caching and invalidation; its collection
view prefetch protocol supplies request and cancellation hooks
([layout](https://developer.apple.com/documentation/appkit/nscollectionviewlayout),
[prefetching](https://developer.apple.com/documentation/appkit/nscollectionviewprefetching)).

## Proposed implementation, in decision order

1. **Make the comparison valid.** Extend the isolated profiler to save the
   display refresh rate, window/card size, Git revision, fixture hash, cache
   state and launch/idle state; capture a short trace whose Hitches,
   frame-lifetime and Time Profiler data are nonempty. Record SwiftUI Update
   Groups for the current/hosted boards, and AppKit item/layout signposts for
   the native candidate. Fail the benchmark when Instruments stalls or a
   required table is missing. Keep traces in ignored `build/profiles/`.
2. **Remove renderer-independent work.** Prepare and cache a validated asset
   descriptor before cell rendering, keyed by reading ID, relative asset
   reference and content generation. Keep source fingerprint checks and
   invalidation off-main so external writers remain safe. Move selection's
   all-ID mapping out of the click path. Run controlled scroll A/B captures
   with disk-preview writes on and deferred until idle; retain the version
   that measurably lowers latency without weakening cache correctness. Keep
   existing bounded decodes and stable already-displayed bitmaps. Apple
   recommends display-sized Image I/O thumbnails rather than full image
   decompression, and asynchronous video frame generation
   ([Image I/O thumbnail bound](https://developer.apple.com/documentation/imageio/kcgimagesourcethumbnailmaxpixelsize),
   [AVFoundation video images](https://developer.apple.com/documentation/avfoundation/creating-images-from-a-video-asset)).
3. **Build the AppKit comparison behind one board switch.** A top-level
   `NSViewRepresentable` owns an `NSScrollView`/`NSCollectionView`. Feed it
   stable ordered reading IDs and revisioned row values from the current
   `AppState`/Rust query. Use a diffable data source for membership and order;
   reload only changed readings. Feed the same precomputed masonry frames to
   the custom collection layout, navigation and prefetch window. Invalidate
   geometry on width, card size or metric changes, not selection or scroll.
   Keep a bounded pool of collection items, each with one hosting view; on
   reuse, cancel old media publication and check reading ID plus generation
   before applying an asynchronous result.
4. **Test native cells only if needed.** If matched traces show hosted cell
   updates or render layers dominate after step 3, replace high-volume image,
   quote and article preview cells with compact AppKit views. Use the same
   reading-specific card data, cache and action coordinator. Keep the existing
   native SwiftUI reader and detail overlay. Compare material/clip diagnostics
   with identical geometry and card content before changing appearance;
   Apple's render-phase guidance identifies masks, rounded corners and visual
   effects as possible offscreen work
   ([render-phase hitches](https://developer.apple.com/videos/play/tech-talks/10857/)).
5. **Adopt the winner and remove the experiment switch.** Preserve the file
   format and native-messaging protocol. Derived thumbnails and any geometry
   index remain disposable per-device data outside the library; no absolute
   path enters the synced library or SQLite index. The Rust core remains the
   authority for reading state, search and reconciliation.

Do not add a blanket `.drawingGroup()` to cards: Apple documents that it
creates an offscreen bitmap and excludes AppKit views; the render-stage trace
must identify a specific layer before changing compositing
([drawingGroup](https://developer.apple.com/documentation/swiftui/view/drawinggroup%28opaque%3Acolormode%3A%29)).

## Repeatable benchmark and acceptance

The existing commands build and exercise a deterministic, isolated library;
the second command verifies replay **only** until reliable trace capture has
been added:

```sh
cd macos
make profile-build
./scripts/check-scroll-performance.sh \
  --fixture build/profiles/fixture \
  --output build/profiles/board-replay-$(date +%Y%m%d-%H%M%S) \
  --no-trace
```

For frame evidence, use the [SwiftUI Instruments procedure](../macos/docs/performance-profiling.md#record-a-real-interaction)
on an optimized Profile build. Attach after reconciliation for **warm** runs;
record startup separately. Use the same machine, OS/Xcode, display/refresh
setting, window size, card-size level, theme, library/fixture hash, preview
cache state and gesture distance/speed for both renderers. Run five paired
recordings in alternating order per scenario and report each result plus the
median, not a single best run. Keep a realistic diverse-media local library as
a private face-validity check; the generated fixture's repeated assets cannot
represent all media cost.

Export the comparable Hitches-stage tables from each private trace for the
same marked scroll interval. From `macos/`, the existing trace can be
inspected with:

```sh
xcrun xctrace export --input build/profiles/real-grid-warm-scroll.trace \
  --xpath '/trace-toc/run/data/table[@schema="hitches" or @schema="hitches-frame-lifetimes" or @schema="hitches-updates" or @schema="hitches-renders" or @schema="hitches-gpu"]' \
  --output /tmp/oia-warm-frame-tables.xml
```

Deduplicate identical `(start, duration)` pairs in render/GPU exports before
summarizing them; some rows describe the same stage in different colors. Use
the Hitches instrument's actual hitch duration/ratio, not the sum of every
swap-row duration. Mark the exact scroll interval with signposts in automated
runs. The existing warm trace has no such marker, so its SwiftUI-work interval
is useful for diagnosis but not an acceptance-grade scroll interval.

| Scenario | Measurements to retain |
| --- | --- |
| Idle attach, warm slow scroll, fast scroll and reversal | Hitches duration and hitch-time ratio in the active interval; frame-lifetime p50/p95/p99/max; app-update, render and GPU stage p95/max; item creations, reconfigures and live item count; main-thread file metadata calls; preview cache hits/misses/source decodes and publication bursts. |
| Cold launch followed by first scroll | Time to first useful board, reconcile/preview signposts, long app updates, worker CPU and I/O, frame-lifetime and Hitches tables. Never pool this with warm-scroll results. |
| Mixed-video viewport and stop/start | Active players, frame generation, media CPU, app/render stages and memory. Test Reduce Motion and inactive-window behavior too. |
| Filter/search, size change, selection and card actions | Input-to-visible-change latency; rows/body updates per one-card edit; geometry solves, anchor preservation, menu construction. |
| Repeated full traversal of a 10,000-reading library | Live item count versus viewport size; peak and settled resident memory; decoded/disk-cache bytes; no monotonic growth; correct first/last card and no gap. This is a scalability check until a usable frame trace exists for the large fixture. |

Use the Hitches and frame-lifetime tracks as separate quantities. A full trace
with `replay_captured` and an exported table is needed for timing; the current
script's `replay_verified_without_trace` and `screen_maximum_frames_per_second`
fields are not timing results. At 120 Hz one refresh period is about 8.33 ms,
but the app has less than that for its own work because rendering also needs
time. Apple recommends keeping continuous-update and collection-view callback
work near 5 ms, and explains why both commit and render stages can miss a
deadline
([Improving app responsiveness](https://developer.apple.com/documentation/xcode/improving-app-responsiveness),
[render loop](https://developer.apple.com/videos/play/tech-talks/10855/)).

The following are **proposed Óia gates**, not Apple certification or a claim
that Photos uses these numbers. Use the display's measured cadence; the
parenthetical time values assume 120 Hz:

- In at least four of five paired warm-scroll runs, hitch-time ratio is under
  5 ms/s, no individual active-scroll hitch exceeds four refresh periods
  (33.3 ms), frame-lifetime p95 is at most four periods (33.3 ms) and p99 at
  most eight (66.7 ms). No warm frame lifetime exceeds 100 ms. Three/six
  periods are stretch targets. If Instruments
  cannot report a quantity reliably, mark that gate unverified and fix the
  measurement before making a renderer decision.
- The candidate improves both median hitch-time ratio and p95 frame lifetime
  by at least 20% over the renderer-independent-fixes baseline, **and** the
  paired difference is larger than run-to-run variation. Measure app update,
  render and GPU stages to confirm the improvement is where expected.
- One card selection and tag edit visibly respond without an unrelated
  board-wide rebuild; selected-card actions and `⌘F` stay responsive during
  background reconcile. Over 20 repeated click/keyboard selections, target
  p95 input-to-visible-selection below 50 ms and no result above 100 ms,
  measured from the Instruments User Events and displayed-frame tracks. Warm
  scrolling performs no original-media decode for already cached cards and
  no library-file metadata read from card construction on the main thread.
- Live cells remain proportional to the viewport rather than library size;
  memory plateaus over repeated traversals; preview and video work remains
  bounded. A candidate with better frame timing but unbounded memory or
  incomplete cards fails.
- All current board behavior passes: mixed card proportions/order; scope,
  search and tag filtering; one-motion optimistic removal/selection advance;
  shift selection and spatial keyboard navigation; zoom anchoring; hover and
  context actions; Quick Look and origin actions; whole-board paste/drop;
  video pause/resume; Gallery/reader handoff. Keyboard and VoiceOver must reach
  every card and its actions, including the actions that are visually revealed
  only on hover. Apple specifically advises checking keyboard focus with Full
  Keyboard Access both on and off
  ([Use SwiftUI with AppKit](https://developer.apple.com/videos/play/wwdc2022/10075/)).

If the AppKit candidate wins timing but misses behavior or accessibility, do
not replace the board until those gaps are closed. If it fails to beat the
current board after shared media/input fixes, keep the SwiftUI board and use
the traces to target the remaining measured cost.
