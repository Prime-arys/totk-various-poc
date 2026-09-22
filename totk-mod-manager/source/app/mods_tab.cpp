#include "app/mods_tab.hpp"

#include <algorithm>

#include "app/app.hpp"
#include "app/mod_activity.hpp"
#include "util/images.hpp"

using namespace brls::literals;

ModsTab::ModsTab()
    : brls::Box(brls::Axis::COLUMN)
{
    this->setGrow(1);

    auto* top = new brls::Box(brls::Axis::ROW);
    top->setAlignItems(brls::AlignItems::CENTER);
    top->setPadding(26, 40, 8, 40);

    profileLabel = new brls::Label();
    profileLabel->setFontSize(24);
    profileLabel->setGrow(1);
    profileLabel->setSingleLine(true);
    top->addView(profileLabel);

    statusLabel = new brls::Label();
    statusLabel->setFontSize(18);
    statusLabel->setShrink(0);
    top->addView(statusLabel);
    this->addView(top);

    hintLabel = new brls::Label();
    hintLabel->setFontSize(15);
    hintLabel->setText("app/mods/hint"_i18n);
    hintLabel->setTextColor(brls::Application::getTheme()["brls/header/subtitle"]);
    hintLabel->setMargins(0, 40, 4, 40);
    this->addView(hintLabel);

    ui::ScrollList scroll = ui::scrollList();
    frame                 = scroll.frame;
    list                  = scroll.content;
    this->addView(frame);

    // While a mod is picked up, the D-pad moves it instead of the focus.
    this->registerAction("", brls::BUTTON_NAV_UP, [this](brls::View*) {
        if (grabbed.empty())
            return false;
        move(grabbed, -1);
        return true;
    }, true, true);
    this->registerAction("", brls::BUTTON_NAV_DOWN, [this](brls::View*) {
        if (grabbed.empty())
            return false;
        move(grabbed, 1);
        return true;
    }, true, true);
    this->registerAction("", brls::BUTTON_B, [this](brls::View*) {
        if (grabbed.empty())
            return false;
        drop();
        return true;
    }, true);

    subscription = app::changed().subscribe([this]() {
        dirty   = false;
        grabbed.clear();
        build();
    });
    orderSubscription = app::orderChanged().subscribe([this]() { followOrder(); });
    build();
}

ModsTab::~ModsTab()
{
    app::changed().unsubscribe(subscription);
    app::orderChanged().unsubscribe(orderSubscription);
}

void ModsTab::willAppear(bool resetState)
{
    brls::Box::willAppear(resetState);
    updateStatus();
    // Options changed on a mod's page can change its conflicts.
    for (size_t i = 0; i < rows.size(); i++)
        updateRow(i);
}

void ModsTab::willDisappear(bool resetState)
{
    brls::Box::willDisappear(resetState);
    if (!grabbed.empty())
        drop();
}

void ModsTab::updateStatus()
{
    const core::State& state = app::state();
    bool applied = state.applied && !dirty;
    statusLabel->setText(applied ? "app/mods/applied"_i18n : "app/mods/not_applied"_i18n);
    statusLabel->setTextColor(applied ? brls::Application::getTheme()["brls/list/listItem_value_color"] : ui::warningColor());
}

void ModsTab::build(const std::string& focusFolder)
{
    core::State& state = app::state();
    profileLabel->setText(brls::getStr("app/mods/profile", state.profile));
    updateStatus();

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
    rows.clear();

    for (auto& problem : state.problems)
        list->addView(ui::paragraph(problem, 16, true));

    order = app::modOrder();
    if (order.empty())
    {
        list->addView(ui::paragraph("app/mods/empty"_i18n, 20));
        if (refocus)
            brls::Application::giveFocus(nullptr);
        return;
    }

    brls::View* focus = nullptr;
    for (size_t index = 0; index < order.size(); index++)
    {
        const std::string folder = order[index];

        auto* row = new ui::ListRow(true, 160, 90);
        rows.push_back(row);
        updateRow(index);
        images::fromMod(row->image, folder);

        row->registerClickAction([this, folder](brls::View*) {
            if (!grabbed.empty())
                drop();
            else
                toggle(folder);
            return true;
        });
        row->registerAction("app/mods/details"_i18n, brls::BUTTON_Y, [this, folder](brls::View*) {
            if (!grabbed.empty())
                drop();
            brls::Application::pushActivity(new ModActivity(folder));
            return true;
        });
        row->registerAction("app/mods/move"_i18n, brls::BUTTON_BACK, [this, folder](brls::View*) {
            if (grabbed.empty())
                grab(folder);
            else
                drop();
            return true;
        });
        // One step at a time, without picking the mod up.
        row->registerAction("app/mods/move_up"_i18n, brls::BUTTON_LB, [this, folder](brls::View*) {
            move(folder, -1);
            return true;
        }, true, true);
        row->registerAction("app/mods/move_down"_i18n, brls::BUTTON_RB, [this, folder](brls::View*) {
            move(folder, 1);
            return true;
        }, true, true);

        list->addView(row);
        if (folder == focusFolder)
            focus = row;
    }

    if (focus)
        brls::Application::giveFocus(focus);
    else if (refocus)
        ui::focusChild(list, focusIndex);
}

void ModsTab::updateRow(size_t index)
{
    if (index >= rows.size() || index >= order.size())
        return;
    core::State& state              = app::state();
    const std::string& folder       = order[index];
    const core::Mod* mod            = state.findMod(folder);
    const core::ProfileEntry* entry = state.findEntry(folder);
    ui::ListRow* row                = rows[index];
    if (!mod)
        return;

    bool moving = folder == grabbed;
    row->title->setText(mod->name);
    std::string subtitle = "#" + std::to_string(index + 1) + "  ·  ";
    // What it is comes first: the marker for the code a mod ships has to be
    // visible, and a long subtitle gets cut.
    subtitle += mod->codeOnly ? "app/mods/kind_code"_i18n : app::kindLabel(mod->kind);
    if (mod->plugins > 0 && !mod->codeOnly)
        subtitle += "  ·  " + "app/mods/plugin"_i18n;
    if (mod->plugins > 0 && !(mod->loadPlugins && state.modPlugins))
        subtitle += "  ·  " + "app/mods/plugin_off"_i18n;
    if (!mod->version.empty())
        subtitle += "  ·  v" + mod->version;
    if (!mod->author.empty())
        subtitle += "  ·  " + mod->author;
    row->subtitle->setText(subtitle);

    if (moving)
        row->setValue("app/mods/moving"_i18n, true);
    else
        row->setValue(entry && entry->enabled ? "app/mods/enabled"_i18n : "app/mods/disabled"_i18n, entry && entry->enabled);
    row->setBackgroundColor(moving ? nvgRGBA(0, 255, 204, 28) : nvgRGBA(0, 0, 0, 0));

    size_t conflicts = entry && entry->enabled ? app::conflictsOf(folder).size() : 0;
    row->setNote(conflicts > 0 ? brls::getStr("app/mods/conflicts", conflicts) : "", ui::warningColor());
}

void ModsTab::toggle(const std::string& folder)
{
    core::ProfileEntry* entry = app::state().findEntry(folder);
    if (!entry)
        return;
    entry->enabled = !entry->enabled;
    save();
    // Conflicts involve enabled mods only.
    for (size_t i = 0; i < rows.size(); i++)
        updateRow(i);
}

void ModsTab::move(const std::string& folder, int delta)
{
    auto it = std::find(order.begin(), order.end(), folder);
    if (it == order.end())
        return;
    long from = it - order.begin();
    long to   = from + delta;
    if (to < 0 || to >= (long)order.size())
        return;

    // The row moves in place (followOrder): rebuilding the list would reload
    // every picture, and free the row whose action is running.
    ui::ListRow* row  = rows[from];
    std::string error = app::moveMod(folder, (size_t)to);
    if (!error.empty())
    {
        ui::message(brls::getStr("app/profiles/save_failed", error));
        return;
    }

    // Keep the moved row in view once the list is laid out again.
    ASYNC_RETAIN
    brls::delay(40, [ASYNC_TOKEN, row]() {
        ASYNC_RELEASE
        if (brls::Application::getCurrentFocus() == row)
            frame->onChildFocusGained(list, row);
    });
}

void ModsTab::followOrder()
{
    std::vector<std::string> wanted = app::modOrder();
    if (wanted == order)
        return;
    if (wanted.size() != order.size() || !std::is_permutation(wanted.begin(), wanted.end(), order.begin()))
        return; // mods came or went: reload() rebuilds the list

    std::vector<ui::ListRow*> reordered;
    for (auto& folder : wanted)
        reordered.push_back(rows[std::find(order.begin(), order.end(), folder) - order.begin()]);
    size_t firstRow = list->getChildren().size() - rows.size();
    for (auto* row : rows)
        list->removeView(row, false);
    for (size_t i = 0; i < reordered.size(); i++)
        list->addView(reordered[i], firstRow + i);
    order = wanted;
    rows  = reordered;
    for (size_t i = 0; i < rows.size(); i++)
        updateRow(i);
    dirty = true;
    updateStatus();
}

void ModsTab::grab(const std::string& folder)
{
    grabbed = folder;
    hintLabel->setText("app/mods/moving_hint"_i18n);
    auto it = std::find(order.begin(), order.end(), folder);
    if (it != order.end())
        updateRow(it - order.begin());
}

void ModsTab::drop()
{
    std::string folder = grabbed;
    grabbed.clear();
    hintLabel->setText("app/mods/hint"_i18n);
    auto it = std::find(order.begin(), order.end(), folder);
    if (it != order.end())
        updateRow(it - order.begin());
}

void ModsTab::save()
{
    core::State& state = app::state();
    std::string error  = core::saveProfile(state.profile, state.entries);
    if (!error.empty())
        ui::message(brls::getStr("app/profiles/save_failed", error));
    state.applied = false;
    dirty         = true;
    updateStatus();
}
