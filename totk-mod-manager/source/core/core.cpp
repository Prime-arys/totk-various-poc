#include "core/core.hpp"

#include <algorithm>
#include <mutex>
#include <stdexcept>
#include <strings.h>

#include <borealis/extern/nlohmann/json.hpp>

#include "totk_manager_core.h"
#include "util/log.hpp"

using json = nlohmann::json;

namespace core
{

namespace
{
    std::recursive_mutex coreLock;

    /** Takes ownership of text returned by the core. */
    std::string take(char* text)
    {
        if (!text)
            return "";
        std::string result(text);
        tkmc_free(text);
        return result;
    }

    json parse(char* text)
    {
        std::string raw = take(text);
        json parsed = json::parse(raw, nullptr, false);
        if (parsed.is_discarded())
        {
            applog::write("core returned invalid JSON: " + raw.substr(0, 200));
            return json::object();
        }
        return parsed;
    }

    std::string str(const json& object, const char* key)
    {
        auto found = object.find(key);
        return found != object.end() && found->is_string() ? found->get<std::string>() : std::string();
    }

    bool flag(const json& object, const char* key, bool fallback = false)
    {
        auto found = object.find(key);
        return found != object.end() && found->is_boolean() ? found->get<bool>() : fallback;
    }

    int64_t number(const json& object, const char* key)
    {
        auto found = object.find(key);
        return found != object.end() && found->is_number() ? found->get<int64_t>() : 0;
    }

    Selection selection(const json& object)
    {
        Selection result;
        if (!object.is_object())
            return result;
        for (auto& [group, options] : object.items())
        {
            auto& selected = result[group];
            if (options.is_array())
                for (auto& option : options)
                    if (option.is_string())
                        selected.push_back(option.get<std::string>());
        }
        return result;
    }

    void logLine(const char* line, void*)
    {
        applog::write(std::string("core: ") + line);
    }

    struct ApplyContext
    {
        ProgressFn* progress;
        ConflictsFn* conflicts;
    };

    void progressTrampoline(int stage, uint32_t done, uint32_t total, const char* item, void* user)
    {
        auto* context = (ApplyContext*)user;
        if (context && context->progress && *context->progress)
            (*context->progress)(stage, done, total, item ? item : "");
    }

    std::vector<Conflict> conflictList(const json& root)
    {
        std::vector<Conflict> conflicts;
        for (auto& entry : root.value("conflicts", json::array()))
        {
            Conflict conflict;
            conflict.kind  = str(entry, "kind");
            conflict.file  = str(entry, "file");
            conflict.count = (int)number(entry, "count");
            for (auto& mod : entry.value("mods", json::array()))
                conflict.mods.push_back({ str(mod, "folder"), str(mod, "name") });
            for (auto& sample : entry.value("samples", json::array()))
                if (sample.is_string())
                    conflict.samples.push_back(sample.get<std::string>());
            conflicts.push_back(std::move(conflict));
        }
        return conflicts;
    }

    bool conflictsTrampoline(const char* text, void* user)
    {
        auto* context = (ApplyContext*)user;
        if (!context || !context->conflicts || !*context->conflicts)
            return true;
        json root = json::parse(text ? text : "", nullptr, false);
        if (root.is_discarded())
            return true;
        return (*context->conflicts)(conflictList(root));
    }
} // namespace

const Mod* State::findMod(const std::string& folder) const
{
    for (auto& mod : mods)
        if (strcasecmp(mod.folder.c_str(), folder.c_str()) == 0)
            return &mod;
    return nullptr;
}

ProfileEntry* State::findEntry(const std::string& folder)
{
    for (auto& entry : entries)
        if (strcasecmp(entry.folder.c_str(), folder.c_str()) == 0)
            return &entry;
    return nullptr;
}

void init()
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    tkmc_init("sdmc:/", logLine, nullptr);
}

State loadState()
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    json root = parse(tkmc_state());
    State state;

    const json& config  = root.value("config", json::object());
    state.profile       = str(config, "profile");
    state.mergerEnabled = flag(config, "enabled", true);
    state.mergeAtBoot   = flag(config, "merge_at_boot", true);
    state.applyPatches  = flag(config, "apply_patches", true);
    state.mergeCacheSize = (int)number(config, "merge_cache_size");
    state.modPlugins     = flag(config, "mod_plugins", true);
    state.locales       = str(config, "locales");

    for (auto& name : root.value("profiles", json::array()))
        if (name.is_string())
            state.profiles.push_back(name.get<std::string>());

    const json& profile = root.value("profile", json::object());
    state.profileExists = flag(profile, "exists");
    for (auto& entry : profile.value("mods", json::array()))
    {
        ProfileEntry parsed;
        parsed.folder  = str(entry, "folder");
        parsed.enabled = flag(entry, "enabled", true);
        parsed.options = selection(entry.value("options", json::object()));
        state.entries.push_back(std::move(parsed));
    }

    for (auto& entry : root.value("mods", json::array()))
    {
        Mod mod;
        mod.folder      = str(entry, "folder");
        mod.name        = str(entry, "name");
        mod.kind        = str(entry, "kind");
        mod.path        = str(entry, "path");
        mod.version     = str(entry, "version");
        mod.author      = str(entry, "author");
        mod.description = str(entry, "description");
        mod.url         = str(entry, "url");
        mod.thumbnail   = str(entry, "thumbnail");
        mod.enabled     = flag(entry, "enabled", true);
        mod.priority    = (int)number(entry, "priority");
        mod.plugins     = (int)number(entry, "plugins");
        mod.loadPlugins = flag(entry, "load_plugins", true);
        mod.codeOnly    = flag(entry, "code_only");
        state.mods.push_back(std::move(mod));
    }

    for (auto& problem : root.value("problems", json::array()))
        if (problem.is_string())
            state.problems.push_back(problem.get<std::string>());

    return state;
}

bool isApplied()
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    return tkmc_is_applied();
}

ModDetails loadDetails(const std::string& folder)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    json root = parse(tkmc_mod_details(folder.c_str()));
    ModDetails details;
    details.folder      = str(root, "folder");
    details.kind        = str(root, "kind");
    details.name        = str(root, "name");
    details.version     = str(root, "version");
    details.author      = str(root, "author");
    details.description = str(root, "description");
    details.url         = str(root, "url");
    details.error       = str(root, "error");
    details.iniOptions  = selection(root.value("ini_options", json::object()));
    details.loadPlugins = flag(root, "load_plugins", true);
    for (auto& plugin : root.value("plugins", json::array()))
        if (plugin.is_string())
            details.plugins.push_back(plugin.get<std::string>());
    for (auto& group : root.value("option_groups", json::array()))
    {
        OptionGroup parsed;
        parsed.name        = str(group, "name");
        parsed.description = str(group, "description");
        parsed.type        = str(group, "type");
        for (auto& option : group.value("options", json::array()))
            parsed.options.emplace_back(str(option, "name"), str(option, "description"));
        for (auto& index : group.value("defaults", json::array()))
            if (index.is_number())
                parsed.defaults.push_back(index.get<int>());
        details.groups.push_back(std::move(parsed));
    }
    return details;
}

std::vector<unsigned char> loadThumbnail(const std::string& folder)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    uint8_t* data = nullptr;
    size_t size   = 0;
    std::vector<unsigned char> bytes;
    if (tkmc_mod_thumbnail(folder.c_str(), &data, &size))
    {
        bytes.assign(data, data + size);
        tkmc_free_bytes(data, size);
    }
    return bytes;
}

bool isValidProfileName(const std::string& name)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    return tkmc_profile_name_valid(name.c_str());
}

std::string saveProfile(const std::string& name, const std::vector<ProfileEntry>& entries)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    tkmc_profile_begin(name.c_str());
    for (auto& entry : entries)
    {
        tkmc_profile_add_mod(entry.folder.c_str(), entry.enabled);
        for (auto& [group, selected] : entry.options)
        {
            if (selected.empty())
                tkmc_profile_add_option(group.c_str(), "");
            for (auto& option : selected)
                tkmc_profile_add_option(group.c_str(), option.c_str());
        }
    }
    return take(tkmc_profile_commit());
}

std::string activateProfile(const std::string& name)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    return take(tkmc_profile_activate(name.c_str()));
}

std::string deleteProfile(const std::string& name)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    return take(tkmc_profile_delete(name.c_str()));
}

std::string renameProfile(const std::string& from, const std::string& to)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    return take(tkmc_profile_rename(from.c_str(), to.c_str()));
}

std::string setModPlugins(const std::string& folder, bool enabled)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    return take(tkmc_mod_set_plugins(folder.c_str(), enabled));
}

std::string setConfig(const std::string& key, const std::string& value)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    return take(tkmc_config_set(key.c_str(), value.c_str()));
}

std::string managerSetting(const std::string& key, const std::string& fallback)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    char* value = tkmc_manager_get(key.c_str());
    return value ? take(value) : fallback;
}

bool managerFlag(const std::string& key, bool fallback)
{
    std::string value = managerSetting(key, "");
    if (value == "1" || strcasecmp(value.c_str(), "true") == 0 || strcasecmp(value.c_str(), "yes") == 0
        || strcasecmp(value.c_str(), "on") == 0)
        return true;
    if (value == "0" || strcasecmp(value.c_str(), "false") == 0 || strcasecmp(value.c_str(), "no") == 0
        || strcasecmp(value.c_str(), "off") == 0)
        return false;
    return fallback;
}

std::string setManagerSetting(const std::string& key, const std::string& value)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    return take(tkmc_manager_set(key.c_str(), value.c_str()));
}

CacheUsage mergeCacheUsage()
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    json root = parse(tkmc_merge_cache_usage());
    CacheUsage usage;
    usage.merges = (int)number(root, "merges");
    usage.bytes  = (uint64_t)number(root, "bytes");
    return usage;
}

std::vector<Conflict> loadConflicts()
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    return conflictList(parse(tkmc_conflicts()));
}

bool normalizeProfile(State& state, const std::string& defaultName)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    bool changed = false;

    if (state.profile.empty() && !state.profiles.empty())
    {
        // Profiles exist but none is active: pick the first.
        activateProfile(state.profiles.front());
        state = loadState();
        changed = true;
    }

    if (state.profile.empty() || !state.profileExists)
    {
        // First run (or the active profile was deleted by hand): start from
        // what the plugin merged so far, i.e. every mod by its mod.ini.
        std::string name = state.profile.empty() ? defaultName : state.profile;
        std::vector<const Mod*> ordered;
        for (auto& mod : state.mods)
            ordered.push_back(&mod);
        std::stable_sort(ordered.begin(), ordered.end(), [](const Mod* a, const Mod* b) {
            if (a->priority != b->priority)
                return a->priority > b->priority;
            return a->name < b->name;
        });
        std::vector<ProfileEntry> entries;
        for (const Mod* mod : ordered)
            entries.push_back({ mod->folder, mod->enabled, {} });
        std::string error = saveProfile(name, entries);
        if (!error.empty())
        {
            applog::write("could not create profile " + name + ": " + error);
            return changed;
        }
        activateProfile(name);
        state = loadState();
        return true;
    }

    std::vector<ProfileEntry> added;
    for (auto& mod : state.mods)
        if (!state.findEntry(mod.folder))
            added.push_back({ mod.folder, true, {} });
    if (!added.empty())
    {
        std::vector<ProfileEntry> entries = added;
        entries.insert(entries.end(), state.entries.begin(), state.entries.end());
        std::string error = saveProfile(state.profile, entries);
        if (error.empty())
        {
            for (auto& entry : added)
                applog::write("new mod added to profile " + state.profile + ": " + entry.folder);
            state   = loadState();
            changed = true;
        }
    }
    return changed;
}

std::vector<Candidate> findMods(const std::string& dir)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    json root = parse(tkmc_find_mods(dir.c_str()));
    std::vector<Candidate> candidates;
    for (auto& entry : root.value("candidates", json::array()))
    {
        Candidate candidate;
        candidate.kind    = str(entry, "kind");
        candidate.path    = str(entry, "path");
        candidate.label   = str(entry, "label");
        candidate.hasCode = flag(entry, "has_code");
        candidate.size    = (uint64_t)number(entry, "size");
        candidates.push_back(std::move(candidate));
    }
    return candidates;
}

std::string folderName(const std::string& name)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    return take(tkmc_folder_name(name.c_str()));
}

std::string install(const Candidate& candidate, const std::string& folder, const InstallInfo& info)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    json root = parse(tkmc_install(candidate.path.c_str(), candidate.kind.c_str(), folder.c_str(), info.name.c_str(),
        info.version.c_str(), info.author.c_str(), info.description.c_str(), info.url.c_str(),
        info.thumbnail.c_str()));
    if (!flag(root, "ok"))
        throw std::runtime_error(str(root, "error"));
    return str(root, "path");
}

std::string uninstall(const std::string& folder)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    return take(tkmc_uninstall(folder.c_str()));
}

std::vector<std::string> migrateOldLayout()
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    json root = parse(tkmc_migrate());
    std::vector<std::string> moved;
    for (auto& folder : root.value("moved", json::array()))
        if (folder.is_string())
            moved.push_back(folder.get<std::string>());
    return moved;
}

bool removeTree(const std::string& dir)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    return tkmc_remove_tree(dir.c_str());
}

RomInfo romInfo(const std::string& prefix)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    json root = parse(tkmc_rom_info(prefix.c_str()));
    RomInfo info;
    info.ok      = flag(root, "ok");
    info.version = (int)number(root, "version");
    info.nso     = str(root, "nso");
    info.error   = str(root, "error");
    return info;
}

ApplyResult apply(const std::string& romPrefix, ProgressFn progress, ConflictsFn conflicts)
{
    std::lock_guard<std::recursive_mutex> guard(coreLock);
    ApplyContext context{ &progress, &conflicts };
    json root = parse(tkmc_apply(romPrefix.c_str(), progressTrampoline, conflictsTrampoline, &context));
    ApplyResult result;
    result.ok        = flag(root, "ok");
    result.reused    = flag(root, "reused");
    result.fromCache = flag(root, "from_cache");
    result.cancelled = flag(root, "cancelled");
    result.conflicts = (int)number(root, "conflicts");
    result.mods     = (int)number(root, "mods");
    result.files    = (int)number(root, "files");
    result.served   = (int)number(root, "served");
    result.warnings = (int)number(root, "warnings");
    result.patches  = (int)number(root, "patches");
    auto seconds    = root.find("seconds");
    result.seconds  = seconds != root.end() && seconds->is_number() ? seconds->get<double>() : 0;
    result.error    = str(root, "error");
    return result;
}

} // namespace core
