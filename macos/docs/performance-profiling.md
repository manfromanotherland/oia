# Profile Óia with SwiftUI Instruments

Use the SwiftUI template in Instruments to locate long view updates, excessive
updates, and actual hitches. The built-in instrument needs no app code changes.
Óia's shared Xcode scheme sets **Profile** to a release-optimized configuration
with debug symbols. Its local ad-hoc signature omits the hardened runtime so
it can load the bundled Sparkle framework; the shippable **Release** configuration
retains hardened runtime. Xcode 26 or newer and an OS that
supports recording the new SwiftUI trace are required; confirm support by making
a short recording and checking that the SwiftUI lanes contain data.

## Record a real interaction

1. Open `macos/Oia.xcodeproj` in Xcode and select the `Oia` scheme on **My Mac**.
2. Choose **Product → Profile** (`⌘I`), then select the **SwiftUI** template in
   Instruments. Press **Record**.
3. Exercise one named scenario, such as board scroll start and sustained scroll,
   changing a kind or tag filter, searching, or opening an article and scrolling
   its reader. Repeat the same gesture and library conditions in later runs.
4. Stop recording and wait for Instruments to finish processing. Save the
   `.trace` locally, for example under the ignored `macos/build/profiles/` folder.

Profiling the normal app uses the selected library, so a trace can include its
content and paths. Keep traces local and out of commits. `make profile-build`
writes to `macos/build/Build/Products/Profile/Óia.app`; the Debug app remains
at `macos/build/Build/Products/Debug/Óia.app`.

## Read the trace

Start with the **Hitches** track and **SwiftUI → Update Groups** around the
interaction. A long group with no single long update can mean many short updates
accumulated. The **Long View Body Updates** lane marks body calls above 500 µs in
orange and above 1 ms in red. **Long Platform View Updates** covers hosted AppKit
views; **Other Long Updates** includes layout and text work. Focus on repeated
updates during interaction; first-frame setup can be slower without causing a
scroll hitch.

For a slow body, select its update, set the inspection range to that update, and
inspect the same range in **Time Profiler** for the expensive call stack. For too
many updates, select an Update Group and inspect **Summary: All Updates** to see
which view bodies ran and how often. Choose **Show Causes** on an update to inspect
the Cause & Effect Graph, then follow the state or model dependency back to its
writer. Re-record the same scenario after a change and compare both the SwiftUI
work and the Hitches track. A red update is a lead to investigate, not proof of
a visible hitch.

Record the scenario, Git revision, Mac/OS/Xcode version, library size, and the
trace's hitch and update evidence alongside any before/after conclusion. Do not
infer FPS from scroll event delivery or a successful trace file.

## Repeatable isolated board scroll

From `macos/`, build and profile an optimized app against a generated offline
library. The fixture generator requires `ffmpeg`. The driver checks for the
installed SwiftUI template, launches an isolated app process, attempts to record
`scroll.trace`, and writes `report.json` with the revision, fixture identity,
scroll replay counters, and trace status. It activates a visible app window and
drives a synthetic scroll, so run it when the screen is available.

```bash
make profile-build
./scripts/check-scroll-performance.sh \
  --fixture build/profiles/fixture \
  --output build/profiles/scroll-baseline
```

After an app change, rebuild and reuse the same fixture with a new output path:

```bash
make profile-build
./scripts/check-scroll-performance.sh \
  --fixture build/profiles/fixture \
  --output build/profiles/scroll-after
```

Open a `scroll.trace` only when `report.json` says `replay_captured` and
`trace_toc_exported` is true. Xcode 27's command-line `xctrace` can collect a
large SwiftUI fixture but stall while processing it. In that case, discard the
incomplete trace, use `--no-trace` to verify the isolated replay, and record a
short real scroll with Xcode and Instruments as described above. Compare the
active scroll periods in the SwiftUI, Time Profiler, and Hitches tracks. The
`report.json` file intentionally labels frame pacing **unverified**:
the synthetic scroll confirms the app moved and SwiftUI saw scroll transitions,
but only the trace's Hitches evidence and a real gesture can assess perceived
smoothness. The generated corpus also repeats a small set of media, so validate
any conclusion against a realistic library.

The driver accepts `--count` (default 10,000), `--label`, and `--app` for an
explicit executable. Keep the same count, fixture, display, and app configuration
when comparing runs. Use a fresh output directory every time; the driver never
overwrites a result.

## Grid measurement on 28 September 2026

Two short SwiftUI recordings of the real library used Xcode 27 and macOS 27.
Each contained a comparable board rebuild with 160 card updates. Rendering the
card action menu only on hover reduced AppKit popup updates in that rebuild from
320 (4.68 ms total) to 4 (0.18 ms), and popup creations during board load from
85 (31.00 ms) to 2 (2.14 ms). This isolates a real source of wasted view work.

The board still spent about 5–7 ms each in `OiaLibraryView.body` and
`LazyMasonryBoard.body` during rebuilds. A later 233 ms hitch occurred while
the startup library reconciliation was scanning files and hashing preview
assets on a worker thread. That interval contained no SwiftUI update or
main-thread CPU samples; the trace does not establish what delayed the frame.
A 24-second trace attached to the already idle Profile app contained no scanner
samples or hangs, and its largest reported Hitch was 41.67 ms. The earlier
launch trace contained six Hitches over 100 ms. This comparison suggests that
concurrent reconciliation matters, but the gestures and launch state differed,
so it does not establish causation. Warm end-to-end frame latency still reached
187 ms during a long app update; frame latency and the Hitches track measure
different parts of the pipeline. Warm Time Profiler samples concentrated in
image downsampling, disk-preview PNG encoding, and context-menu construction;
these are candidates for a controlled follow-up measurement. Steady render and
GPU stages also reached about 15–17 ms and 13–14 ms in the earlier recording,
respectively, so compositing deserves a separate look for a 120 Hz goal. The
Hitches track reported roughly 33 ms frame lifetimes during quiet periods;
these recordings do not establish scroll FPS or parity with Photos. The private
`.trace` files remain in the ignored `macos/build/profiles/` directory on the
profiling Mac.

## References

- [Optimize SwiftUI performance with Instruments (WWDC25)](https://developer.apple.com/videos/play/wwdc2025/306/)
- [Understanding and improving SwiftUI performance](https://developer.apple.com/documentation/xcode/understanding-and-improving-swiftui-performance)
