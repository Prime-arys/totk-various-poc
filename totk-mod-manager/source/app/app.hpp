#pragma once

#include <string>
#include <vector>

#include <borealis/core/event.hpp>

#include "core/core.hpp"

// What the screens share: the mods and profiles as last read, and the two
// actions available everywhere (apply, launch the game).
namespace app
{

/** How far to push the console while merging. */
enum class Boost
{
    Off,
    /** The system's boost mode: CPU at 1785 MHz. */
    Standard,
    /** The same, plus sys-clk overrides for the CPU and the memory when
     *  sys-clk runs. */
    SysClk,
};

/** The manager's own preferences. */
struct Preferences
{
    Boost boost = Boost::SysClk;
    /** sys-clk targets: the highest listed frequency not above these. */
    int boostCpuMhz    = 1785;
    int boostMemoryMhz = 1600;
    /** Ask before merging mods that conflict. */
    bool warnConflicts = true;
    /** "auto" (the console's), "fr" or "en-US": read at start. */
    std::string language = "auto";
};

/** The state as last loaded. */
core::State& state();
const Preferences& preferences();
/** Reads sd:/totk/manager.ini again. */
void loadPreferences();

/** Where the manager's .nro was started from (to start it again). */
void setNroPath(const std::string& path);
const std::string& nroPath();

/** What reload() assumes about the merge on the SD card. */
enum class Merge
{
    /** Compare it with the profile (slow with large folder mods). */
    Check,
    /** It is current: a merge just finished. */
    Current,
    /** Something changed since. */
    Outdated,
    /** Nothing that counts for the merge changed (a boot-time setting). */
    Unchanged,
};

/** Reads the SD card again (making sure the active profile lists every mod)
 *  and tells the screens. */
void reload(Merge merge = Merge::Outdated);

/** Fired after reload(). */
brls::Event<>& changed();

/** Fired by moveMod(): the rows of the mods tab follow without being
 *  rebuilt (borealis may still point at one of them). */
brls::Event<>& orderChanged();

/** Merges the active profile now, with a progress dialog. */
void apply();

/** Starts TotK and leaves the manager. */
void launchGame();

/** Localized "Default". */
std::string defaultProfileName();

/** "Package TKMM", "Folder mod"... */
std::string kindLabel(const std::string& kind);

/** Installed mods in the active profile's order (winner first). */
std::vector<std::string> modOrder();

/** Puts a mod at `position` of modOrder() and saves the profile. Returns an
 *  error, or an empty string. */
std::string moveMod(const std::string& folder, size_t position);

/** The name a mod is shown under. */
std::string modName(const std::string& folder);

/** A conflict of the last merge as it stands now: the enabled mods it
 *  involves, winner first in the active profile's order. */
struct ActiveConflict
{
    core::Conflict conflict;
    std::vector<std::string> folders;
};

/** Conflicts still involving two enabled mods or more. */
std::vector<ActiveConflict> activeConflicts();

/** Those `folder` is part of. */
std::vector<ActiveConflict> conflictsOf(const std::string& folder);

/** The last path parts of a game file, short enough for a list row. */
std::string shortFileName(const std::string& file);

} // namespace app
