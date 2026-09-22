#pragma once

#include <string>

#include <borealis.hpp>

#include "app/widgets.hpp"
#include "core/core.hpp"

// One installed mod: its place in the active profile, its options (TKMM
// packages), the plugins it ships, its conflicts with other mods, its
// description, and removing it.
class ModActivity : public brls::Activity
{
  public:
    explicit ModActivity(const std::string& folder);

    brls::View* createContentView() override;

  private:
    void buildPriority(brls::Box* content);
    void updatePriority();
    void choosePosition();
    /** Fills conflictsBox for the profile as it is now. */
    void buildConflicts();
    void buildOptions(brls::Box* content);
    /** The plugins the mod ships, and whether they are loaded in the game. */
    void buildPlugins(brls::Box* content);
    /** The selection currently in effect for a group: the profile's, the
     *  mod.ini's, or the package's defaults. */
    std::vector<std::string> selectedOptions(const core::OptionGroup& group);
    void select(const core::OptionGroup& group, const std::string& option, bool on);
    void saveSelection();

    std::string folder;
    core::ModDetails details;
    ui::ListRow* priorityRow = nullptr;
    brls::Box* conflictsBox  = nullptr;
};
