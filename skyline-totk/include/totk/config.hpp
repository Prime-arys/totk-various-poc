#pragma once

#include <string>

#include "types.h"

namespace totk {

// Runtime configuration, read from sd:/skyline/totk/config.ini at boot.
// Every field has a sane default, so the file is entirely optional.
struct Config {
    static constexpr const char* Path = "sd:/skyline/totk/config.ini";

    // Log sinks (bit flags), "log = kernel,sd,tcp" or "log = none".
    enum LogSink : u32 {
        LogNone = 0,
        LogKernel = 1 << 0,  // svcOutputDebugString, visible to emulators/debuggers
        LogSd = 1 << 1,      // appended to log_path on the SD card
        LogTcp = 1 << 2,     // served on tcp_port, e.g. for `cargo skyline listen`
    };

    u32 log_sinks = LogSd;
    std::string log_path = "sd:/skyline/totk/skyline.log";
    u16 tcp_port = 6969;
    bool load_plugins = true;

    // Plugins are looked up here first, then in romfs:/skyline/plugins. This
    // folder is outside the title's romfs directory on purpose: as soon as
    // atmosphere/contents/<tid>/romfs exists, Atmosphère builds a layered romfs
    // over TotK's ~300 000 entries at every boot, which costs seconds and
    // enough fs.mitm memory to stop the game from starting on firmware 20+.
    std::string plugins_dir = "sd:/atmosphere/contents/0100F2C0115B6000/skyline/plugins";

    bool WantsTcp() const { return (log_sinks & LogTcp) != 0; }
};

extern Config g_Config;

// Reads the config file off the SD card (which must already be mounted).
// Missing or malformed files leave the defaults in place.
void LoadConfig();

};  // namespace totk
