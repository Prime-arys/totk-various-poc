#include "app/main_activity.hpp"

#include "app/app.hpp"
#include "app/browse_tab.hpp"
#include "app/mods_tab.hpp"
#include "app/profiles_tab.hpp"
#include "app/settings_tab.hpp"
#include "app/widgets.hpp"
#include "core/game.hpp"

using namespace brls::literals;

brls::View* MainActivity::createContentView()
{
    auto* tabs = new brls::TabFrame();
    tabs->addTab("app/tabs/mods"_i18n, []() { return new ModsTab(); });
    tabs->addTab("app/tabs/profiles"_i18n, []() { return new ProfilesTab(); });
    tabs->addSeparator();
    tabs->addTab("app/tabs/gamebanana"_i18n, []() { return new BrowseTab(); });
    tabs->addSeparator();
    tabs->addTab("app/tabs/settings"_i18n, []() { return new SettingsTab(); });

    auto* frame = new brls::AppletFrame(tabs);
    frame->setTitle("app/title"_i18n);
    frame->setIcon(std::string(BRLS_RESOURCES) + "img/icon.png");

    frame->registerAction("app/actions/apply"_i18n, brls::BUTTON_X, [](brls::View*) {
        app::apply();
        return true;
    });
    frame->registerAction("app/actions/quit"_i18n, brls::BUTTON_START, [](brls::View*) {
        brls::Application::quit();
        return true;
    });
    return frame;
}

void MainActivity::onContentAvailable()
{
    if (!app::state().problems.empty())
        return;
    // A gentle word on first use when the game's files are out of reach.
    if (!app::state().applied)
        brls::Application::notify("app/notify/not_applied"_i18n);
}
