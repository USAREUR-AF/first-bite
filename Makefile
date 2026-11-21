# Developer tasks for the first-bite workspace. `make help` lists them.

# Network interface to capture on, and to replay onto.
IFACE ?= en1
# Environment file for `run` and `mock`, relative to mind/ (its paths are too).
ENV_FILE ?= env/default.local.env
# Capture file for `replay` and `mockplay`; `mockplay` defaults to the example.
PCAP ?=
RUST_LOG ?= warn,mind=debug,flow=debug
# Images the release binaries are built in, pinned by tag and digest; change
# both together. Ubuntu 20.04 sets the oldest glibc the standard binaries run
# on (2.31).
PORTABLE_IMAGE ?= ubuntu:20.04@sha256:8feb4d8ca5354def3d8fce243717141ce31e2c428701f6682bd2fafe15388214
STATIC_IMAGE ?= alpine:3.24@sha256:f59abe656b692d901e184be3445e084786602108599d2a51d229c0f5a48c71bf
PORTABLE_DIR = target/ubuntu-20.04/release
FIPS_DIR = target/ubuntu-20.04-fips/release
STATIC_DIR = target/alpine-3.24/$(shell uname -m | sed s/arm64/aarch64/)-unknown-linux-musl/release
STATIC_FIPS_DIR = target/alpine-3.24-fips/$(shell uname -m | sed s/arm64/aarch64/)-unknown-linux-musl/release
# Bare images the release binaries are run in after the build.
PORTABLE_RUNS_IN ?= ubuntu:20.04 ubuntu:22.04 ubuntu:24.04 ubuntu:26.04
STATIC_RUNS_IN ?= busybox:musl alpine:3.24 ubuntu:20.04
# Name the container image is built under, and its platform (default: this
# machine's), for `make image`.
IMAGE_NAME ?= first-bite:local
IMAGE_PLATFORM ?=
IMAGE_REVISION ?= $(shell git rev-parse HEAD 2>/dev/null || echo unknown)
# Targets the decoder library must compile for; `make decoder-targets`.
DECODER_TARGETS ?= aarch64-apple-ios aarch64-linux-android x86_64-pc-windows-msvc wasm32-unknown-unknown aarch64-unknown-linux-gnu

.DEFAULT_GOAL := help

.PHONY: help build release portable portable-check portable-dir release-features portable-fips portable-fips-check portable-fips-dir security-selftest static-fips static-fips-check static static-check static-dir image image-check decoder-targets test wasm fmt lint guards hooks deny audit vendor run mock replay mockplay echo blob bucket ports figures site brief clean

help: ## list the targets
	@awk -F':.*## ' '/^[a-z-]+:.*## /{printf "  %-10s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

build: ## debug build of every crate
	cargo build --workspace --locked

# Cargo features every released Linux and macOS mind carries beyond its
# defaults: the Zenoh sink, so a released binary can publish on a Zenoh fabric.
RELEASE_FEATURES ?= mind/zenoh

release: ## optimised mind and bite in target/release/, with the features the released archives carry
	cargo build --release --locked -p mind -p bite --features $(RELEASE_FEATURES)

portable: ## release binaries that need only glibc 2.31, built in Ubuntu 20.04 (needs docker)
	tools/build-in-container.sh $(PORTABLE_IMAGE) tools/build-portable.sh --features $(RELEASE_FEATURES)

portable-check: ## run the `portable` binaries in bare Ubuntu 20.04 to 26.04 containers
	tools/smoke-test.sh $(PORTABLE_DIR) $(PORTABLE_RUNS_IN)

release-features: # the cargo features of a released mind, for the release workflow
	@echo $(RELEASE_FEATURES)

portable-dir: # where `portable` leaves mind and bite
	@echo $(PORTABLE_DIR)

portable-fips: ## the `portable` binaries with the upstream FIPS module build of AWS-LC as crypto provider (needs docker)
	BUILD_VARIANT=fips tools/build-in-container.sh $(PORTABLE_IMAGE) tools/build-portable.sh --features mind/fips,$(RELEASE_FEATURES)

portable-fips-check: ## run the `portable-fips` binaries in bare Ubuntu containers; mind must report the FIPS module active
	SMOKE_EXPECT_FIPS=yes tools/smoke-test.sh $(FIPS_DIR) $(PORTABLE_RUNS_IN)

portable-fips-dir: # where `portable-fips` leaves mind and bite
	@echo $(FIPS_DIR)

static: ## fully static release binaries, built in Alpine (needs docker)
	tools/build-in-container.sh $(STATIC_IMAGE) tools/build-static.sh

static-check: ## run the `static` binaries in bare busybox, Alpine and Ubuntu containers
	tools/smoke-test.sh $(STATIC_DIR) $(STATIC_RUNS_IN)

static-dir: # where `static` leaves mind and bite
	@echo $(STATIC_DIR)

static-fips: ## the `static` binaries with the upstream FIPS module build of AWS-LC as crypto provider (needs docker)
	BUILD_VARIANT=fips tools/build-in-container.sh $(STATIC_IMAGE) tools/build-static.sh --features mind/fips

static-fips-check: ## run the `static-fips` binaries in bare containers; mind must report the FIPS module active
	SMOKE_EXPECT_FIPS=yes tools/smoke-test.sh $(STATIC_FIPS_DIR) $(STATIC_RUNS_IN)

# The build's own attestations are off: the release workflow attests the
# published image, and a single-platform image stays a plain manifest.
image: ## the container image of the Dockerfile, as IMAGE_NAME (needs docker)
	version=$$(tools/release-version.sh) && docker build \
		$(if $(IMAGE_PLATFORM),--platform $(IMAGE_PLATFORM)) \
		--provenance=false --sbom=false \
		$(if $(CARGO_BUILD_JOBS),--build-arg CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS)) \
		--build-arg VERSION=$$version --build-arg REVISION=$(IMAGE_REVISION) \
		--tag $(IMAGE_NAME) .

image-check: ## run the `image` container: usage, user, bite check, a replay, labels
	tools/image-check.sh $(IMAGE_NAME)

decoder-targets: ## check that the bite library compiles for each of DECODER_TARGETS
	rustup target add $(DECODER_TARGETS)
	@for target in $(DECODER_TARGETS); do \
		echo "cargo check -p bite --lib --no-default-features --target $$target"; \
		cargo check -p bite --lib --no-default-features --locked --target $$target || exit 1; \
	done

test: ## run every crate's tests
	cargo test --workspace --locked

# The wasm-bindgen command must be the version of the wasm-bindgen crate in
# Cargo.lock; the two halves of the binding are generated to match.
WASM_BINDGEN_VERSION = $(shell awk '/^name = "wasm-bindgen"$$/ { getline; gsub(/[^0-9.]/, ""); print; exit }' Cargo.lock)

# Built for size, since a browser downloads it. Stripping only debug info
# keeps the section wasm-bindgen reads the module's features from.
WASM_PROFILE = CARGO_PROFILE_RELEASE_OPT_LEVEL=s CARGO_PROFILE_RELEASE_LTO=fat \
	CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_STRIP=debuginfo

wasm: ## the decoder for browsers in site/pkg/, for the page in site/decoder/
	@wasm-bindgen --version 2>/dev/null | grep -qx 'wasm-bindgen $(WASM_BINDGEN_VERSION)' || { \
		echo "make wasm needs wasm-bindgen $(WASM_BINDGEN_VERSION) and the wasm32 target:"; \
		echo "  rustup target add wasm32-unknown-unknown"; \
		echo "  cargo install wasm-bindgen-cli --version $(WASM_BINDGEN_VERSION) --locked"; \
		exit 2; }
	$(WASM_PROFILE) cargo build --release --locked -p bite-wasm --target wasm32-unknown-unknown
	wasm-bindgen --target web --remove-name-section --remove-producers-section \
		--out-dir site/pkg target/wasm32-unknown-unknown/release/bite_wasm.wasm
	python3 tools/bundle-dictionaries.py

fmt: ## format the Rust sources
	cargo fmt --all

lint: ## what CI checks: rustfmt, clippy, workflow and shell lint, leak check
	cargo fmt --all --check
	cargo clippy --workspace --all-targets --locked -- -D warnings
	actionlint
	yamllint .github
	shellcheck tools/*.sh tools/git-hooks/pre-commit deployment/*.sh deployment/*/*.sh mocks/*/*.sh
	$(MAKE) guards

# The files that are published: everything tracked except private/ and the
# two internal documents, which exist only in the maintainers' repository.
guards: ## leak check of the published files, pin check, both self-tests, generated packs current
	git ls-files -z -- . ':!private' ':!docs/plan.md' ':!docs/decisions.md' | xargs -0 tools/check-public-tree.sh
	tools/check-public-tree.test.sh
	python3 tools/check-pins.py
	python3 tools/check-pins.py --self-test
	tools/bite2pack/refresh.sh --check

hooks: ## install the pre-commit hook that runs the leak check on staged files
	git config core.hooksPath tools/git-hooks
	@echo "pre-commit hook installed (tools/git-hooks/pre-commit)"

deny: ## licence, ban and source policy (needs cargo-deny)
	cargo deny --locked check licenses bans sources

audit: ## RustSec advisories (needs cargo-audit)
	cargo audit

security-selftest: ## prove the transport security levels with local servers and throwaway certificates (needs docker, OpenSSL 3.5)
	tools/security-selftest.sh

vendor: ## copy every dependency into vendor/ for an air-gapped build
	@cargo vendor --locked --versioned-dirs vendor
	@echo
	@echo "Put the lines above in .cargo/config.toml on the machine that builds offline,"
	@echo "then build there with: cargo build --release --locked --offline"

# The binary is built as the invoking user and only the capture runs as root,
# so target/ stays writable without sudo.
run: ## capture on IFACE and publish (sudo for the capture)
	cargo build --locked -p mind
	cd mind && sudo -E RUST_LOG=$(RUST_LOG) ../target/debug/mind $(IFACE) --env-file $(ENV_FILE) --output --activate-subpubs

mock: ## read frames from UDP 127.0.0.1:12345 instead of an interface
	cargo build --locked -p mind
	cd mind && RUST_LOG=$(RUST_LOG) ../target/debug/mind --mock --env-file $(ENV_FILE)

replay: ## replay PCAP onto IFACE for `make run` (needs tcpreplay)
	@test -n "$(PCAP)" || { echo "usage: make replay PCAP=<capture file> [IFACE=$(IFACE)]"; exit 2; }
	sudo tcpreplay -i $(IFACE) $(PCAP)

mockplay: ## send PCAP (default: the example capture) to `make mock`
	python3 tools/mockplay.py $(or $(PCAP),examples/weather-station.pcap)

echo: ## REST stand-in on port 5001: prints every POST
	python3 mocks/echo/echo.py

blob: ## Azure Blob stand-in on port 10000 (needs docker compose)
	docker compose -f mocks/azure/compose.yml up

bucket: ## S3 stand-in on port 9090 (needs docker compose)
	docker compose -f mocks/s3/compose.yml up

ports: ## show what is listening on the ports the mocks and mind use
	@for port in 5001 9090 10000 12000 12001 12002; do \
		lsof -nP -iTCP:$$port -sTCP:LISTEN || echo "nothing listening on $$port"; \
	done

figures: ## write the illustrations in docs/img (light and dark SVG)
	python3 docs/img/build.py

site: ## build the documentation site into site/_build
	python3 docs/img/build.py --check
	python3 tools/build-site.py

brief: ## print docs/brief/brief.html to docs/brief/first-bite-brief.pdf (needs Chrome or Chromium)
	python3 docs/brief/build.py

clean: ## remove build output and Python caches
	cargo clean
	find . -type d -name __pycache__ -prune -exec rm -rf {} +
	find . -type f -name '*.pyc' -delete
