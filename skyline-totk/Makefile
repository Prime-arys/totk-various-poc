.PHONY: all clean package npdm nso install dump-npdm

# ----------------------------------------------------------------------------
# skyline-totk - Skyline adapted to The Legend of Zelda: Tears of the Kingdom
# ----------------------------------------------------------------------------

TITLE_ID      := 0100F2C0115B6000

PYTHON        := python3
ifeq (, $(shell which python3 2>/dev/null))
	PYTHON := python
endif

NAME          := skyline-totk
MAKE_NSO      := nso.mk

# Nothing lands inside the source folder unless asked: OUT_ROOT holds the
# objects, the .nso and the packaged exefs. The repository points it at
# output/skyline (utils/build-skyline.sh); on its own, this Makefile keeps the
# usual build/ and out/ next to the sources.
OUT_ROOT      ?= .
BUILD_DIR     ?= $(OUT_ROOT)/build
NSO           := $(OUT_ROOT)/$(NAME).nso

NPDM_JSON     := npdm/totk.json
OUT_DIR       := $(OUT_ROOT)/out/atmosphere/contents/$(TITLE_ID)
EXEFS_DIR     := $(OUT_DIR)/exefs
# Outside the title's romfs folder: any file under atmosphere/contents/<tid>/romfs
# makes Atmosphère build a LayeredFS romfs at boot, which is slow for TotK.
PLUGIN_DIR    := $(OUT_DIR)/skyline/plugins

# Path to the game's own main.npdm, only needed to regenerate npdm/totk.json
# (see `make dump-npdm` / scripts/npdm2json.py).
GAME_NPDM     ?=

all: package

nso:
	@mkdir -p $(OUT_ROOT)
	$(MAKE) all -f $(MAKE_NSO) MAKE_NSO=$(MAKE_NSO) BUILD=$(abspath $(BUILD_DIR)) \
		TARGET=$(NAME) OUTPUT=$(abspath $(OUT_ROOT))/$(NAME)

npdm: $(EXEFS_DIR)/main.npdm

$(EXEFS_DIR)/main.npdm: $(NPDM_JSON)
	@mkdir -p $(EXEFS_DIR)
	npdmtool $(NPDM_JSON) $@

package: nso npdm
	@mkdir -p $(EXEFS_DIR) $(PLUGIN_DIR)
	@if [ -d $(OUT_DIR)/romfs/skyline/plugins ]; then mv $(OUT_DIR)/romfs/skyline/plugins/*.nro $(PLUGIN_DIR)/ 2>/dev/null || true; rm -rf $(OUT_DIR)/romfs; fi
	@cp $(NSO) $(EXEFS_DIR)/subsdk9
	@echo "Packaged into $(OUT_DIR)"
	@echo "  exefs/subsdk9      $$(stat -c %s $(EXEFS_DIR)/subsdk9) bytes"
	@echo "  exefs/main.npdm    $$(stat -c %s $(EXEFS_DIR)/main.npdm) bytes"
	@echo "Copy the '$(OUT_ROOT)/out/atmosphere' folder to the root of your SD card."

# Regenerate the npdm descriptor from the game's own main.npdm.
#   make dump-npdm GAME_NPDM=/path/to/exefs/main.npdm
dump-npdm:
	@test -n "$(GAME_NPDM)" || (echo "usage: make dump-npdm GAME_NPDM=<path to game main.npdm>"; exit 1)
	$(PYTHON) scripts/npdm2json.py "$(GAME_NPDM)" $(NPDM_JSON) --full-perms

# Copy the built package onto a mounted SD card: make install SD=/d
install: package
	@test -n "$(SD)" || (echo "usage: make install SD=<sd card mount point>"; exit 1)
	@mkdir -p "$(SD)/atmosphere/contents/$(TITLE_ID)/exefs"
	@cp $(EXEFS_DIR)/subsdk9 "$(SD)/atmosphere/contents/$(TITLE_ID)/exefs/subsdk9"
	@cp $(EXEFS_DIR)/main.npdm "$(SD)/atmosphere/contents/$(TITLE_ID)/exefs/main.npdm"
	@mkdir -p "$(SD)/atmosphere/contents/$(TITLE_ID)/skyline/plugins"
	@echo "Installed to $(SD)"

clean:
	$(MAKE) clean -f $(MAKE_NSO) BUILD=$(abspath $(BUILD_DIR)) \
		TARGET=$(NAME) OUTPUT=$(abspath $(OUT_ROOT))/$(NAME)
	@rm -rf $(OUT_ROOT)/out
