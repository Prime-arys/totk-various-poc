#include "totk/version.hpp"

#include <cstdlib>

#include "nn/oe.h"
#include "skyline/utils/cpputils.hpp"

namespace totk {

namespace {
    std::string g_VersionString = "unknown";
    u32 g_VersionCode = 0;
};  // namespace

void InitVersion() {
    nn::oe::DisplayVersion version = {};
    nn::oe::GetDisplayVersion(&version);

    version.name[sizeof(version.name) - 1] = '\0';
    if (version.name[0] == '\0') return;

    g_VersionString = version.name;

    // "major.minor.micro" -> major * 10000 + minor * 100 + micro
    u32 code = 0;
    const char* cursor = version.name;
    for (int part = 0; part < 3; part++) {
        char* end = nullptr;
        unsigned long value = strtoul(cursor, &end, 10);
        code = code * 100 + (u32)(value % 100);
        if (end == nullptr || *end != '.') {
            // Fewer than three components: pad the rest with zeroes.
            for (int remaining = part + 1; remaining < 3; remaining++) code *= 100;
            break;
        }
        cursor = end + 1;
    }
    g_VersionCode = code;
}

const std::string& GetVersionString() { return g_VersionString; }

u32 GetVersionCode() { return g_VersionCode; }

};  // namespace totk

extern "C" u32 totk_get_version() { return totk::GetVersionCode(); }

extern "C" const char* totk_get_version_string() { return totk::GetVersionString().c_str(); }

// The mount name the game passed to nn::fs::MountRom, e.g. "rom:/". Plugins
// need it to tell romfs paths apart from save/SD paths in nn::fs hooks.
extern "C" const char* totk_get_rom_mount() { return skyline::utils::g_RomMountStr.c_str(); }
