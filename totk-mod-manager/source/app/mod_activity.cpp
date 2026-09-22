#include "app/mod_activity.hpp"

#include <algorithm>
#include <memory>
#include <strings.h>

#include "app/app.hpp"
#include "app/widgets.hpp"
#include "util/images.hpp"

using namespace brls::literals;

namespace
{
bool sameName(const std::string& a, const std::string& b)
{
    return strcasecmp(a.c_str(), b.c_str()) == 0;
}

/** Under its option: long descriptions would push the check mark away. */
void addOptionDescription(brls::Box* content, const std::string& description)
{
    if (description.empty())
        return;
    auto* label = ui::paragraph(description, 15, true);
    label->setMargins(0, 16, 10, 16);
    content->addView(label);
}
} // namespace

ModActivity::ModActivity(const std::string& folder)
    : folder(folder)
{
    details = core::loadDetails(folder);
}

brls::View* ModActivity::createContentView()
{
    ui::ScrollList scroll = ui::scrollList();
    brls::Box* content    = scroll.content;

    auto* top = new brls::Box(brls::Axis::ROW);
    top->setMarginBottom(24);

    auto* image = new brls::Image();
    image->setWidth(384);
    image->setHeight(216);
    image->setScalingType(brls::ImageScalingType::FIT);
    image->setCornerRadius(6);
    image->setBackgroundColor(nvgRGBA(255, 255, 255, 18));
    image->setMarginRight(30);
    images::fromMod(image, folder);
    top->addView(image);

    auto* facts = new brls::Box(brls::Axis::COLUMN);
    facts->setGrow(1);
    facts->setShrink(1);
    auto* name = new brls::Label();
    name->setText(details.name);
    name->setFontSize(30);
    name->setIsWrapping(true);
    name->setMarginBottom(16);
    facts->addView(name);
    auto fact = [&](const std::string& label, const std::string& value) {
        if (value.empty())
            return;
        facts->addView(ui::paragraph(brls::getStr("app/common/labelled", label, value), 18, true));
    };
    const core::Mod* listed = app::state().findMod(folder);
    bool codeOnly           = listed && listed->codeOnly;
    fact("app/details/kind"_i18n, codeOnly ? "app/mods/kind_code"_i18n : app::kindLabel(details.kind));
    fact("app/details/version"_i18n, details.version);
    fact("app/details/author"_i18n, details.author);
    fact("app/details/folder"_i18n, "sd:/totk/mods/" + folder);
    fact("app/details/source"_i18n, details.url);
    top->addView(facts);
    content->addView(top);

    if (!details.error.empty())
        content->addView(ui::paragraph(details.error, 18));

    buildPriority(content);

    // Options before the description: GameBanana descriptions run long.
    buildOptions(content);

    buildPlugins(content);

    conflictsBox = new brls::Box(brls::Axis::COLUMN);
    content->addView(conflictsBox);
    buildConflicts();

    if (!details.description.empty())
    {
        content->addView(ui::header("app/details/description"_i18n));
        // Long pages would make a very tall label.
        std::string text = details.description;
        if (text.size() > 3000)
            text = text.substr(0, 3000) + "…";
        content->addView(ui::paragraph(text, 18));
    }

    content->addView(ui::header("app/details/manage"_i18n));
    auto* remove = new ui::ListRow(false);
    remove->title->setText("app/details/uninstall"_i18n);
    remove->title->setTextColor(nvgRGB(255, 110, 100));
    remove->subtitle->setText("app/details/uninstall_hint"_i18n);
    std::string target = folder;
    remove->registerClickAction([target](brls::View*) {
        ui::confirm(brls::getStr("app/details/uninstall_confirm", target), "app/details/uninstall"_i18n, [target]() {
            std::string error = core::uninstall(target);
            if (!error.empty())
            {
                ui::message(brls::getStr("app/details/uninstall_failed", error));
                return;
            }
            brls::Application::popActivity(brls::TransitionAnimation::FADE, []() { app::reload(); });
        });
        return true;
    });
    content->addView(remove);

    auto* frame = new brls::AppletFrame(scroll.frame);
    frame->setTitle(details.name);
    return frame;
}

void ModActivity::buildPriority(brls::Box* content)
{
    content->addView(ui::header("app/details/priority"_i18n, "app/details/priority_hint"_i18n));
    priorityRow = new ui::ListRow(false);
    priorityRow->title->setText("app/details/position"_i18n);
    priorityRow->subtitle->setText("app/details/position_hint"_i18n);
    priorityRow->registerClickAction([this](brls::View*) {
        choosePosition();
        return true;
    });
    content->addView(priorityRow);
    updatePriority();
}

void ModActivity::buildPlugins(brls::Box* content)
{
    if (details.plugins.empty())
        return;

    content->addView(ui::header(brls::getStr("app/details/plugins", details.plugins.size()),
        "app/details/plugins_hint"_i18n));
    for (const std::string& plugin : details.plugins)
        content->addView(ui::paragraph(plugin, 16, true));

    auto* load          = new brls::BooleanCell();
    std::string target  = folder;
    load->init("app/details/load_plugins"_i18n, details.loadPlugins && app::state().modPlugins, [target](bool on) {
        std::string error = core::setModPlugins(target, on);
        if (!error.empty())
            ui::message(brls::getStr("app/settings/save_failed", error));
        app::reload(app::Merge::Unchanged);
    });
    content->addView(load);
    if (!app::state().modPlugins)
        content->addView(ui::paragraph("app/details/plugins_off"_i18n, 16, true));
}

void ModActivity::updatePriority()
{
    std::vector<std::string> order = app::modOrder();
    auto it = std::find_if(order.begin(), order.end(), [&](const std::string& f) { return sameName(f, folder); });
    if (it == order.end())
        priorityRow->setValue("-", false);
    else
        priorityRow->setValue(brls::getStr("app/details/position_value", (it - order.begin()) + 1, order.size()));
}

void ModActivity::choosePosition()
{
    std::vector<std::string> order = app::modOrder();
    std::vector<std::string> labels;
    int current = 0;
    for (size_t i = 0; i < order.size(); i++)
    {
        labels.push_back("#" + std::to_string(i + 1) + "  ·  " + app::modName(order[i]));
        if (sameName(order[i], folder))
            current = (int)i;
    }
    if (labels.empty())
        return;

    std::string target = folder;
    auto* dropdown     = new brls::Dropdown(
        brls::getStr("app/details/position_title", details.name), labels,
        [this, target, current](int selected) {
            if (selected < 0 || selected == current)
                return;
            std::string error = app::moveMod(target, (size_t)selected);
            if (!error.empty())
            {
                ui::message(brls::getStr("app/profiles/save_failed", error));
                return;
            }
            updatePriority();
            buildConflicts();
        },
        current);
    brls::Application::pushActivity(new brls::Activity(dropdown));
}

void ModActivity::buildConflicts()
{
    conflictsBox->clearViews();
    const core::ProfileEntry* entry = app::state().findEntry(folder);
    if (!entry || !entry->enabled)
        return;

    std::vector<app::ActiveConflict> conflicts = app::conflictsOf(folder);
    if (conflicts.empty())
    {
        if (app::state().conflicts.empty())
            return;
        conflictsBox->addView(ui::header("app/details/conflicts"_i18n));
        conflictsBox->addView(ui::paragraph("app/details/no_conflict"_i18n, 16, true));
        return;
    }

    conflictsBox->addView(ui::header(brls::getStr("app/details/conflicts_count", conflicts.size()),
        "app/details/conflicts_hint"_i18n));

    // Several hundred textures would make a very long page.
    const size_t SHOWN = 40;
    for (size_t i = 0; i < conflicts.size() && i < SHOWN; i++)
    {
        const app::ActiveConflict& active = conflicts[i];
        const core::Conflict& conflict    = active.conflict;
        bool wins                         = sameName(active.folders.front(), folder);

        std::vector<std::string> others;
        for (auto& other : active.folders)
            if (!sameName(other, folder))
                others.push_back(brls::getStr("app/common/quoted", app::modName(other)));
        std::string otherNames;
        for (size_t j = 0; j < others.size(); j++)
            otherNames += (j ? ", " : "") + others[j];
        std::string winner = brls::getStr("app/common/quoted", app::modName(active.folders.front()));

        std::string subtitle;
        if (conflict.wholeFile())
            subtitle = wins ? brls::getStr("app/details/file_wins", otherNames) : brls::getStr("app/details/file_loses", winner);
        else
            subtitle = brls::getStr("app/details/values_count", conflict.count) + "  ·  "
                + (wins ? brls::getStr("app/details/values_win", otherNames) : brls::getStr("app/details/values_lose", winner));

        auto* row = new ui::ListRow(false);
        row->title->setText(app::shortFileName(conflict.file));
        row->subtitle->setText(subtitle);
        if (wins)
        {
            row->setValue("app/details/wins"_i18n, true);
        }
        else
        {
            row->value->setText("app/details/loses"_i18n);
            row->value->setTextColor(ui::warningColor());
        }

        // The whole story: full path, order, changed values.
        std::string text = conflict.file + "\n\n";
        for (size_t j = 0; j < active.folders.size(); j++)
            text += std::to_string(j + 1) + ". " + app::modName(active.folders[j]) + "\n";
        if (!conflict.samples.empty())
        {
            std::string samples;
            for (auto& sample : conflict.samples)
                samples += (samples.empty() ? "" : ", ") + sample;
            if (conflict.count > (int)conflict.samples.size())
                samples += ", …";
            text += "\n" + brls::getStr("app/details/values_list", samples);
        }
        row->registerClickAction([text](brls::View*) {
            ui::message(text);
            return true;
        });
        conflictsBox->addView(row);
    }
    if (conflicts.size() > SHOWN)
        conflictsBox->addView(ui::paragraph(brls::getStr("app/details/more_conflicts", conflicts.size() - SHOWN), 16, true));
}

std::vector<std::string> ModActivity::selectedOptions(const core::OptionGroup& group)
{
    std::vector<std::string> selected;
    bool found                = false;
    core::ProfileEntry* entry = app::state().findEntry(folder);
    if (entry)
    {
        for (auto& [name, chosen] : entry->options)
        {
            if (sameName(name, group.name))
            {
                selected = chosen;
                found    = true;
            }
        }
    }
    if (!found)
    {
        for (auto& [name, chosen] : details.iniOptions)
        {
            if (sameName(name, group.name))
            {
                selected = chosen;
                found    = true;
            }
        }
    }
    if (!found)
    {
        for (int index : group.defaults)
            if (index >= 0 && index < (int)group.options.size())
                selected.push_back(group.options[index].first);
    }

    // What the merge makes of it (TkMod::selected_changelogs): options of the
    // group in its own order, one at most in a single choice group, the first
    // one when a choice is required and none was made.
    std::vector<std::string> effective;
    for (auto& option : group.options)
    {
        bool chosen = std::any_of(selected.begin(), selected.end(), [&](const std::string& s) { return sameName(s, option.first); });
        if (chosen && !(group.single() && !effective.empty()))
            effective.push_back(option.first);
    }
    if (effective.empty() && group.required() && !group.options.empty())
        effective.push_back(group.options.front().first);
    return effective;
}

void ModActivity::buildOptions(brls::Box* content)
{
    if (details.groups.empty())
        return;

    for (auto& group : details.groups)
    {
        std::string subtitle = group.single() ? "app/details/single"_i18n : "app/details/multi"_i18n;
        if (group.required())
            subtitle += " · " + "app/details/required"_i18n;
        content->addView(ui::header(group.name, subtitle));
        if (!group.description.empty())
            content->addView(ui::paragraph(group.description, 16, true));

        std::vector<std::string> selected = selectedOptions(group);
        auto isSelected = [&](const std::string& option) {
            return std::any_of(selected.begin(), selected.end(), [&](const std::string& s) { return sameName(s, option); });
        };

        if (group.single())
        {
            auto radios = std::make_shared<std::vector<brls::RadioCell*>>();
            for (auto& [option, description] : group.options)
            {
                auto* cell = new brls::RadioCell();
                cell->title->setText(option);
                cell->setSelected(isSelected(option));
                radios->push_back(cell);
                size_t index                 = radios->size() - 1;
                const core::OptionGroup copy = group;
                std::string name             = option;
                cell->registerClickAction([this, radios, index, copy, name](brls::View*) {
                    bool wasSelected = (*radios)[index]->getSelected();
                    if (wasSelected && copy.required())
                        return true;
                    for (size_t i = 0; i < radios->size(); i++)
                        (*radios)[i]->setSelected(i == index && !wasSelected);
                    select(copy, name, !wasSelected);
                    return true;
                });
                content->addView(cell);
                addOptionDescription(content, description);
            }
        }
        else
        {
            for (auto& [option, description] : group.options)
            {
                auto* cell                   = new brls::BooleanCell();
                const core::OptionGroup copy = group;
                std::string name             = option;
                cell->init(option, isSelected(option), [this, copy, name](bool on) { select(copy, name, on); });
                content->addView(cell);
                addOptionDescription(content, description);
            }
        }
    }
}

void ModActivity::select(const core::OptionGroup& group, const std::string& option, bool on)
{
    core::ProfileEntry* entry = app::state().findEntry(folder);
    if (!entry)
        return;
    std::vector<std::string> selected = selectedOptions(group);
    if (group.single())
        selected.clear();
    selected.erase(std::remove_if(selected.begin(), selected.end(), [&](const std::string& s) { return sameName(s, option); }),
        selected.end());
    if (on)
        selected.push_back(option);

    // Groups are recorded under their own spelling.
    for (auto it = entry->options.begin(); it != entry->options.end();)
        it = sameName(it->first, group.name) ? entry->options.erase(it) : std::next(it);
    entry->options[group.name] = selected;
    saveSelection();
}

void ModActivity::saveSelection()
{
    core::State& state = app::state();
    std::string error  = core::saveProfile(state.profile, state.entries);
    if (!error.empty())
        ui::message(brls::getStr("app/profiles/save_failed", error));
    else
        state.applied = false;
}
