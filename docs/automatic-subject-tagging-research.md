# Automatic Subject Tagging Research

Checked **2026-09-30** against Apple's public documentation. Óia currently targets macOS 15
([`macos/project.yml`](../macos/project.yml)). This note covers subject Tags for readings; the
related [image search research](./image-content-search-research.md) covers visual retrieval,
colour, and similar-image search.

## Decision for Óia

Use the on-device [Foundation Models content-tagging use
case](https://developer.apple.com/documentation/foundationmodels/categorizing-and-organizing-data-with-content-tags)
for text on macOS 26 and later when `SystemLanguageModel` is available; the [macOS 26 release
notes](https://developer.apple.com/documentation/macos-release-notes/macos-26-release-notes)
introduced direct access to the on-device model. It is designed to extract
topics, actions, objects, and emotions from input text. Request a small set of **topics** for the
subject matter of an article, link with text, post, or quote. Apple says short inputs work best
with topic or emotion tagging; asking for objects or actions from a brief quote can just repeat
its words. Use a fresh session per reading because Apple warns that reusing a session can mix tags
from earlier turns. The model can return one-to-few-word lowercase tags and supports a bounded
structured result with `@Generable` and `@Guide(.maximumCount(...))`.

For image Tags on macOS 15 and later, run Vision's
[`ClassifyImageRequest`](https://developer.apple.com/documentation/vision/classifyimagerequest)
on the locally saved image. Apple's [classification sample](https://developer.apple.com/documentation/vision/classifying-images-for-categorization-and-search)
returns labels with confidence values and explicitly uses those labels for categorisation and
search. Run [`RecognizeTextRequest`](https://developer.apple.com/documentation/vision/recognizetextrequest)
for screenshots, infographics, and memes so text visible in the pixels can also influence subject
Tags. As an enhancement on macOS 27, the [Foundation Models image `Attachment`
API](https://developer.apple.com/documentation/foundationmodels/analyzing-images-with-multimodal-prompting)
can directly classify or describe visual content with guided generation. Apple introduced image
prompting in the macOS 27 generation, which [shipped on 14 September
2026](https://www.apple.com/uk/newsroom/2026/09/major-updates-for-apples-software-platforms-are-now-available/).
For videos, inspect the saved poster and a few representative frames; Apple's
[`AVAssetImageGenerator`](https://developer.apple.com/documentation/avfoundation/creating-images-from-a-video-asset)
can extract frames asynchronously from the local movie. The number and choice of frames need a
quality test; a poster alone can misrepresent the whole video. That last sentence is an Óia
design inference, not an Apple API guarantee.

Machine Tags should appear alongside user Tags with a distinct colour. Write both to the local
Markdown reading so the files remain authoritative; retain provenance so a user Tag wins a
case-insensitive duplicate. Removing any effective Tag needs a durable machine-tag suppression record in the
Markdown reading, or the next analysis pass will restore it. The exact frontmatter representation
is a library-format design decision. Keep raw Vision labels/confidences, OCR text, colour data,
model diagnostics, and Spotlight donations in the disposable per-device index. This boundary is
an Óia product and architecture decision, not a platform requirement.

## Capabilities and limits

| Capability | Minimum OS | Requirement | Role in Óia |
| --- | ---: | --- | --- |
| Foundation Models `.contentTagging` | macOS 26 | Eligible Apple Intelligence device, supported language and model available | Subject Tags from text |
| Vision image classification and OCR | macOS 15 current target | No Apple Intelligence requirement | Image and screenshot subjects |
| Foundation Models image prompting | macOS 27 | Eligible Apple Intelligence device, supported language and model available | Richer visual subjects and descriptions |
| Core Spotlight semantic search | macOS 15 | Local semantic resources available | Find related items without requiring an exact Tag |
| Create ML `MLTextClassifier` + `NLModel` | Supported on current target | Locally trained and bundled model | Optional fixed taxonomy after enough labelled examples |

The on-device Foundation Models path must check
[`SystemLanguageModel.availability`](https://developer.apple.com/documentation/foundationmodels/systemlanguagemodel)
at runtime. Apple documents `deviceNotEligible`, `appleIntelligenceNotEnabled`, and
`modelNotReady` cases. Its [Apple Intelligence requirements](https://support.apple.com/en-us/121115)
include a Mac with M1 or later (or the eligible MacBook Neo), matching supported device and Siri
languages, and downloaded models. Check [`supportsLocale(_:)`](https://developer.apple.com/documentation/foundationmodels/supporting-languages-and-locales-with-foundation-models)
for the model used; unsupported input can throw `unsupportedLanguageOrLocale`. The same APIs do
not make cloud calls when using the on-device system model. Do not select
`PrivateCloudComputeLanguageModel`, a server provider, or a remote fallback for this local-only
feature. The ordinary system model has no managed entitlement documented; Apple's custom adapter
and Private Cloud Compute entitlements are separate features, as detailed in the [image research
note](./image-content-search-research.md#availability-constraints).

Apple's [context-window guide](https://developer.apple.com/documentation/foundationmodels/managing-the-context-window)
documents 4,096 tokens per session and counts instructions, schema, prompt, and output; inspect
`SystemLanguageModel.contextSize` at runtime as Apple updates the model. Therefore,
do not feed an entire long article blindly. A practical Óia pipeline is to use title, source,
description, and selected substantive passages; split long articles into fresh sessions and
consolidate a small number of topic candidates. These passage-selection and consolidation steps
are proposed design choices. A URL-only lightweight link has little evidence; derive at most a
weak suggestion from its title or known metadata, and wait for a later full browser capture to
improve it.

Apple's [content-tagging guide](https://developer.apple.com/documentation/foundationmodels/categorizing-and-organizing-data-with-content-tags)
says the specialised model identifies broad topics but recommends the `general` use case for
constraints more complex than tag-count limits. For a fixed, controlled vocabulary, guided
generation with a constrained enum or an Óia-trained classifier may be easier to evaluate than
unrestricted generated strings. Apple's [Create ML text-classifier
guide](https://developer.apple.com/documentation/createml/creating-a-text-classifier-model)
requires labelled training examples and shows `NLModel` for predictions. Built-in
[`NLTagger`](https://developer.apple.com/documentation/naturallanguage/nltagger) provides word
classes and named entities such as people, places, and organisations; it is not a general
subject-matter classifier by itself.

## Search and processing

Keep SQLite FTS5 for exact text and Tag queries. For broader matching, [Core Spotlight
`CSUserQuery`](https://developer.apple.com/documentation/corespotlight/building-a-search-interface-for-your-app)
supports semantic as well as lexical search on macOS 15 and later. Apple's [WWDC24 Spotlight
session](https://developer.apple.com/videos/play/wwdc2024/10131/) says donated content is in a
private, entirely local index and can match similar meanings. It is an additional disposable
search signal, not the authority for Óia Tags.

Analyse after the reading and local assets are saved, off the UI thread, and recheck the reading's
content hash before committing Tag changes. Also scan on app launch or library reconciliation for
items whose analysis is missing or stale, because browser saves can arrive while the app is not
open and external processes can modify files. Version the analyser and prompt; Apple's
[`SystemLanguageModel` documentation](https://developer.apple.com/documentation/foundationmodels/systemlanguagemodel)
states that the system model changes with OS updates. Apple's
[`NSBackgroundActivityScheduler`](https://developer.apple.com/documentation/foundation/nsbackgroundactivityscheduler)
can schedule deferrable maintenance work, but its timing is system controlled. The queue,
content-hash check, and reconciliation are Óia design recommendations derived from its local-file
contract.
