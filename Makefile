# AU 版 (自分の Mac 用)
#   make au           engine/ffi (Rust) を arm64 + x86_64 でビルドし、JUCE の殻と合わせて AU / VST3 / Standalone を作る
#   make au-install   ~/Library/Audio/Plug-Ins/ に入れ (アドホック署名)、auval で確かめる
#   make au-dist      配布用の zip を au/build/dist/ に作る (GitHub Release に添付するもの)
#   make au-clean
# バージョンは au-vX.Y.Z タグから入る。指定するなら make au VERSION=1.2.3
SHELL := /bin/bash
RUSTUP_BIN := $(shell brew --prefix rustup 2>/dev/null)/bin
export PATH := $(RUSTUP_BIN):$(PATH)
export MACOSX_DEPLOYMENT_TARGET := 11.0

BUILD := au/build
FFI := $(BUILD)/libstair_ffi.a
ART := $(BUILD)/StairOne_artefacts/Release
AU_DEST := $(HOME)/Library/Audio/Plug-Ins/Components
VST3_DEST := $(HOME)/Library/Audio/Plug-Ins/VST3
# タグ au-vX.Y.Z から。無ければ 0.0.0
VERSION ?= $(shell git describe --tags --match 'au-v*' --abbrev=0 2>/dev/null | sed 's/^au-v//' || true)
VERSION := $(if $(VERSION),$(VERSION),0.0.0)

.PHONY: au au-ffi au-sign au-install au-dist au-validate au-clean
DIST := $(BUILD)/dist

au-ffi:
	cd engine && for t in aarch64-apple-darwin x86_64-apple-darwin; do cargo build -q -p stair-ffi --release --target $$t; done
	mkdir -p $(BUILD)
	lipo -create -output $(FFI) engine/target/aarch64-apple-darwin/release/libstair_ffi.a engine/target/x86_64-apple-darwin/release/libstair_ffi.a

au: au-ffi
	cmake -S au -B $(BUILD) -DCMAKE_BUILD_TYPE=Release -DSTAIR_FFI_LIB=$(abspath $(FFI)) -DSTAIR_VERSION=$(VERSION)
	cmake --build $(BUILD) --config Release -j 8

# アドホック署名 (Apple Developer ID なし。自分の Mac 用)
au-sign: au
	codesign --force --deep -s - "$(ART)/AU/Stair One.component"
	codesign --force --deep -s - "$(ART)/VST3/Stair One.vst3"
	codesign --force --deep -s - "$(ART)/Standalone/Stair One.app"

au-install: au-sign
	mkdir -p $(AU_DEST) $(VST3_DEST)
	rm -rf "$(AU_DEST)/Stair One.component" "$(VST3_DEST)/Stair One.vst3"
	cp -R "$(ART)/AU/Stair One.component" $(AU_DEST)/
	cp -R "$(ART)/VST3/Stair One.vst3" $(VST3_DEST)/
	# AU の一覧を読み直させる
	-killall -9 AudioComponentRegistrar 2>/dev/null
	$(MAKE) au-validate

au-dist: au-sign
	rm -rf $(DIST) && mkdir -p $(DIST)
	cd "$(ART)/AU" && ditto -c -k --keepParent "Stair One.component" "$(abspath $(DIST))/StairOne-$(VERSION)-AU.zip"
	cd "$(ART)/VST3" && ditto -c -k --keepParent "Stair One.vst3" "$(abspath $(DIST))/StairOne-$(VERSION)-VST3.zip"
	cd "$(ART)/Standalone" && ditto -c -k --keepParent "Stair One.app" "$(abspath $(DIST))/StairOne-$(VERSION)-Standalone.zip"
	ls -la $(DIST)

au-validate:
	auval -v aumu Str1 Mrmt

au-clean:
	rm -rf $(BUILD)
