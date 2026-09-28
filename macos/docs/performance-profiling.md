# Profile Óia with SwiftUI Instruments

Use the SwiftUI template in Instruments to locate long view updates, excessive
updates, and actual hitches. The built-in instrument needs no app code changes.
Óia's shared Xcode scheme already sets **Profile** to **Release**, so a profile
uses optimized Swift code with debug symbols. Xcode 26 or newer and an OS that
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
content and paths. Keep traces local and out of commits. A Release build shares
the normal `macos/build` derived-data path; the Debug app remains at
`macos/build/Build/Products/Debug/Óia.app`.

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
installed SwiftUI template, launches an isolated app process, records
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

Open each `scroll.trace` in Instruments and compare the active scroll periods in
the SwiftUI, Time Profiler, and Hitches tracks. The `report.json` file
intentionally labels frame pacing **unverified**:
the synthetic scroll confirms the app moved and SwiftUI saw scroll transitions,
but only the trace's Hitches evidence and a real gesture can assess perceived
smoothness. The generated corpus also repeats a small set of media, so validate
any conclusion against a realistic library.

The driver accepts `--count` (default 10,000), `--label`, and `--app` for an
explicit executable. Keep the same count, fixture, display, and app configuration
when comparing runs. Use a fresh output directory every time; the driver never
overwrites a result.

## References

- [Optimize SwiftUI performance with Instruments (WWDC25)](https://developer.apple.com/videos/play/wwdc2025/306/)
- [Understanding and improving SwiftUI performance](https://developer.apple.com/documentation/xcode/understanding-and-improving-swiftui-performance)
