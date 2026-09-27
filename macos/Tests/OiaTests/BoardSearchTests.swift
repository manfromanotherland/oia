// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

final class BoardSearchTests: XCTestCase {
    func testTokenPreservesExactTagsAndNormalizesVisualValues() throws {
        let tag = BoardSearchToken(kind: .tag, value: "  blue\n")
        let plainTag = BoardSearchToken(kind: .tag, value: "blue")
        let visual = BoardSearchToken(kind: .visual, value: "blue")

        XCTAssertEqual(tag.value, "  blue\n")
        XCTAssertEqual(tag.displayValue, "blue")
        XCTAssertNotEqual(tag.id, plainTag.id)
        XCTAssertNotEqual(tag.id, visual.id)

        let encoded = try JSONEncoder().encode(tag)
        XCTAssertEqual(try JSONDecoder().decode(BoardSearchToken.self, from: encoded), tag)
    }

    func testColorTokenKeepsOnlyTheHexAndComposesWithOtherTerms() {
        let color = BoardSearchToken(kind: .color, value: "colour:#42c878")
        let criteria = BoardSearchCriteria(tokens: [
            color,
            BoardSearchToken(kind: .visual, value: "furniture"),
            BoardSearchToken(kind: .tag, value: "interiors")
        ])

        XCTAssertEqual(color.displayValue, "#42C878")
        XCTAssertEqual(criteria.colorTerms, ["#42C878"])
        XCTAssertEqual(criteria.visualTerms, ["furniture"])
        XCTAssertEqual(criteria.tagTerms, ["interiors"])
        XCTAssertTrue(BoardSearchCriteria(tokens: [
            BoardSearchToken(kind: .color, value: "#42c878"), color
        ]).tokens.count == 1)
    }

    func testCriteriaDropsEmptyAndDuplicateTermsButKeepsKindsDistinct() {
        let criteria = BoardSearchCriteria(tokens: [
            BoardSearchToken(kind: .visual, value: " furniture "),
            BoardSearchToken(kind: .tag, value: "blue"),
            BoardSearchToken(kind: .tag, value: "blue"),
            BoardSearchToken(kind: .visual, value: "blue"),
            BoardSearchToken(kind: .visual, value: "  \n")
        ])

        XCTAssertEqual(criteria.tagTerms, ["blue"])
        XCTAssertEqual(criteria.visualTerms, ["furniture", "blue"])
        XCTAssertTrue(criteria.isActive)
        XCTAssertTrue(criteria.hasVisualTerms)
        XCTAssertEqual(criteria.tokens.count, 3)
    }

    func testCriteriaSemanticIdentityIgnoresAndTermOrder() {
        let tag = BoardSearchToken(kind: .tag, value: "design")
        let visual = BoardSearchToken(kind: .visual, value: "chair")

        XCTAssertEqual(
            BoardSearchCriteria(tokens: [tag, visual]),
            BoardSearchCriteria(tokens: [visual, tag])
        )
    }

    func testCriteriaReplacesEarlierItemTypeWhileKeepingOtherTokens() {
        let criteria = BoardSearchCriteria(tokens: [
            BoardSearchToken(kind: .itemType, value: "image"),
            BoardSearchToken(kind: .tag, value: "cinema"),
            BoardSearchToken(kind: .itemType, value: "video")
        ])

        XCTAssertEqual(criteria.tokens.map(\.kind), [.tag, .itemType])
        XCTAssertEqual(criteria.tagTerms, ["cinema"])
        XCTAssertEqual(criteria.itemTypeTerms, ["video"])
    }

    func testSearchInputNormalizesTextAndIncludesStructuredTermsInActivity() {
        let empty = BoardSearchInput(text: " \n", tokens: [])
        let structured = BoardSearchInput(
            text: "  ",
            tokens: [BoardSearchToken(kind: .visual, value: "blue furniture")]
        )
        let text = BoardSearchInput(text: "  Dieter Rams  ", tokens: [])

        XCTAssertNil(empty.text)
        XCTAssertFalse(empty.isActive)
        XCTAssertFalse(empty.hasVisualTerms)
        XCTAssertNil(structured.text)
        XCTAssertTrue(structured.isActive)
        XCTAssertTrue(structured.hasVisualTerms)
        XCTAssertEqual(text.text, "Dieter Rams")
        XCTAssertTrue(text.isActive)
    }

    func testTypedItemTypeAndColorComposeWithFreeText() {
        let input = BoardSearchInput(text: "image #ff0000 modern", tokens: [])
        XCTAssertEqual(input.criteria.itemTypeTerms, ["image"])
        XCTAssertEqual(input.criteria.colorTerms, ["#FF0000"])
        XCTAssertEqual(input.text, "modern")
    }

    func testLastTypedItemTypeOverridesSelectedAndEarlierTypedTypes() {
        let input = BoardSearchInput(
            text: "article image video modern",
            tokens: [BoardSearchToken(kind: .itemType, value: "quote")]
        )
        XCTAssertEqual(input.criteria.itemTypeTerms, ["video"])
        XCTAssertEqual(input.text, "modern")
    }

    func testSubmittingHexColorsCreatesTokensAndPreservesOtherTerms() {
        let completion = BoardSearchTermCompletion(
            text: "image #ff0000 modern #00ff00",
            tokens: [BoardSearchToken(kind: .tag, value: "chairs")]
        )

        XCTAssertTrue(completion.didComplete)
        XCTAssertEqual(completion.text, "modern")
        XCTAssertEqual(completion.tokens.map(\.kind), [.tag, .itemType, .color, .color])
        XCTAssertEqual(completion.tokens.map(\.value), ["chairs", "image", "#FF0000", "#00FF00"])
        XCTAssertEqual(
            BoardSearchInput(text: completion.text, tokens: completion.tokens).criteria.itemTypeTerms,
            ["image"]
        )
    }

    func testSubmittingDuplicateOrInvalidHexDoesNotAddExtraTokens() {
        let existing = [BoardSearchToken(kind: .color, value: "#FF0000")]
        let duplicate = BoardSearchTermCompletion(text: "#ff0000 chair", tokens: existing)
        XCTAssertTrue(duplicate.didComplete)
        XCTAssertEqual(duplicate.text, "chair")
        XCTAssertEqual(duplicate.tokens, existing)

        let invalid = BoardSearchTermCompletion(text: "chair  #ff00  ", tokens: existing)
        XCTAssertFalse(invalid.didComplete)
        XCTAssertEqual(invalid.text, "chair  #ff00  ")
        XCTAssertEqual(invalid.tokens, existing)
    }

    func testSubmittingNewItemTypeReplacesExistingTypeToken() {
        let completion = BoardSearchTermCompletion(
            text: "video cinema",
            tokens: [
                BoardSearchToken(kind: .itemType, value: "image"),
                BoardSearchToken(kind: .tag, value: "film")
            ]
        )

        XCTAssertTrue(completion.didComplete)
        XCTAssertEqual(completion.text, "cinema")
        XCTAssertEqual(completion.tokens.map(\.kind), [.tag, .itemType])
        XCTAssertEqual(completion.tokens.map(\.value), ["film", "video"])
    }

    func testItemTypesAreDefaultSuggestionsAndSelectedTypeIsExcluded() {
        let initial = BoardSearchSuggestions(text: "", tagCandidates: [], selectedTokens: [])
        XCTAssertEqual(initial.itemTypeTokens.map(\.value), ["image", "video", "article", "link", "quote"])
        let selected = BoardSearchSuggestions(
            text: "im", tagCandidates: [],
            selectedTokens: [BoardSearchToken(kind: .itemType, value: "image")]
        )
        XCTAssertTrue(selected.itemTypeTokens.isEmpty)
    }

    func testCompleteHexIsSuggestedAsAColorToken() {
        let suggestions = BoardSearchSuggestions(text: "#ff0000", tagCandidates: [], selectedTokens: [])
        XCTAssertEqual(suggestions.colorToken, BoardSearchToken(kind: .color, value: "#FF0000"))
        XCTAssertNil(BoardSearchSuggestions(text: "#ff00", tagCandidates: [], selectedTokens: []).colorToken)
        XCTAssertNil(BoardSearchSuggestions(
            text: "#ff0000",
            tagCandidates: [],
            selectedTokens: [BoardSearchToken(kind: .color, value: "#FF0000")]
        ).colorToken)
    }

    func testTagSuggestionsMatchCaseAndDiacriticsWithPrefixesFirst() {
        let suggestions = BoardSearchSuggestions(
            text: " BOOK ",
            tagCandidates: candidates(["notebook", "Résumé", "bookcase", "book"]),
            selectedTokens: []
        )
        let accentMatch = BoardSearchSuggestions(
            text: "resume",
            tagCandidates: candidates(["notes", "Résumé"]),
            selectedTokens: []
        )

        XCTAssertEqual(suggestions.tagTokens.map(\.value), ["bookcase", "book", "notebook"])
        XCTAssertEqual(accentMatch.tagTokens.map(\.value), ["Résumé"])
    }

    func testTagSuggestionsAreBoundedAndComeFromAvailableTags() {
        let tags = (0 ..< 20).map { "blue-\($0)" }
        let suggestions = BoardSearchSuggestions(
            text: "blue",
            tagCandidates: candidates(tags),
            selectedTokens: []
        )

        XCTAssertEqual(suggestions.tagTokens.count, BoardSearchSuggestions.maximumTagCount)
        XCTAssertEqual(suggestions.tagTokens.map(\.value), Array(tags.prefix(8)))
        XCTAssertTrue(suggestions.tagTokens.allSatisfy { tags.contains($0.value) })
    }

    func testSelectedTokenIsExcludedOnlyFromItsOwnKind() {
        let selectedVisual = BoardSearchToken(kind: .visual, value: "blue")
        let withVisualSelected = BoardSearchSuggestions(
            text: " blue ",
            tagCandidates: candidates(["blue"]),
            selectedTokens: [selectedVisual]
        )

        XCTAssertEqual(withVisualSelected.tagTokens.map(\.value), ["blue"])
        XCTAssertNil(withVisualSelected.visualToken)

        let selectedTag = BoardSearchToken(kind: .tag, value: "blue")
        let withTagSelected = BoardSearchSuggestions(
            text: "blue",
            tagCandidates: candidates(["blue"]),
            selectedTokens: [selectedTag]
        )

        XCTAssertTrue(withTagSelected.tagTokens.isEmpty)
        XCTAssertEqual(withTagSelected.visualToken, selectedVisual)
    }

    func testVisualSuggestionPreservesTheFullTrimmedMultiwordText() {
        let suggestions = BoardSearchSuggestions(
            text: "  blue furniture  ",
            tagCandidates: candidates(["blue", "furniture"]),
            selectedTokens: []
        )

        XCTAssertEqual(suggestions.visualToken?.value, "blue furniture")
        XCTAssertTrue(suggestions.tagTokens.isEmpty)
    }

    func testEmptyTextHasNoSuggestions() {
        let suggestions = BoardSearchSuggestions(
            text: " \n ",
            tagCandidates: candidates(["blue"]),
            selectedTokens: []
        )

        XCTAssertTrue(suggestions.tagTokens.isEmpty)
        XCTAssertNil(suggestions.visualToken)
    }

    func testUnavailableVisualCompletionIsNotOffered() {
        let suggestions = BoardSearchSuggestions(
            text: "typography",
            tagCandidates: [],
            selectedTokens: [],
            includeVisualToken: false
        )

        XCTAssertNil(suggestions.visualToken)
    }

    func testTagSuggestionsPreserveExactStoredUnicodeForCoreMatching() {
        let decomposed = "Cafe\u{301}"
        let suggestions = BoardSearchSuggestions(
            text: "café",
            tagCandidates: candidates([decomposed]),
            selectedTokens: []
        )

        XCTAssertEqual(suggestions.tagTokens.map(\.value), [decomposed])
        XCTAssertEqual(suggestions.tagTokens.first?.displayValue, "Café")
    }

    func testCanonicallyEquivalentTagsKeepDistinctByteExactIdentity() {
        let precomposed = "Café"
        let decomposed = "Cafe\u{301}"
        let precomposedToken = BoardSearchToken(kind: .tag, value: precomposed)
        let decomposedToken = BoardSearchToken(kind: .tag, value: decomposed)

        XCTAssertFalse(ExactTagIdentity.matches(precomposed, decomposed))
        XCTAssertNotEqual(precomposedToken.id, decomposedToken.id)

        let criteria = BoardSearchCriteria(tokens: [precomposedToken, decomposedToken])
        XCTAssertEqual(criteria.tagTerms.count, 2)
        XCTAssertEqual(
            criteria.tagTerms.map { Array($0.utf8) },
            [Array(precomposed.utf8), Array(decomposed.utf8)]
        )

        let filters = LibraryFilters(tags: [
            TagCount(tag: precomposed, count: 1),
            TagCount(tag: decomposed, count: 1)
        ])
        XCTAssertEqual(filters.searchTagCandidates.count, 2)
    }

    private func candidates(_ values: [String]) -> [BoardSearchTagCandidate] {
        values.compactMap(BoardSearchTagCandidate.init(value:))
    }
}
