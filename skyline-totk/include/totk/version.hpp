#pragma once

#include <string>

#include "types.h"

namespace totk {

// The game's own version string ("1.2.1", "1.1.2", ...), read once from the
// applet's display version and cached. Plugins use it to pick the right
// offsets/patterns for the running build.
const std::string& GetVersionString();

// Same thing packed for easy comparison: "1.2.1" -> 10201.
u32 GetVersionCode();

// Queries and caches the version. Safe to call once the applet is up, i.e.
// any time after romfs has been mounted.
void InitVersion();

};  // namespace totk

#ifdef __cplusplus
extern "C" {
#endif

// Plugin-facing ABI (exported from the subsdk, see exported.txt).
u32 totk_get_version();
const char* totk_get_version_string();

// Mount prefix of the game's romfs, e.g. "rom:/".
const char* totk_get_rom_mount();

#ifdef __cplusplus
}
#endif
