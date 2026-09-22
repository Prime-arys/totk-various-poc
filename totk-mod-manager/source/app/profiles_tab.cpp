#include "app/profiles_tab.hpp"

#include <strings.h>

#include "app/app.hpp"
#include "app/widgets.hpp"

using namespace brls::literals;

namespace
{
bool profileExists(const std::string& name)
{
    for (auto& existing : app::state().profiles)
        if (strcasecmp(existing.c_str(), name.c_str()) == 0)
            return true;
    return false;
}

void reportError(const std::string& error)
{
    if (!error.empty())
        ui::message(brls::getStr("app/profiles/error", error));
}
} // namespace

ProfilesTab::ProfilesTab()
    : brls::Box(brls::Axis::COLUMN)
{
    this->setGrow(1);
    ui::ScrollList scroll = ui::scrollList();
    list                  = scroll.content;
    this->addView(scroll.frame);

    subscription = app::changed().subscribe([this]() { build(); });
    build();
}

ProfilesTab::~ProfilesTab()
{
    app::changed().unsubscribe(subscription);
}

void ProfilesTab::build()
{
    const core::State& state = app::state();
    // Rebuilt while one of our rows has the focus: give it back afterwards.
    brls::View* focused = brls::Application::getCurrentFocus();
    size_t focusIndex   = 0;
    bool refocus        = false;
    auto& children      = list->getChildren();
    for (size_t i = 0; i < children.size(); i++)
    {
        if (ui::isInside(focused, children[i]))
        {
            focusIndex = i;
            refocus    = true;
        }
    }
    list->clearViews();

    list->addView(ui::header("app/profiles/title"_i18n));
    list->addView(ui::paragraph("app/profiles/explanation"_i18n, 18, true));

    for (const std::string& name : state.profiles)
    {
        bool active = name == state.profile;
        auto* row   = new ui::ListRow(false);
        row->title->setText(name);
        int enabled = 0, total = 0;
        if (active)
        {
            for (auto& entry : state.entries)
            {
                if (!state.findMod(entry.folder))
                    continue;
                total++;
                enabled += entry.enabled ? 1 : 0;
            }
            row->subtitle->setText(brls::getStr("app/profiles/summary", enabled, total));
        }
        else
        {
            row->subtitle->setText("app/profiles/inactive_hint"_i18n);
        }
        row->setValue(active ? "app/profiles/active"_i18n : "", true);
        row->registerClickAction([this, name](brls::View*) {
            openMenu(name);
            return true;
        });
        list->addView(row);
    }

    list->addView(ui::header("app/profiles/new"_i18n));

    auto* empty = new ui::ListRow(false);
    empty->title->setText("app/profiles/create_empty"_i18n);
    empty->subtitle->setText("app/profiles/create_empty_hint"_i18n);
    empty->registerClickAction([this](brls::View*) {
        askName("app/profiles/create_empty"_i18n, "", [this](std::string name) { createProfile(name, false); });
        return true;
    });
    list->addView(empty);

    auto* copy = new ui::ListRow(false);
    copy->title->setText("app/profiles/create_copy"_i18n);
    copy->subtitle->setText(brls::getStr("app/profiles/create_copy_hint", state.profile));
    copy->registerClickAction([this](brls::View*) {
        askName("app/profiles/create_copy"_i18n, app::state().profile + " 2",
            [this](std::string name) { createProfile(name, true); });
        return true;
    });
    list->addView(copy);

    if (refocus)
        ui::focusChild(list, focusIndex);
}

void ProfilesTab::askName(const std::string& title, const std::string& initial, std::function<void(std::string)> then)
{
    brls::Application::getImeManager()->openForText(
        [then](std::string text) {
            // Trailing spaces from the keyboard are not part of the name.
            while (!text.empty() && text.back() == ' ')
                text.pop_back();
            if (text.empty())
                return;
            if (!core::isValidProfileName(text))
            {
                brls::sync([]() { ui::message("app/profiles/invalid_name"_i18n); });
                return;
            }
            if (profileExists(text))
            {
                brls::sync([text]() { ui::message(brls::getStr("app/profiles/exists", text)); });
                return;
            }
            brls::sync([then, text]() { then(text); });
        },
        title, "app/profiles/name_hint"_i18n, 32, initial);
}

void ProfilesTab::createProfile(const std::string& name, bool copyActive)
{
    const core::State& state = app::state();
    std::vector<core::ProfileEntry> entries;
    if (copyActive)
    {
        entries = state.entries;
    }
    else
    {
        // Every mod listed, none enabled: the list is there to be ticked.
        for (auto& mod : state.mods)
            entries.push_back({ mod.folder, false, {} });
    }
    std::string error = core::saveProfile(name, entries);
    if (error.empty())
        error = core::activateProfile(name);
    reportError(error);
    app::reload();
}

void ProfilesTab::openMenu(const std::string& name)
{
    const core::State& state = app::state();
    bool active              = name == state.profile;

    std::vector<std::string> labels;
    std::vector<std::function<void()>> actions;

    if (!active)
    {
        labels.push_back("app/profiles/activate"_i18n);
        actions.push_back([name]() {
            reportError(core::activateProfile(name));
            // Switching back to the profile that was applied needs no merge.
            app::reload(app::Merge::Check);
        });
    }
    labels.push_back("app/profiles/rename"_i18n);
    actions.push_back([this, name]() {
        askName("app/profiles/rename"_i18n, name, [name](std::string to) {
            reportError(core::renameProfile(name, to));
            app::reload();
        });
    });
    if (!active)
    {
        labels.push_back("app/profiles/delete"_i18n);
        actions.push_back([name]() {
            ui::confirm(brls::getStr("app/profiles/delete_confirm", name), "app/profiles/delete"_i18n, [name]() {
                reportError(core::deleteProfile(name));
                app::reload();
            });
        });
    }

    auto* dropdown = new brls::Dropdown(
        name, labels,
        [actions](int selected) {
            if (selected >= 0 && selected < (int)actions.size())
            {
                auto action = actions[selected];
                // After the menu is gone: some actions open the keyboard.
                brls::sync([action]() { action(); });
            }
        },
        -1);
    brls::Application::pushActivity(new brls::Activity(dropdown));
}
