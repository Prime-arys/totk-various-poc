#pragma once

#include <borealis.hpp>

// Tabs: mods of the active profile, profiles, GameBanana, settings. X applies
// the profile from anywhere, + leaves.
class MainActivity : public brls::Activity
{
  public:
    brls::View* createContentView() override;
    void onContentAvailable() override;
};
