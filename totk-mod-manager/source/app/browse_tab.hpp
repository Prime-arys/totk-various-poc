#pragma once

#include <string>

#include <borealis.hpp>

#include "net/gamebanana.hpp"

// GameBanana's TotK mods: search, sort, pages; a mod opens its page.
class BrowseTab : public brls::Box
{
  public:
    BrowseTab();

    static brls::View* create() { return new BrowseTab(); }

  private:
    void load();
    void show(const gamebanana::Page& page);

    brls::Box* list;
    brls::DetailCell* searchCell;
    brls::SelectorCell* sortCell;
    brls::Label* statusLabel;
    brls::ScrollingFrame* frame;
    int requestId = 0;
    /** The list had the focus when it was cleared. */
    bool refocus = false;
};
