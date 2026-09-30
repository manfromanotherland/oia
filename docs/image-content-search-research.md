# Image Content and Colour Search Research

Checked **2026-08-25** against Apple's public documentation, WWDC sessions, SDK behaviour, and
first-party model licences; platform status and product decision updated **2026-09-30**. The
current Óia macOS deployment target is 15.0 in
[`macos/project.yml`](../macos/project.yml).

## Decision

Yes: Óia can search the pixels in saved images locally. It does not need branded Apple
Intelligence for the first useful version.

The recommended stack is:

1. On **macOS 15 and later**, donate each image to **Core Spotlight's semantic media index** and
   query it with `CSUserQuery`. This is the closest public Apple API to typing `chair` and getting
   an image whose pixels depict a chair, without bundling a model.
2. On the current **macOS 15 baseline**, run
   **Vision image classification** and index its labels and confidence values. Apple's own sample
   is specifically about using those labels for categorisation and search.
3. Compute weighted dominant colours with **Core Image `kMeans()`**. This is deterministic,
   local, and more appropriate than a generative model for a query such as `blue`.
4. Use **Vision FeaturePrint** for a separate “find similar images” feature. It cannot turn a text
   query into an image match.
5. Consider **Foundation Models image prompting on macOS 27** for richer captions or structured
   visual attributes. It requires Apple Intelligence and is not a safe sole indexing path.
6. Bundle a licensed **Core ML text/image dual encoder** only if quality testing shows that Core
   Spotlight and Vision are insufficient and Óia needs consistent open-vocabulary retrieval
   across OS versions.

Generated machine subject tags belong in the reading's Markdown frontmatter, alongside user tags
with separate provenance. User tags win case-insensitive duplicates, and removing a machine tag
must be durable in the Markdown file. Raw Vision labels and confidences, palettes, captions,
feature prints, embeddings, and Spotlight donations remain disposable per-device analysis data.
See [Automatic Subject Tagging Research](./automatic-subject-tagging-research.md).

## Capability Matrix

| Need | Best public API | Minimum macOS | Apple Intelligence | Local/offline behaviour |
| --- | --- | ---: | --- | --- |
| Natural-language text to image pixels | Core Spotlight semantic media indexing + `CSUserQuery` | 15 | No requirement documented | Private, entirely local index; semantic models download to the device |
| Explicit automatic labels/categories | Vision `VNClassifyImageRequest` | 10.15 | No | Built-in on-device classifier; no model in the app bundle |
| Modern Swift Vision classification API | Vision `ClassifyImageRequest` | 15 | No | Same role, with Swift concurrency APIs |
| Similar-image search | Vision FeaturePrint | 10.15 legacy API; 15 modern API | No | Image-to-image comparison on device |
| Predominant-colour search | Core Image `kMeans()` | 11 | No | Deterministic local image processing |
| Custom classifier, detector, or text/image embeddings | Core ML, optionally through Vision | 10.13 for Core ML; ML Programs from 12 | No | Local when the model is bundled or already downloaded |
| Rich image captions and custom structured attributes | Foundation Models image `Attachment` | 27 | Yes | On-device and offline once the system model is available |
| Read the Photos app's own inferred object labels | No public PhotoKit API | — | — | PhotoKit exposes assets and documented metadata, not Photos' private semantic index |

## Core Spotlight Is the Closest Stable Answer to `chair`

Apple's WWDC24 session [Support semantic search with Core
Spotlight](https://developer.apple.com/videos/play/wwdc2024/10131/) says all of the following
explicitly:

- an app's donated content is stored in a private, entirely local index that never leaves the
  device and isn't visible to other apps;
- semantic search works best on text and media assets such as images and videos;
- an image or video's `CSSearchableItemAttributeSet.contentURL` should point at the asset so that
  the asset itself is processed into the semantic index;
- `CSUserQuery` accepts the natural-language search string and returns ranked items;
- the semantic models are downloaded to the device and run in the app's process.

Apple documents semantic `CSUserQuery` search on [macOS 15 and
later](https://developer.apple.com/documentation/corespotlight/building-a-search-interface-for-your-app).
The framework's [2024 update](https://developer.apple.com/documentation/updates/corespotlight)
describes results that are similar in meaning to a query rather than lexical matches. A query can
also be restricted to images with a filter such as `contentTypeTree="public.image"`.

This is not the Foundation Models framework and Apple documents no dependency on the Apple
Intelligence setting, an M-series Mac, or a Foundation Models availability check. The documented
requirements are macOS 15 or later and downloadable semantic-model resources. That is an absence
of a documented Apple Intelligence gate, not a promise that quality or resource availability is
identical on every supported Mac.

### Limits and Óia-specific risks

- Spotlight returns ranked matching item IDs but does not expose the image labels, captions, or
  embedding vectors that produced the match. Vision is still needed if Óia wants visible
  categories or category facets.
- Apple does not publish the media model's vocabulary or guarantee recall for any particular term.
  `chair`, small incidental objects, illustrations, and screenshots need a representative quality
  test.
- Donated items can also be found by the user in system Spotlight. Other apps cannot query the
  private app index, but making Óia cards discoverable in the system UI should be a deliberate
  product decision.
- Apple's session says the `contentURL` lets Spotlight process a media asset from the app's
  sandboxed container. Óia' source assets live in a user-selected, security-scoped library
  folder. A prototype must confirm that Spotlight can retain the required access. The robust
  fallback is to place a small derived thumbnail in per-device Application Support and donate that
  URL; the original library image remains canonical.
- The app must be able to reconstruct all donations after Spotlight asks for reindexing or after
  its local index is lost.

Core Spotlight therefore fits Óia as an additional **disposable platform search index**, not
as the source of truth. A narrow macOS adapter can donate images and return candidate reading IDs;
the Rust core should still own parsing, filters, result merging, and pagination.

## Vision for Automatic Categorisation

Apple's sample [Classifying images for categorization and
search](https://developer.apple.com/documentation/vision/classifying-images-for-categorization-and-search)
does almost exactly the proposed fallback. A classification request returns multiple identifiers
and confidence values for an image. The sample filters them for a chosen precision/recall tradeoff,
stores them, and searches images by label.

The legacy [`VNClassifyImageRequest`](https://developer.apple.com/documentation/vision/vnclassifyimagerequest)
is available on Óia's macOS 15 baseline. The newer `ClassifyImageRequest` Swift API is macOS
15+. Apple recommends trying this built-in classifier before bundling a third-party classifier
because it avoids app-size cost and may perform better; see [Classifying Images with Vision and
Core ML](https://developer.apple.com/documentation/coreml/classifying-images-with-vision-and-core-ml).

A direct check of `VNClassifyImageRequest().supportedIdentifiers()` on the current development Mac
returned **1,303 identifiers**, including `chair`, `armchair`, `folding_chair`, `high_chair`,
`swivel_chair`, and `dining_room`. That verifies the exact example on this runtime, but the stored
analysis must record its Vision request revision because Apple can change the model and taxonomy.

Vision classification has important limits:

- it uses a fixed system taxonomy rather than arbitrary open-vocabulary text;
- labels describe the image as a whole and may miss a small chair in a wide dining-room shot;
- lowering the confidence threshold improves recall but creates more false positives, while a
  high-precision threshold omits more valid images;
- it returns categories, not general-purpose text/image embeddings.

For explicit bounding boxes around arbitrary objects such as chairs, run a **custom Core ML object
detector** through Vision. Vision supplies the image preprocessing and observations, but the custom
model defines the detectable classes. Saliency is cheaper when Óia only needs to focus later
analysis on likely foreground objects: [`VNGenerateObjectnessBasedSaliencyImageRequest`](https://developer.apple.com/documentation/vision/vngenerateobjectnessbasedsaliencyimagerequest)
returns a heat map of image areas likely to represent objects, but it does not name them.

## FeaturePrint Is for “More Like This,” Not Text Search

Vision's [FeaturePrint sample](https://developer.apple.com/documentation/vision/analyzing-image-similarity-with-feature-print)
generates a representation for each image and compares two feature prints. Apple documents that
[shorter distances mean greater
similarity](https://developer.apple.com/documentation/vision/vnfeatureprintobservation/computedistance(_:to:)).

This is useful for:

- “find visually similar cards”;
- near-duplicate detection;
- clustering a library by visual similarity.

It cannot implement `chair` by itself. The public API has an image encoder and image-to-image
distance, but no text encoder that maps the word `chair` into the same space. FeaturePrint data
also needs a recorded request revision and rebuilding when that revision changes.

## Predominant-Colour Search

Colour search should be a deterministic image-analysis feature, not an Apple Intelligence prompt.

Core Image's [`kMeans()`](https://developer.apple.com/documentation/coreimage/cifilter-swift.class/kmeans())
finds the most common colours in an image. Each output pixel is a cluster centre and its alpha
component is that colour's weight. The filter can work in a perceptual colour space. Apple also
publishes an [Accelerate dominant-colours
sample](https://developer.apple.com/documentation/accelerate/calculating-the-dominant-colors-in-an-image)
if Óia needs more control over clustering.

A practical pipeline is:

1. honour EXIF orientation, convert HDR/wide-gamut input to a consistent working colour space, and
   downsample;
2. compute a small perceptual palette with cluster weights;
3. convert cluster centres to a perceptual representation such as Lab;
4. map them to a controlled Óia vocabulary such as red, orange, yellow, green, teal, blue,
   purple, pink, brown, black, grey, and white;
5. store the centre, weight, mapped name, and analyser revision;
6. rank `blue` by both perceptual distance to blue and the blue cluster's coverage, so a mostly-blue
   image outranks an image with one tiny blue object.

`CIAreaAverage` is faster, but one average pixel is a poor definition of “predominant”: unrelated
colours can blend into grey or another colour that barely exists in the source. If product testing
shows that people mean the foreground subject's colour rather than the whole frame, weight pixels
using Vision's objectness saliency heat map before clustering. Keep whole-frame and salient-subject
colour as distinct signals rather than silently changing the meaning.

Plain colour words can contribute a colour score alongside text and semantic scores. A future
explicit `color:blue` filter would remove ambiguity for searches such as `blue chair` or `blue
whale`.

## Foundation Models and Branded Apple Intelligence

The Foundation Models framework gives apps the system model behind Apple Intelligence. Apple's
WWDC25 introduction says the model runs on device, keeps input and output private, and can operate
offline; it also says apps must check availability because it only runs on Apple
Intelligence-enabled devices and supported regions. See [Meet the Foundation Models
framework](https://developer.apple.com/videos/play/wwdc2025/286/).

There are two materially different platform generations:

- **macOS 26:** Foundation Models is useful for text. Its specialised content-tagging model
  generates topics, actions, objects, and emotions from [input
  text](https://developer.apple.com/documentation/foundationmodels/categorizing-and-organizing-data-with-content-tags).
  It could consolidate text labels already produced by Vision, but it cannot inspect raw image
  pixels through the macOS 26 public API.
- **macOS 27:** the on-device model gains image input through
  [`Attachment`](https://developer.apple.com/documentation/foundationmodels/attachment). Apple's
  [multimodal prompting
  guide](https://developer.apple.com/documentation/FoundationModels/analyzing-images-with-multimodal-prompting)
  documents image classification, scene understanding, descriptions, and structured extraction.
  Apple's [Foundation Models updates](https://developer.apple.com/documentation/updates/foundationmodels)
  place image analysis in the June 2026/macOS 27 generation and tell developers to retest prompts
  when the OS model changes.

On macOS 27, an image `Attachment` plus guided generation could produce richer derived data such
as a caption, scene, objects, style, and a small controlled category enum. It is not the right
query-time retrieval engine: prompting every image for every keystroke would be slow, and the
public API documents generated responses rather than a reusable image embedding or nearest-neighbour
index. Precompute results in the background and cache them by content hash, prompt revision, and
system-model version.

### Availability constraints

Apple's [`SystemLanguageModel`](https://developer.apple.com/documentation/foundationmodels/systemlanguagemodel)
requires a runtime availability check. Documented failure reasons include
`deviceNotEligible`, `appleIntelligenceNotEnabled`, and `modelNotReady`. Apple's [current support
requirements](https://support.apple.com/en-us/121115) include a Mac with M1 or later, Apple
Intelligence enabled, supported language/region settings, and downloaded on-device models.

macOS 27 [shipped on 14 September 2026](https://www.apple.com/uk/newsroom/2026/09/major-updates-for-apples-software-platforms-are-now-available/);
Apple released [27.0.1 on 28 September 2026](https://developer.apple.com/news/releases/?id=09282026c).
The image-attachment API can therefore be considered for a shipping macOS 27 feature. Óia must
preserve a non-Foundation-Models path because users can disable Apple Intelligence or its model
can be unavailable.

The ordinary on-device system model has no special entitlement documented. Custom Foundation
Models adapters require Apple's
[`com.apple.developer.foundation-model-adapter`](https://developer.apple.com/documentation/BundleResources/Entitlements/com.apple.developer.foundation-model-adapter)
permission before App Store submission. Private Cloud Compute is a separate, networked path
with a [managed entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.private-cloud-compute);
it conflicts with an offline core feature and is unnecessary here. Any use must also follow
Apple's [Foundation Models acceptable-use
requirements](https://developer.apple.com/apple-intelligence/acceptable-use-requirements-for-the-foundation-models-framework/).

## Core ML for Full Open-Vocabulary Retrieval

If Óia needs complete control, a dual-encoder model can map images and text into the same
vector space:

1. precompute and normalise one image embedding per asset;
2. encode the query text such as `chair` at search time;
3. rank image vectors by cosine similarity;
4. combine that score with FTS, tag, kind, and colour filters in the Rust core.

Core ML runs models locally on the CPU, GPU, and Neural Engine. Apple's [Core ML Tools
overview](https://apple.github.io/coremltools/docs-guides/source/overview-coremltools.html) says
strictly on-device execution needs no network and keeps data private. Apple's ML Program format is
available from [macOS 12](https://apple.github.io/coremltools/docs-guides/source/convert-to-ml-program.html),
so a compatible converted model can support Óia's macOS 15 baseline.

Apple's [MobileCLIP research](https://machinelearning.apple.com/research/mobileclip) demonstrates
efficient text/image retrieval, and its official iOS sample demonstrates [zero-shot scene
classification with Core ML](https://github.com/apple/ml-mobileclip/blob/main/ios_app/README.md).
It is technically relevant but not currently a production model choice: Apple's
[`LICENSE_MODELS`](https://github.com/apple/ml-mobileclip/blob/main/LICENSE_MODELS) limits the
released weights to non-commercial research and explicitly excludes product development and use
in a commercial product or service. The code's MIT licence does not override the weights licence.
Any bundled model needs a separately verified licence compatible with Óia' distribution.

This path also adds app size, conversion work, performance tuning, quality evaluation, and a model
upgrade contract. It should be a measured response to quality gaps rather than the default first
implementation.

## PhotoKit and Apple's Photos App

PhotoKit is not a bridge into the Photos app's private semantic intelligence. Apple's [Fetching
Assets](https://developer.apple.com/documentation/photokit/fetching-assets) documentation exposes
asset and collection retrieval, documented metadata, and image/video content after explicit user
authorisation. It does not document a query for Photos' inferred object labels, captions,
embeddings, or natural-language search index. Image requests may also fetch missing iCloud content
from the network.

The new [photo-app Apple Intelligence
integration](https://developer.apple.com/documentation/AppIntents/integrating-your-photo-app-with-apple-intelligence)
works in the other direction: an app donates its own entities to Spotlight and Siri. It does not
return the Photos app's categories to Óia.

Óia already owns local image files, so PhotoKit would add an unnecessary Photos permission.
Do not inspect Photos' private database or use internal APIs: App Review rule 2.5.1 permits only
public APIs.

## Recommended Óia Design

### Derived data

Keep raw machine analysis in the per-device disposable index. Subject labels converted into
machine Tags are written to reading frontmatter (see the [tagging note](./automatic-subject-tagging-research.md)):

```text
image_analysis
  reading_id, asset_relpath, asset_sha256, analyser, analyser_version, status

image_labels
  reading_id, label, confidence, analyser_version

image_colours
  reading_id, name, lab_components, coverage, analyser_version

image_embeddings                  # optional later
  reading_id, model_id, dimension, normalised_vector
```

Core Spotlight donations form another rebuildable per-device index keyed by the stable reading ID.
If a derived thumbnail is required for sandbox access, keep it outside the library beside
the ordinary local cache.

### Ownership boundary

Apple's image frameworks are platform APIs, so the macOS layer needs a narrow adapter. It should
accept local image bytes/URLs and return plain analysis facts or Spotlight candidate IDs. The Rust
core should own:

- deciding which assets are stale from relative path, content hash, and analyser revision;
- persistence in the disposable index;
- merging FTS, user and machine Tags, Vision label, Spotlight semantic, colour, and optional vector scores;
- kind/Tag filters, pagination, and ordering;
- reconciliation after files are added, removed, or changed externally.

This keeps Swift as the platform capability boundary rather than a second search/domain
implementation.

### Staged delivery

1. **macOS 15 baseline:** Vision labels plus Core Image dominant colours, analysed in the
   background and cached by asset hash. Add Core Spotlight media donation and `CSUserQuery` for
   natural-language pixel search; keep Vision as fallback and inspectable category data.
2. **Independent feature:** FeaturePrint-powered “similar images.”
3. **macOS 27 enhancement:** optional structured Foundation Models captions/categories when
   `SystemLanguageModel` is available.
4. **Only after evaluation:** a production-licensed Core ML dual encoder if Spotlight/Vision recall,
   reproducibility, or cross-version behaviour is not good enough.

Before choosing thresholds, test a small representative corpus:
prominent and incidental chairs, dining rooms with partially hidden chairs, screenshots,
illustrations, low-light images, monochrome images, and images where blue is a small accent versus
the dominant field. Record precision and missed results for both Core Spotlight and Vision rather
than treating either system model as a fixed contract.
