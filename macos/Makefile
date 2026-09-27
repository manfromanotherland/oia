# Build helpers for the Óia macOS app.
# Run these on macOS from the macos/ directory.
#
# Prerequisites:
#   brew install xcodegen
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin

CORE_DIR := ../core
FRAMEWORKS_DIR := Frameworks
BINDINGS_DIR := GeneratedBindings
DMG := dist/Óia.dmg

# Path to Sparkle's `sign_update` tool. Left empty so it's auto-discovered from
# the SPM artifact at run time; override on the command line if it lives
# elsewhere: `make sparkle-sign SIGN_UPDATE=/path/to/sign_update`.
SIGN_UPDATE ?=

.PHONY: all xcframework bindings xcodegen test dmg release sparkle-sign clean format format-check lint lint-fix

all: xcframework bindings xcodegen

## Build the XCFramework and copy it into Frameworks/
xcframework:
	cd $(CORE_DIR) && ./scripts/build-xcframework.sh --release
	mkdir -p $(FRAMEWORKS_DIR)
	rm -rf $(FRAMEWORKS_DIR)/OiaCore.xcframework
	cp -R $(CORE_DIR)/dist/OiaCore.xcframework $(FRAMEWORKS_DIR)/

## Copy generated Swift bindings into GeneratedBindings/
bindings: xcframework
	rm -rf $(BINDINGS_DIR)
	mkdir -p $(BINDINGS_DIR)
	cp $(CORE_DIR)/dist/swift/*.swift $(BINDINGS_DIR)/
	cp $(CORE_DIR)/dist/swift/*.h $(BINDINGS_DIR)/
	cp $(CORE_DIR)/dist/swift/*.modulemap $(BINDINGS_DIR)/

## Regenerate the Xcode project from project.yml
xcodegen:
	xcodegen generate

## Run the test suites. The scheme runs the fast, hostless unit tests
## (OiaTests) before the UI suite, so logic regressions surface first.
## Assumes `make all` has generated the framework, bindings, and project.
test:
	xcodebuild test -project Oia.xcodeproj -scheme Oia -derivedDataPath build

## Build a Release .app and wrap it in a distributable .dmg (dist/Óia.dmg).
## Ad-hoc signed only (local testing) — for a shippable build use `make release`.
dmg: all
	./scripts/package-dmg.sh

## Build a SHIPPABLE .dmg: Developer ID signed -> notarized -> stapled, then
## Sparkle-signed. Requires a Developer ID cert and a notarytool profile, passed
## via env (nothing sensitive is stored in the repo):
##   SIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)" \
##   NOTARY_PROFILE=oia-notary make release
## sparkle-sign runs last, on the final stapled .dmg (stapling rewrites the .dmg,
## so the EdDSA signature must be taken after it). See README "Software updates".
release: all
	./scripts/release-dmg.sh
	$(MAKE) sparkle-sign

## Sign the built .dmg with your Sparkle EdDSA key (read from your login Keychain)
## and print the `sparkle:edSignature` + `length` attributes to paste into the
## appcast <enclosure>. Run `make dmg` first, and generate a key once — see the
## README "Software updates (Sparkle)". Auto-finds sign_update in the SPM
## artifact; override with `SIGN_UPDATE=/path/to/sign_update` if needed.
sparkle-sign:
	@test -f "$(DMG)" || { echo "error: $(DMG) not found — run 'make dmg' first" >&2; exit 1; }
	@tool="$(SIGN_UPDATE)"; \
	if [ -z "$$tool" ]; then \
	  tool=$$(find "$$HOME/Library/Developer/Xcode/DerivedData" -name sign_update -path '*Sparkle*' 2>/dev/null | head -1); \
	fi; \
	if [ -z "$$tool" ] || [ ! -x "$$tool" ]; then \
	  echo "error: sign_update not found. Build once so SPM fetches Sparkle, or pass" >&2; \
	  echo "       SIGN_UPDATE=/path/to/sign_update (see README)." >&2; \
	  exit 1; \
	fi; \
	echo "==> Signing $(DMG) with your Sparkle key"; \
	"$$tool" "$(DMG)"

## Reformat Swift sources in place (config: .swiftformat). Run this before `lint`;
## SwiftFormat is configured to agree with SwiftLint, so it won't create new
## SwiftLint violations.
format:
	swiftformat .

## Verify formatting without editing — fails if anything is unformatted (CI/hooks).
format-check:
	swiftformat --lint .

## Lint Swift sources with SwiftLint (config: .swiftlint.yml).
lint:
	swiftlint lint

## Auto-fix the SwiftLint violations that are safe to correct in place.
lint-fix:
	swiftlint --fix

clean:
	rm -rf $(FRAMEWORKS_DIR) $(BINDINGS_DIR) Oia.xcodeproj build dist
