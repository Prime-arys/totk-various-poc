#include "app/app.hpp"

#include <algorithm>
#include <condition_variable>
#include <cstdlib>
#include <memory>
#include <mutex>
#include <strings.h>

#include <borealis.hpp>
#include <switch.h>

#include "app/widgets.hpp"
#include "core/game.hpp"
#include "core/sysclk.hpp"
#include "util/log.hpp"
#include "util/worker.hpp"

using namespace brls::literals;

namespace app
{

namespace
{
    core::State currentState;
    Preferences currentPreferences;
    std::string currentNroPath;
    brls::Event<> changedEvent;
    brls::Event<> orderChangedEvent;

    const char* stageText(int stage)
    {
        switch (stage)
        {
            case 0:
                return "app/apply/stage_reading";
            case 1:
                return "app/apply/stage_merging";
            case 3:
                return "app/apply/stage_conflicts";
            default:
                return "app/apply/stage_writing";
        }
    }

    bool sameFolder(const std::string& a, const std::string& b)
    {
        return strcasecmp(a.c_str(), b.c_str()) == 0;
    }

    /** "« A » wins over « B »: 2 files, 5 values", one line per pair of mods,
     *  most conflicts first. */
    std::string conflictSummary(const std::vector<core::Conflict>& conflicts)
    {
        struct Pair
        {
            std::string winner;
            std::string loser;
            int files  = 0;
            int values = 0;
        };
        std::vector<Pair> pairs;
        for (auto& conflict : conflicts)
        {
            for (size_t i = 1; i < conflict.mods.size(); i++)
            {
                const std::string& winner = conflict.mods[0].name;
                const std::string& loser  = conflict.mods[i].name;
                auto found = std::find_if(pairs.begin(), pairs.end(),
                    [&](const Pair& pair) { return pair.winner == winner && pair.loser == loser; });
                if (found == pairs.end())
                {
                    pairs.push_back({ winner, loser });
                    found = pairs.end() - 1;
                }
                if (conflict.wholeFile())
                    found->files++;
                else
                    found->values += conflict.count;
            }
        }
        std::stable_sort(pairs.begin(), pairs.end(),
            [](const Pair& a, const Pair& b) { return a.files + a.values > b.files + b.values; });

        const size_t SHOWN = 5;
        std::string text;
        for (size_t i = 0; i < pairs.size() && i < SHOWN; i++)
        {
            std::string counts;
            if (pairs[i].files > 0)
                counts = brls::getStr("app/conflicts/files", pairs[i].files);
            if (pairs[i].values > 0)
                counts += (counts.empty() ? "" : ", ") + brls::getStr("app/conflicts/values", pairs[i].values);
            std::string pair = brls::getStr("app/conflicts/pair", pairs[i].winner, pairs[i].loser);
            text += "· " + brls::getStr("app/conflicts/line", pair, counts) + "\n";
        }
        if (pairs.size() > SHOWN)
            text += brls::getStr("app/conflicts/more_pairs", pairs.size() - SHOWN) + "\n";
        return text;
    }

    /** Interface thread: asks whether to merge mods that conflict. */
    void openConflictQuestion(const std::vector<core::Conflict>& conflicts, std::function<void(bool)> reply)
    {
        brls::Style style = brls::Application::getStyle();
        auto* box         = new brls::Box(brls::Axis::COLUMN);
        box->setPadding(style["brls/dialog/paddingTopBottom"], style["brls/dialog/paddingLeftRight"],
            style["brls/dialog/paddingTopBottom"], style["brls/dialog/paddingLeftRight"]);

        auto* title = new brls::Label();
        title->setText(brls::getStr("app/conflicts/title", conflicts.size()));
        title->setFontSize(24);
        title->setMarginBottom(18);
        box->addView(title);

        auto* list = ui::paragraph(conflictSummary(conflicts), 18);
        box->addView(list);
        box->addView(ui::paragraph("app/conflicts/rule"_i18n, 16, true));

        auto* dialog = new brls::Dialog(box);
        // An answer is needed: the merge waits for it.
        dialog->setCancelable(false);
        dialog->addButton("app/common/cancel"_i18n, [reply]() { reply(false); });
        dialog->addButton("app/conflicts/apply_anyway"_i18n, [reply]() { reply(true); });
        dialog->open();
    }

    /** Merging thread: waits for the answer. */
    bool askAboutConflicts(const std::vector<core::Conflict>& conflicts)
    {
        struct Answer
        {
            std::mutex lock;
            std::condition_variable ready;
            bool done    = false;
            bool goAhead = false;
        };
        auto answer = std::make_shared<Answer>();
        brls::sync([conflicts, answer]() {
            openConflictQuestion(conflicts, [answer](bool goAhead) {
                {
                    std::lock_guard<std::mutex> guard(answer->lock);
                    answer->goAhead = goAhead;
                    answer->done    = true;
                }
                answer->ready.notify_all();
            });
        });
        std::unique_lock<std::mutex> guard(answer->lock);
        answer->ready.wait(guard, [&] { return answer->done; });
        applog::write(std::string("conflicts: ") + (answer->goAhead ? "merging anyway" : "merge cancelled"));
        return answer->goAhead;
    }

    void showResult(const core::ApplyResult& result)
    {
        if (result.cancelled)
        {
            ui::message("app/apply/cancelled"_i18n);
            return;
        }
        if (!result.ok)
        {
            ui::message(brls::getStr("app/apply/failed", result.error));
            return;
        }

        std::string text;
        if (result.mods == 0)
            text = "app/apply/nothing"_i18n;
        else if (result.fromCache)
            text = brls::getStr("app/apply/from_cache", result.mods, result.served);
        else if (result.reused)
            text = brls::getStr("app/apply/up_to_date", result.mods, result.served);
        else
            text = brls::getStr("app/apply/done", result.mods, result.files, result.seconds);
        if (result.conflicts > 0)
            text += "\n" + brls::getStr("app/apply/conflicts", result.conflicts);
        if (result.warnings > 0)
            text += "\n" + brls::getStr("app/apply/warnings", result.warnings);

        auto* dialog = new brls::Dialog(text);
        dialog->addButton("hints/ok"_i18n, []() {});
        if (!game::installedVersion().empty())
            dialog->addButton("app/apply/launch"_i18n, []() { brls::sync([]() { launchGame(); }); });
        dialog->open();
    }
} // namespace

core::State& state()
{
    return currentState;
}

const Preferences& preferences()
{
    return currentPreferences;
}

void loadPreferences()
{
    std::string boost = core::managerSetting("boost", "");
    if (boost == "off")
        currentPreferences.boost = Boost::Off;
    else if (boost == "standard")
        currentPreferences.boost = Boost::Standard;
    else if (boost == "sysclk")
        currentPreferences.boost = Boost::SysClk;
    else // before sys-clk support: a switch
        currentPreferences.boost = core::managerFlag("cpu_boost", true) ? Boost::SysClk : Boost::Off;
    currentPreferences.boostCpuMhz    = std::atoi(core::managerSetting("boost_cpu_mhz", "1785").c_str());
    currentPreferences.boostMemoryMhz = std::atoi(core::managerSetting("boost_memory_mhz", "1600").c_str());
    if (currentPreferences.boostCpuMhz <= 0)
        currentPreferences.boostCpuMhz = 1785;
    if (currentPreferences.boostMemoryMhz <= 0)
        currentPreferences.boostMemoryMhz = 1600;
    currentPreferences.warnConflicts = core::managerFlag("warn_conflicts", true);
    currentPreferences.language      = core::managerSetting("language", "auto");
}

void setNroPath(const std::string& path)
{
    currentNroPath = path;
}

const std::string& nroPath()
{
    return currentNroPath;
}

brls::Event<>& changed()
{
    return changedEvent;
}

brls::Event<>& orderChanged()
{
    return orderChangedEvent;
}

void reload(Merge merge)
{
    bool wasApplied = currentState.applied;
    currentState    = core::loadState();
    core::normalizeProfile(currentState, defaultProfileName());
    currentState.conflicts = core::loadConflicts();
    switch (merge)
    {
        case Merge::Check:
            currentState.applied = core::isApplied();
            break;
        case Merge::Current:
            currentState.applied = true;
            break;
        case Merge::Outdated:
            currentState.applied = false;
            break;
        case Merge::Unchanged:
            currentState.applied = wasApplied;
            break;
    }
    changedEvent.fire();
}

std::string defaultProfileName()
{
    return "app/profiles/default_name"_i18n;
}

std::string kindLabel(const std::string& kind)
{
    if (kind == "package")
        return "app/mods/kind_package"_i18n;
    if (kind == "romfs")
        return "app/mods/kind_romfs"_i18n;
    return "app/mods/kind_folder"_i18n;
}

std::vector<std::string> modOrder()
{
    std::vector<std::string> order;
    for (auto& entry : currentState.entries)
        if (currentState.findMod(entry.folder))
            order.push_back(entry.folder);
    return order;
}

std::string moveMod(const std::string& folder, size_t position)
{
    std::vector<std::string> order = modOrder();
    auto it = std::find_if(order.begin(), order.end(), [&](const std::string& f) { return sameFolder(f, folder); });
    if (it == order.end())
        return "";
    std::string moved = *it;
    order.erase(it);
    order.insert(order.begin() + std::min(position, order.size()), moved);

    // Mods listed in the profile but not installed keep to the end.
    std::vector<core::ProfileEntry> reordered;
    for (auto& name : order)
        reordered.push_back(*currentState.findEntry(name));
    for (auto& entry : currentState.entries)
        if (!currentState.findMod(entry.folder))
            reordered.push_back(entry);
    currentState.entries = reordered;
    currentState.applied = false;
    std::string error    = core::saveProfile(currentState.profile, currentState.entries);
    orderChangedEvent.fire();
    return error;
}

std::string modName(const std::string& folder)
{
    const core::Mod* mod = currentState.findMod(folder);
    return mod ? mod->name : folder;
}

std::vector<ActiveConflict> activeConflicts()
{
    // Position of each enabled mod in the profile.
    std::vector<std::string> enabled;
    for (auto& entry : currentState.entries)
        if (entry.enabled && currentState.findMod(entry.folder))
            enabled.push_back(entry.folder);
    auto position = [&](const std::string& folder) -> long {
        for (size_t i = 0; i < enabled.size(); i++)
            if (sameFolder(enabled[i], folder))
                return (long)i;
        return -1;
    };

    std::vector<ActiveConflict> active;
    for (auto& conflict : currentState.conflicts)
    {
        ActiveConflict entry{ conflict, {} };
        for (auto& mod : conflict.mods)
            if (position(mod.folder) >= 0)
                entry.folders.push_back(mod.folder);
        if (entry.folders.size() < 2)
            continue;
        std::stable_sort(entry.folders.begin(), entry.folders.end(),
            [&](const std::string& a, const std::string& b) { return position(a) < position(b); });
        active.push_back(std::move(entry));
    }
    return active;
}

std::vector<ActiveConflict> conflictsOf(const std::string& folder)
{
    std::vector<ActiveConflict> result;
    for (auto& entry : activeConflicts())
        if (std::any_of(entry.folders.begin(), entry.folders.end(), [&](const std::string& f) { return sameFolder(f, folder); }))
            result.push_back(std::move(entry));
    return result;
}

std::string shortFileName(const std::string& file)
{
    // The file name and its folder: "Mals/EUfr/EventFlowMsg/Npc.msbt" reads
    // as "EventFlowMsg/Npc.msbt".
    size_t last = file.rfind('/');
    if (last == std::string::npos || last == 0)
        return file;
    size_t before = file.rfind('/', last - 1);
    return before == std::string::npos ? file : file.substr(before + 1);
}

void apply()
{
    game::Romfs romfs = game::mountRomfs();
    if (romfs.source == game::RomSource::None)
    {
        ui::message(game::runsAsTotk() ? "app/apply/no_romfs_override"_i18n : "app/apply/no_romfs"_i18n);
        return;
    }

    if (currentState.locales.empty() || currentState.locales == "auto")
        game::rememberGuessedLocale();

    // A merge still in the cache comes back at once: no need to push the
    // console for that.
    bool merging  = !currentState.applied;
    bool boosted  = merging && currentPreferences.boost != Boost::Off && game::setCpuBoost(true);
    std::string title = boosted ? "app/apply/title_boost"_i18n : "app/apply/title"_i18n;
    if (merging && currentPreferences.boost == Boost::SysClk)
    {
        sysclk::Info clocks = sysclk::query();
        if (clocks.running)
        {
            uint32_t cpu    = sysclk::pick(clocks.cpu, currentPreferences.boostCpuMhz);
            uint32_t memory = sysclk::pick(clocks.memory, currentPreferences.boostMemoryMhz);
            if (!sysclk::boost(cpu, memory).empty())
                title = brls::getStr("app/apply/title_sysclk", cpu / 1000000, memory / 1000000);
        }
    }
    auto progress = ui::Progress::open(title);
    progress->update("app/apply/stage_start"_i18n, 0);
    auto result        = std::make_shared<core::ApplyResult>();
    std::string prefix = romfs.prefix;
    bool warn          = currentPreferences.warnConflicts;

    worker::run(
        [progress, result, prefix, warn]() {
            u64 lastUpdate = 0;
            auto onProgress = [&](int stage, uint32_t done, uint32_t total, const std::string& item) {
                // Reading mods is about a third of the work, merging most of
                // the rest.
                float part     = total > 0 ? (float)done / (float)total : 0;
                float fraction = 0.95f;
                if (stage == 0)
                    fraction = 0.30f * part;
                else if (stage == 3)
                    fraction = 0.30f + 0.05f * part;
                else if (stage == 1)
                    fraction = 0.35f + 0.57f * part;

                u64 now = armTicksToNs(armGetSystemTick()) / 1000000;
                if (now - lastUpdate < 100 && done != total)
                    return;
                lastUpdate         = now;
                std::string status = brls::getStr(stageText(stage));
                if (total > 0)
                    status += fmt::format(" ({}/{})", std::min(done + (stage == 0 ? 1 : 0), total), total);
                progress->update(status, fraction, item);
            };
            auto onConflicts = [warn](const std::vector<core::Conflict>& conflicts) {
                return !warn || askAboutConflicts(conflicts);
            };
            *result = core::apply(prefix, onProgress, onConflicts);
        },
        [progress, result, boosted]() {
            sysclk::unboost();
            if (boosted)
                game::setCpuBoost(false);
            game::unmountRomfs();
            applog::write("apply: ok=" + std::to_string(result->ok) + " files=" + std::to_string(result->files)
                + " served=" + std::to_string(result->served) + " conflicts=" + std::to_string(result->conflicts)
                + " error=" + result->error);
            progress->close([result]() {
                // A cancelled merge leaves the previous one as it was.
                reload(result->ok ? Merge::Current : result->cancelled ? Merge::Unchanged : Merge::Check);
                showResult(*result);
            });
        });
}

void launchGame()
{
    std::string error = game::launch();
    if (!error.empty())
    {
        ui::message(brls::getStr("app/apply/launch_failed", error));
        return;
    }
    brls::Application::quit();
}

} // namespace app
