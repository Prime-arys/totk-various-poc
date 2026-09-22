#pragma once

#include <string>
#include <vector>

#include <borealis.hpp>

#include "app/widgets.hpp"

// The active profile's mods, in priority order: enable, reorder, open.
class ModsTab : public brls::Box
{
  public:
    ModsTab();
    ~ModsTab() override;

    static brls::View* create() { return new ModsTab(); }

    /** Back from a mod's page: its options may have changed. */
    void willAppear(bool resetState = false) override;
    void willDisappear(bool resetState = false) override;

  private:
    void build(const std::string& focusFolder = "");
    void updateStatus();
    /** Title, number, value and conflict note of the row at `index`. */
    void updateRow(size_t index);
    void toggle(const std::string& folder);
    /** Moves a mod `delta` places, towards the top when negative. */
    void move(const std::string& folder, int delta);
    /** Picks a mod up: the D-pad then moves it, A or B puts it down. */
    void grab(const std::string& folder);
    void drop();
    /** Puts the rows in the profile's order, moving them rather than
     *  building new ones. */
    void followOrder();
    void save();

    brls::Label* profileLabel;
    brls::Label* statusLabel;
    brls::Label* hintLabel;
    brls::Box* list;
    brls::ScrollingFrame* frame;
    /** Installed mods, in profile order, and their rows. */
    std::vector<std::string> order;
    std::vector<ui::ListRow*> rows;
    /** The mod being moved, if any. */
    std::string grabbed;
    bool dirty = false;
    brls::Event<>::Subscription subscription;
    brls::Event<>::Subscription orderSubscription;
};
