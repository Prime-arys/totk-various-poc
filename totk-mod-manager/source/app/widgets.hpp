#pragma once

#include <cstdint>
#include <functional>
#include <memory>
#include <string>

#include <borealis.hpp>

// Small building blocks shared by the screens.
namespace ui
{

/** A focusable list row: an optional picture, a title, a subtitle, and a
 *  value on the right with an optional note under it. */
class ListRow : public brls::Box
{
  public:
    explicit ListRow(bool withImage, float imageWidth = 128, float imageHeight = 72);

    brls::Image* image = nullptr;
    brls::Label* title;
    brls::Label* subtitle;
    brls::Label* value;
    brls::Label* note;

    void setValue(const std::string& text, bool accent = true);
    /** Empty hides it. */
    void setNote(const std::string& text, NVGcolor color);
};

/** Orange, for what needs a look. */
NVGcolor warningColor();

/** A section title with the line borealis draws under it. */
brls::Header* header(const std::string& title, const std::string& subtitle = "");

/** Wrapping text. */
brls::Label* paragraph(const std::string& text, float fontSize = 18, bool dim = false);

/** A column inside a scrolling frame, padded like borealis' own lists. */
struct ScrollList
{
    brls::ScrollingFrame* frame;
    brls::Box* content;
};
ScrollList scrollList();

/** A modal dialog with a status line and a progress bar, updated from any
 *  thread. */
class Progress
{
  public:
    static std::shared_ptr<Progress> open(const std::string& title, bool cancelable = false);

    /** Thread safe. `fraction` < 0 shows no bar. */
    void update(const std::string& status, float fraction, const std::string& detail = "");
    /** Thread safe. */
    bool cancelled() const { return cancelRequested; }
    /** Interface thread. */
    void close(std::function<void()> then = nullptr);

  private:
    struct Views;
    std::shared_ptr<Views> views;
    bool cancelRequested = false;
};

/** Whether `view` is `ancestor` or inside it. */
bool isInside(brls::View* view, brls::View* ancestor);

/** Focuses the child at `index` of `box` (or the last one), for lists
 *  rebuilt while one of their rows had the focus. */
void focusChild(brls::Box* box, size_t index);

/** A message with an OK button. */
void message(const std::string& text, std::function<void()> then = nullptr);

/** A question: `yes` runs `action`, the other button does nothing. */
void confirm(const std::string& text, const std::string& yes, std::function<void()> action);

std::string formatSize(uint64_t bytes);
std::string formatCount(int64_t count);

} // namespace ui
