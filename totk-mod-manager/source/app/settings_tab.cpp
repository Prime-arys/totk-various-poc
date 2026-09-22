#include "app/settings_tab.hpp"

#include <sys/stat.h>

#include <cstdio>
#include <memory>
#include <strings.h>

#include "app/app.hpp"
#include "app/widgets.hpp"
#include "core/game.hpp"
#include "core/sysclk.hpp"
#include "util/worker.hpp"

using namespace brls::literals;

namespace
{
bool exists(const std::string& path)
{
    struct stat info;
    return stat(path.c_str(), &info) == 0;
}

/** Settings read at boot only: the merge on the SD card stays what it was. */
void setFlag(const char* key, bool value)
{
    std::string error = core::setConfig(key, value ? "1" : "0");
    if (!error.empty())
        ui::message(brls::getStr("app/settings/save_failed", error));
    app::reload(app::Merge::Unchanged);
}

brls::DetailCell* infoCell(const std::string& title, const std::string& value, bool good)
{
    auto* cell = new brls::DetailCell();
    cell->setText(title);
    cell->setDetailText(value);
    cell->setDetailTextColor(good ? brls::Application::getTheme()["brls/list/listItem_value_color"] : nvgRGB(255, 120, 100));
    return cell;
}

/** Manager preferences, applied at once. */
void setPreference(const char* key, const std::string& value)
{
    std::string error = core::setManagerSetting(key, value);
    if (!error.empty())
        ui::message(brls::getStr("app/settings/save_failed", error));
    app::loadPreferences();
}

const char* LANGUAGE_VALUES[] = { "auto", "fr", "en-US" };

const int CACHE_SIZES[] = { 1, 2, 3, 5, 10, 15, 20, 30, 50 };

const char* BOOST_VALUES[] = { "off", "standard", "sysclk" };

/** "1785 MHz", or "1331.2 MHz" for a clock not on a round megahertz. */
std::string megahertz(uint32_t hz)
{
    char text[24];
    if (hz % 1000000 == 0)
        std::snprintf(text, sizeof(text), "%u MHz", hz / 1000000);
    else
        std::snprintf(text, sizeof(text), "%.1f MHz", hz / 1e6);
    return text;
}

/** A sys-clk frequency choice, saved as whole megahertz. */
brls::SelectorCell* frequencyCell(const std::string& title, const std::vector<uint32_t>& list, int currentMhz, const char* key)
{
    std::vector<std::string> labels;
    int index = 0;
    for (size_t i = 0; i < list.size(); i++)
    {
        labels.push_back(megahertz(list[i]));
        if ((int)(list[i] / 1000000) <= currentMhz)
            index = (int)i;
    }
    auto* cell = new brls::SelectorCell();
    cell->init(title, labels, index, [list, key](int selected) {
        if (selected >= 0 && selected < (int)list.size())
            setPreference(key, std::to_string(list[selected] / 1000000));
    });
    return cell;
}

const char* LOCALE_VALUES[] = { "auto", "all", "EUfr", "USfr", "EUen", "USen", "EUde", "EUes", "USes", "EUit", "EUnl", "EUru", "JPja", "KRko", "CNzh", "TWzh" };

/** The language the plugin saw the game read, if any. */
std::string detectedLocale()
{
    FILE* file = fopen("sdmc:/totk/locale.txt", "r");
    if (!file)
        return "";
    char text[16] = {};
    size_t read   = fread(text, 1, sizeof(text) - 1, file);
    fclose(file);
    std::string locale(text, read);
    while (!locale.empty() && std::isspace((unsigned char)locale.back()))
        locale.pop_back();
    return locale.size() == 4 ? locale : "";
}
} // namespace

SettingsTab::SettingsTab()
    : brls::Box(brls::Axis::COLUMN)
{
    this->setGrow(1);
    ui::ScrollList scroll = ui::scrollList();
    brls::Box* list       = scroll.content;
    this->addView(scroll.frame);
    const core::State& state = app::state();

    list->addView(ui::header("app/settings/merge"_i18n));

    auto* enabled = new brls::BooleanCell();
    enabled->init("app/settings/enabled"_i18n, state.mergerEnabled, [](bool on) { setFlag("enabled", on); });
    list->addView(enabled);

    auto* atBoot = new brls::BooleanCell();
    atBoot->init("app/settings/merge_at_boot"_i18n, state.mergeAtBoot, [](bool on) { setFlag("merge_at_boot", on); });
    list->addView(atBoot);
    list->addView(ui::paragraph("app/settings/merge_at_boot_hint"_i18n, 16, true));

    auto* patches = new brls::BooleanCell();
    patches->init("app/settings/patches"_i18n, state.applyPatches, [](bool on) { setFlag("apply_patches", on); });
    list->addView(patches);

    auto* modPlugins = new brls::BooleanCell();
    modPlugins->init("app/settings/mod_plugins"_i18n, state.modPlugins, [](bool on) { setFlag("mod_plugins", on); });
    list->addView(modPlugins);
    list->addView(ui::paragraph("app/settings/mod_plugins_hint"_i18n, 16, true));

    const app::Preferences& preferences = app::preferences();
    auto* warn = new brls::BooleanCell();
    warn->init("app/settings/warn_conflicts"_i18n, preferences.warnConflicts,
        [](bool on) { setPreference("warn_conflicts", on ? "1" : "0"); });
    list->addView(warn);
    list->addView(ui::paragraph("app/settings/warn_conflicts_hint"_i18n, 16, true));

    std::vector<std::string> localeLabels;
    std::string detected = detectedLocale();
    int localeIndex      = 0;
    for (size_t i = 0; i < sizeof(LOCALE_VALUES) / sizeof(LOCALE_VALUES[0]); i++)
    {
        std::string value = LOCALE_VALUES[i];
        if (value == "auto")
            localeLabels.push_back(detected.empty() ? "app/settings/locales_auto_unknown"_i18n
                                                    : brls::getStr("app/settings/locales_auto", detected));
        else if (value == "all")
            localeLabels.push_back("app/settings/locales_all"_i18n);
        else
            localeLabels.push_back(value);
        if (strcasecmp(state.locales.c_str(), value.c_str()) == 0 || (value == "auto" && state.locales.empty()))
            localeIndex = (int)i;
    }
    auto* locales = new brls::SelectorCell();
    locales->init("app/settings/locales"_i18n, localeLabels, localeIndex, [](int selected) {
        std::string error = core::setConfig("locales", LOCALE_VALUES[selected]);
        if (!error.empty())
            ui::message(brls::getStr("app/settings/save_failed", error));
        // A merge with more languages than needed is still current.
        app::reload(app::Merge::Check);
    });
    list->addView(locales);
    list->addView(ui::paragraph("app/settings/locales_hint"_i18n, 16, true));

    int cacheIndex = 0;
    std::vector<std::string> cacheLabels;
    for (size_t i = 0; i < sizeof(CACHE_SIZES) / sizeof(CACHE_SIZES[0]); i++)
    {
        cacheLabels.push_back(std::to_string(CACHE_SIZES[i]));
        if (CACHE_SIZES[i] <= state.mergeCacheSize)
            cacheIndex = (int)i;
    }
    auto* cacheSize = new brls::SelectorCell();
    cacheSize->init("app/settings/cache_size"_i18n, cacheLabels, cacheIndex, [](int selected) {
        // Fewer merges kept takes effect after the next merge.
        std::string error = core::setConfig("merge_cache_size", std::to_string(CACHE_SIZES[selected]));
        if (!error.empty())
            ui::message(brls::getStr("app/settings/save_failed", error));
        app::reload(app::Merge::Unchanged);
    });
    list->addView(cacheSize);
    auto* cacheUsage = new brls::DetailCell();
    cacheUsage->setText("app/settings/cache_usage"_i18n);
    cacheUsage->setDetailText("app/browse/loading"_i18n);
    cacheUsage->setFocusable(false);
    list->addView(cacheUsage);
    list->addView(ui::paragraph("app/settings/cache_hint"_i18n, 16, true));
    auto usage = std::make_shared<core::CacheUsage>();
    ASYNC_RETAIN
    worker::run([usage]() { *usage = core::mergeCacheUsage(); },
        [ASYNC_TOKEN, usage, cacheUsage]() {
            ASYNC_RELEASE
            cacheUsage->setDetailText(brls::getStr("app/settings/cache_usage_value", usage->merges, ui::formatSize(usage->bytes)));
        },
        256 * 1024);

    buildBoost(list);

    list->addView(ui::header("app/settings/interface"_i18n));
    int languageIndex = 0;
    for (size_t i = 0; i < sizeof(LANGUAGE_VALUES) / sizeof(LANGUAGE_VALUES[0]); i++)
        if (preferences.language == LANGUAGE_VALUES[i])
            languageIndex = (int)i;
    auto* language = new brls::SelectorCell();
    // Both names in their own language: whoever cannot read the current one
    // still finds theirs.
    language->init("app/settings/language"_i18n, { "app/settings/language_auto"_i18n, "Français", "English" }, languageIndex,
        [](int selected) {
            if (selected < 0 || app::preferences().language == LANGUAGE_VALUES[selected])
                return;
            setPreference("language", LANGUAGE_VALUES[selected]);
            // Translations are loaded at start.
            brls::sync([]() {
                ui::confirm("app/settings/language_restart"_i18n, "app/settings/restart"_i18n, []() {
                    if (!game::restartAfterExit(app::nroPath()))
                    {
                        ui::message("app/settings/restart_manually"_i18n);
                        return;
                    }
                    brls::Application::quit();
                });
            });
        });
    list->addView(language);

    list->addView(ui::header("app/settings/installation"_i18n));
    std::string version = game::installedVersion();
    list->addView(infoCell("app/settings/game"_i18n, version.empty() ? "app/settings/not_found"_i18n : version,
        version == "1.2.1"));
    const std::string exefs = "sdmc:/atmosphere/contents/0100F2C0115B6000/exefs/";
    bool skyline            = exists(exefs + "subsdk9") && exists(exefs + "main.npdm");
    list->addView(infoCell("app/settings/skyline"_i18n, skyline ? "app/settings/installed"_i18n : "app/settings/missing"_i18n,
        skyline));
    bool plugin = exists("sdmc:/atmosphere/contents/0100F2C0115B6000/skyline/plugins/totk-mod-merger-plugin.nro");
    list->addView(infoCell("app/settings/plugin"_i18n, plugin ? "app/settings/installed"_i18n : "app/settings/missing"_i18n,
        plugin));
    bool romfsFolder = exists("sdmc:/atmosphere/contents/0100F2C0115B6000/romfs");
    if (romfsFolder)
        list->addView(ui::paragraph("app/settings/layeredfs_warning"_i18n, 16));
    std::string mode = game::runsAsTotk() ? "app/settings/mode_override"_i18n
        : game::isApplication()           ? "app/settings/mode_application"_i18n
                                          : "app/settings/mode_applet"_i18n;
    list->addView(infoCell("app/settings/mode"_i18n, mode, game::runsAsTotk()));
    list->addView(ui::paragraph("app/settings/mode_hint"_i18n, 16, true));

    list->addView(ui::header("app/settings/maintenance"_i18n));
    auto* clear = new ui::ListRow(false);
    clear->title->setText("app/settings/clear_cache"_i18n);
    clear->subtitle->setText("app/settings/clear_cache_hint"_i18n);
    clear->registerClickAction([](brls::View*) {
        ui::confirm("app/settings/clear_cache_confirm"_i18n, "app/settings/clear"_i18n, []() {
            core::removeTree("sd:/totk/merged");
            core::removeTree("sd:/totk/cache");
            core::removeTree("sd:/totk/downloads");
            app::reload();
            brls::Application::notify("app/settings/cleared"_i18n);
        });
        return true;
    });
    list->addView(clear);

    list->addView(ui::header("app/settings/about"_i18n));
    list->addView(ui::paragraph(brls::getStr("app/settings/about_text", APP_VERSION), 16, true));
}

void SettingsTab::buildBoost(brls::Box* list)
{
    const app::Preferences& preferences = app::preferences();
    list->addView(ui::header("app/settings/boost"_i18n));

    auto* mode = new brls::SelectorCell();
    mode->init("app/settings/boost_mode"_i18n,
        { "app/settings/boost_off"_i18n, "app/settings/boost_standard"_i18n, "app/settings/boost_sysclk"_i18n },
        (int)preferences.boost, [](int selected) {
            if (selected >= 0 && selected < 3)
                setPreference("boost", BOOST_VALUES[selected]);
        });
    list->addView(mode);
    list->addView(ui::paragraph("app/settings/boost_hint"_i18n, 16, true));

    sysclk::Info clocks = sysclk::query();
    if (!clocks.running)
    {
        list->addView(ui::paragraph("app/settings/sysclk_missing"_i18n, 16));
        return;
    }
    if (clocks.cpu.empty() || clocks.memory.empty())
    {
        // An older sys-clk: it cannot list its clocks, it rounds to the
        // nearest one.
        list->addView(ui::paragraph(brls::getStr("app/settings/sysclk_old", clocks.apiVersion,
                                        preferences.boostCpuMhz, preferences.boostMemoryMhz),
            16));
        return;
    }
    list->addView(frequencyCell("app/settings/sysclk_cpu"_i18n, clocks.cpu, preferences.boostCpuMhz, "boost_cpu_mhz"));
    list->addView(frequencyCell("app/settings/sysclk_memory"_i18n, clocks.memory, preferences.boostMemoryMhz, "boost_memory_mhz"));
    list->addView(ui::paragraph("app/settings/sysclk_hint"_i18n, 16, true));
}
