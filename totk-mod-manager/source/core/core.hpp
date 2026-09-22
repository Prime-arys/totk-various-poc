#pragma once

#include <cstdint>
#include <functional>
#include <map>
#include <string>
#include <vector>

// C++ side of the Rust core (core/include/totk_manager_core.h): the same
// data, parsed into plain structures. Calls are serialized: the core is not
// thread safe.
namespace core
{

using Selection = std::map<std::string, std::vector<std::string>>;

struct Mod
{
    std::string folder;
    std::string name;
    std::string kind; // "folder", "package" or "romfs"
    std::string path;
    std::string version;
    std::string author;
    std::string description;
    std::string url;
    std::string thumbnail; // sd:/ path, or empty
    bool enabled = true;
    int priority = 100;
    /** Skyline plugins the mod ships, loaded in the game by the merger. */
    int plugins      = 0;
    bool loadPlugins = true;
    /** Only code: nothing of this mod is merged. */
    bool codeOnly    = false;
};

struct ProfileEntry
{
    std::string folder;
    bool enabled = true;
    Selection options;
};

struct ConflictMod
{
    std::string folder;
    std::string name;
};

/** Mods changing the same thing (see totk-merge's conflicts module). */
struct Conflict
{
    std::string kind; // "file": a whole file; "values": values of a merged file
    std::string file;
    int count = 1;                  // values set differently
    std::vector<ConflictMod> mods;  // winner first, as merged
    std::vector<std::string> samples;

    bool wholeFile() const { return kind == "file"; }
};

struct State
{
    std::string profile;
    bool profileExists = false;
    bool mergerEnabled = true;
    bool mergeAtBoot   = true;
    bool applyPatches  = true;
    bool modPlugins    = true;
    int mergeCacheSize = 10;
    std::string locales;
    std::vector<std::string> profiles;
    std::vector<ProfileEntry> entries; // the active profile, winner first
    std::vector<Mod> mods;             // installed, by folder name
    std::vector<std::string> problems;
    /** The SD card's merge matches this profile (see isApplied). */
    bool applied = false;
    /** Found by the last merge (the manager's or the plugin's). */
    std::vector<Conflict> conflicts;

    const Mod* findMod(const std::string& folder) const;
    ProfileEntry* findEntry(const std::string& folder);
};

struct OptionGroup
{
    std::string name;
    std::string description;
    std::string type; // multi, multi_required, single, single_required
    std::vector<std::pair<std::string, std::string>> options; // name, description
    std::vector<int> defaults;

    bool single() const { return type.rfind("single", 0) == 0; }
    bool required() const { return type.find("required") != std::string::npos; }
};

struct ModDetails
{
    std::string folder;
    std::string kind;
    std::string name;
    std::string version;
    std::string author;
    std::string description;
    std::string url;
    std::string error;
    std::vector<OptionGroup> groups;
    Selection iniOptions;
    /** File names of the plugins the mod ships, and whether they are loaded. */
    std::vector<std::string> plugins;
    bool loadPlugins = true;
};

struct Candidate
{
    std::string kind;
    std::string path;
    std::string label;
    bool hasCode  = false;
    uint64_t size = 0;
};

struct InstallInfo
{
    std::string name;
    std::string version;
    std::string author;
    std::string description;
    std::string url;
    std::string thumbnail; // image file to move into the mod folder
};

struct RomInfo
{
    bool ok     = false;
    int version = 0;
    std::string nso;
    std::string error;
};

struct ApplyResult
{
    bool ok        = false;
    bool reused    = false;
    /** An older merge came back from the cache. */
    bool fromCache = false;
    bool cancelled = false;
    int conflicts  = 0;
    int mods      = 0;
    int files     = 0;
    int served    = 0;
    int warnings  = 0;
    int patches   = 0;
    double seconds = 0;
    std::string error;
};

using ProgressFn = std::function<void(int stage, uint32_t done, uint32_t total, const std::string& item)>;
/** Called from the merging thread; returns whether to merge anyway. */
using ConflictsFn = std::function<bool(const std::vector<Conflict>& conflicts)>;

/** sd:/ is "sdmc:/" here; log lines go to the manager's log. */
void init();

State loadState();
/** Whether the merge on the SD card is the active profile's as it is now.
 *  Lists the files of folder mods: slow, call it sparingly. */
bool isApplied();
ModDetails loadDetails(const std::string& folder);
/** Loads (or not) the plugins a mod ships. Empty on success. */
std::string setModPlugins(const std::string& folder, bool enabled);
std::vector<unsigned char> loadThumbnail(const std::string& folder);

bool isValidProfileName(const std::string& name);
/** Empty on success, otherwise what went wrong. */
std::string saveProfile(const std::string& name, const std::vector<ProfileEntry>& entries);
std::string activateProfile(const std::string& name);
std::string deleteProfile(const std::string& name);
std::string renameProfile(const std::string& from, const std::string& to);
std::string setConfig(const std::string& key, const std::string& value);
/** The manager's own preferences (sd:/totk/manager.ini). */
std::string managerSetting(const std::string& key, const std::string& fallback);
bool managerFlag(const std::string& key, bool fallback);
std::string setManagerSetting(const std::string& key, const std::string& value);
std::vector<Conflict> loadConflicts();

struct CacheUsage
{
    int merges     = 0;
    uint64_t bytes = 0;
};
/** Lists the merge store: call it off the interface thread. */
CacheUsage mergeCacheUsage();

/**
 * Makes sure there is an active profile listing every installed mod: creates
 * one from the mods' own settings the first time, and puts mods copied onto
 * the SD card since then at the top of it, enabled. Returns true when
 * something changed (the state is reloaded).
 */
bool normalizeProfile(State& state, const std::string& defaultName);

std::vector<Candidate> findMods(const std::string& dir);
std::string folderName(const std::string& name);
/** Returns the installed mod's path, or throws std::runtime_error. */
std::string install(const Candidate& candidate, const std::string& folder, const InstallInfo& info);
std::string uninstall(const std::string& folder);
std::vector<std::string> migrateOldLayout();
bool removeTree(const std::string& dir);

RomInfo romInfo(const std::string& prefix);
ApplyResult apply(const std::string& romPrefix, ProgressFn progress, ConflictsFn conflicts);

} // namespace core
