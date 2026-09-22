#pragma once

#include <cstdint>
#include <memory>

#include <borealis.hpp>

#include "net/gamebanana.hpp"

// A GameBanana mod page: pictures, description, and its files to install.
class GameBananaActivity : public brls::Activity
{
  public:
    explicit GameBananaActivity(int64_t id);
    ~GameBananaActivity() override;

    brls::View* createContentView() override;
    void onContentAvailable() override;

  private:
    void show(std::shared_ptr<gamebanana::Submission> submission);

    int64_t id;
    brls::AppletFrame* frame = nullptr;
    brls::Box* content       = nullptr;
    /** False once the page is closed, for the download still running. */
    std::shared_ptr<bool> alive = std::make_shared<bool>(true);
};
