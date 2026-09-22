#pragma once

#include <string>

#include <borealis.hpp>

// Profiles: named mod lists. Create, switch, rename, duplicate, delete.
class ProfilesTab : public brls::Box
{
  public:
    ProfilesTab();
    ~ProfilesTab() override;

    static brls::View* create() { return new ProfilesTab(); }

  private:
    void build();
    void openMenu(const std::string& name);
    void askName(const std::string& title, const std::string& initial, std::function<void(std::string)> then);
    void createProfile(const std::string& name, bool copyActive);

    brls::Box* list;
    brls::Event<>::Subscription subscription;
};
