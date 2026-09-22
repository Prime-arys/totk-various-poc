// totk-mod-manager: manages the mods totk-mod-merger-plugin merges into Tears
// of the Kingdom, from the console.

#include <sys/stat.h>

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <exception>

#include <borealis.hpp>
#include <switch.h>

#include "app/app.hpp"
#include "app/main_activity.hpp"
#include "core/core.hpp"
#include "core/game.hpp"
#include "core/sysclk.hpp"
#include "net/http.hpp"
#include "util/images.hpp"
#include "util/log.hpp"
#include "util/worker.hpp"

using namespace brls::literals;

namespace
{
[[noreturn]] void onTerminate()
{
    std::string what = "unknown";
    if (auto current = std::current_exception())
    {
        try
        {
            std::rethrow_exception(current);
        }
        catch (const std::exception& e)
        {
            what = e.what();
        }
        catch (...)
        {
        }
    }
    applog::write("fatal: uncaught exception: " + what);
    applog::flush();
    std::abort();
}

/** The translation to load: the one picked in the settings, or the console's
 *  language, where every French variant (fr-CA too) reads the French one and
 *  every other language the English one. */
std::string interfaceLocale(const std::string& preference)
{
    if (preference == "fr" || preference == "en-US")
        return preference;
    u64 code = 0;
    if (R_SUCCEEDED(setGetSystemLanguage(&code)) && std::strncmp((const char*)&code, "fr", 2) == 0)
        return "fr";
    return brls::LOCALE_AUTO;
}
} // namespace

int main(int argc, char* argv[])
{
    std::set_terminate(onTerminate);
    mkdir("sdmc:/totk", 0777);
    applog::open("sdmc:/totk/manager.log");
    char program[64];
    std::snprintf(program, sizeof(program), "program %016llX, applet type %d",
        (unsigned long long)game::currentProgramId(), (int)appletGetAppletType());
    applog::write(std::string("totk-mod-manager ") + APP_VERSION + ", " + program
        + (game::runsAsTotk() ? " (started from TotK's icon)" : ""));

    if (argc > 0 && argv[0])
        app::setNroPath(argv[0]);

    http::init();
    core::init();
    app::loadPreferences();

    brls::Platform::APP_LOCALE_DEFAULT = interfaceLocale(app::preferences().language);
    if (!brls::Application::init())
    {
        applog::write("could not start borealis");
        return EXIT_FAILURE;
    }
    brls::Application::createWindow("app/title"_i18n);
    brls::Application::getPlatform()->setThemeVariant(brls::ThemeVariant::DARK);
    brls::Application::setGlobalQuit(false);

    // Mods of the old layout (loose .tkcl files) move into folders first.
    for (auto& folder : core::migrateOldLayout())
        applog::write("moved into its own folder: " + folder);
    app::reload(app::Merge::Check);

    brls::Application::pushActivity(new MainActivity());
    while (brls::Application::mainLoop())
        ;

    // Nothing of the manager may outlive it: the homebrew menu is loaded into
    // this same process next, and a thread still running from the manager's
    // unloaded code crashes it.
    http::abort();
    worker::shutdown();
    images::shutdown();
    sysclk::unboost();
    game::endCpuBoost();
    http::cleanup();
    applog::write("bye");
    return EXIT_SUCCESS;
}
