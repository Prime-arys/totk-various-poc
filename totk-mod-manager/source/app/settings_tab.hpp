#pragma once

#include <borealis.hpp>

// Merger settings (config.ini), boost mode, what is installed, and
// housekeeping.
class SettingsTab : public brls::Box
{
  public:
    SettingsTab();

    static brls::View* create() { return new SettingsTab(); }

  private:
    /** Boost mode, and sys-clk's clocks when it runs. */
    void buildBoost(brls::Box* list);
};
