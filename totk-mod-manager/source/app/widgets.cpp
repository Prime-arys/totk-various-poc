#include "app/widgets.hpp"

#include <algorithm>
#include <cmath>
#include <mutex>

using namespace brls::literals;

namespace ui
{

ListRow::ListRow(bool withImage, float imageWidth, float imageHeight)
    : brls::Box(brls::Axis::ROW)
{
    this->setFocusable(true);
    this->setAlignItems(brls::AlignItems::CENTER);
    this->setPadding(10, 16, 10, 16);
    this->setMinHeight(withImage ? imageHeight + 20 : 70);
    this->setCornerRadius(4);

    if (withImage)
    {
        image = new brls::Image();
        image->setWidth(imageWidth);
        image->setHeight(imageHeight);
        image->setScalingType(brls::ImageScalingType::FILL);
        image->setCornerRadius(4);
        image->setMarginRight(20);
        image->setBackgroundColor(nvgRGBA(255, 255, 255, 18));
        this->addView(image);
    }

    auto* texts = new brls::Box(brls::Axis::COLUMN);
    texts->setGrow(1);
    texts->setShrink(1);
    texts->setJustifyContent(brls::JustifyContent::CENTER);

    title = new brls::Label();
    title->setFontSize(22);
    title->setSingleLine(true);
    texts->addView(title);

    subtitle = new brls::Label();
    subtitle->setFontSize(16);
    subtitle->setSingleLine(true);
    subtitle->setMarginTop(6);
    subtitle->setTextColor(brls::Application::getTheme()["brls/header/subtitle"]);
    texts->addView(subtitle);
    this->addView(texts);

    auto* side = new brls::Box(brls::Axis::COLUMN);
    side->setAlignItems(brls::AlignItems::FLEX_END);
    side->setJustifyContent(brls::JustifyContent::CENTER);
    side->setMarginLeft(20);
    side->setShrink(0);

    value = new brls::Label();
    value->setFontSize(20);
    value->setSingleLine(true);
    value->setHorizontalAlign(brls::HorizontalAlign::RIGHT);
    side->addView(value);

    note = new brls::Label();
    note->setFontSize(15);
    note->setSingleLine(true);
    note->setMarginTop(6);
    note->setHorizontalAlign(brls::HorizontalAlign::RIGHT);
    note->setVisibility(brls::Visibility::GONE);
    side->addView(note);
    this->addView(side);
}

void ListRow::setNote(const std::string& text, NVGcolor color)
{
    note->setText(text);
    note->setTextColor(color);
    note->setVisibility(text.empty() ? brls::Visibility::GONE : brls::Visibility::VISIBLE);
}

NVGcolor warningColor()
{
    return nvgRGB(255, 190, 70);
}

void ListRow::setValue(const std::string& text, bool accent)
{
    value->setText(text);
    value->setTextColor(accent ? brls::Application::getTheme()["brls/list/listItem_value_color"]
                               : brls::Application::getTheme()["brls/text_disabled"]);
}

brls::Header* header(const std::string& title, const std::string& subtitle)
{
    auto* view = new brls::Header();
    view->setTitle(title);
    if (!subtitle.empty())
        view->setSubtitle(subtitle);
    return view;
}

brls::Label* paragraph(const std::string& text, float fontSize, bool dim)
{
    auto* label = new brls::Label();
    label->setText(text);
    label->setFontSize(fontSize);
    label->setIsWrapping(true);
    label->setMarginBottom(12);
    if (dim)
        label->setTextColor(brls::Application::getTheme()["brls/header/subtitle"]);
    return label;
}

ScrollList scrollList()
{
    ScrollList list;
    list.frame = new brls::ScrollingFrame();
    list.frame->setGrow(1);
    list.frame->setScrollingBehavior(brls::ScrollingBehavior::CENTERED);

    list.content = new brls::Box(brls::Axis::COLUMN);
    list.content->setAlignItems(brls::AlignItems::STRETCH);
    list.content->setPadding(20, 40, 40, 40);
    list.frame->setContentView(list.content);
    return list;
}

struct Progress::Views
{
    std::mutex lock;
    bool alive = true;
    brls::Dialog* dialog  = nullptr;
    brls::Label* status   = nullptr;
    brls::Label* detail   = nullptr;
    brls::Box* track      = nullptr;
    brls::Rectangle* fill = nullptr;
};

static const float TRACK_WIDTH = 640;

std::shared_ptr<Progress> Progress::open(const std::string& title, bool cancelable)
{
    auto progress   = std::make_shared<Progress>();
    progress->views = std::make_shared<Views>();

    auto* box = new brls::Box(brls::Axis::COLUMN);
    box->setAlignItems(brls::AlignItems::CENTER);
    box->setPadding(50, 60, 40, 60);

    auto* heading = new brls::Label();
    heading->setText(title);
    heading->setFontSize(26);
    heading->setMarginBottom(26);
    box->addView(heading);

    progress->views->status = new brls::Label();
    progress->views->status->setFontSize(20);
    progress->views->status->setHorizontalAlign(brls::HorizontalAlign::CENTER);
    progress->views->status->setMarginBottom(18);
    box->addView(progress->views->status);

    progress->views->track = new brls::Box(brls::Axis::ROW);
    progress->views->track->setWidth(TRACK_WIDTH);
    progress->views->track->setHeight(10);
    progress->views->track->setCornerRadius(5);
    progress->views->track->setBackgroundColor(nvgRGBA(255, 255, 255, 40));
    progress->views->fill = new brls::Rectangle(brls::Application::getTheme()["brls/accent"]);
    progress->views->fill->setWidth(0);
    progress->views->fill->setHeight(10);
    progress->views->fill->setCornerRadius(5);
    progress->views->track->addView(progress->views->fill);
    box->addView(progress->views->track);

    progress->views->detail = new brls::Label();
    progress->views->detail->setFontSize(16);
    progress->views->detail->setMarginTop(16);
    progress->views->detail->setSingleLine(true);
    progress->views->detail->setTextColor(brls::Application::getTheme()["brls/header/subtitle"]);
    box->addView(progress->views->detail);

    // Something to hold the focus. Without it, closing a dialog opened on top
    // of this one (a question asked during the job) leaves borealis' focus on
    // that dialog's button, which is then freed.
    auto* anchor = new brls::Box();
    anchor->setFocusable(true);
    anchor->setHideHighlight(true);
    anchor->setWidth(1);
    anchor->setHeight(1);
    box->addView(anchor);

    progress->views->dialog = new brls::Dialog(box);
    progress->views->dialog->setCancelable(false);
    if (cancelable)
    {
        std::weak_ptr<Progress> weak = progress;
        std::shared_ptr<Views> target = progress->views;
        progress->views->dialog->addButton("app/common/cancel"_i18n, [weak, target]() {
            // The dialog closes itself after a button press.
            std::lock_guard<std::mutex> guard(target->lock);
            target->alive = false;
            if (auto strong = weak.lock())
                strong->cancelRequested = true;
        });
    }
    progress->views->dialog->open();
    return progress;
}

void Progress::update(const std::string& status, float fraction, const std::string& detail)
{
    std::shared_ptr<Views> target = views;
    brls::sync([target, status, fraction, detail]() {
        std::lock_guard<std::mutex> guard(target->lock);
        if (!target->alive)
            return;
        target->status->setText(status);
        target->detail->setText(detail);
        if (fraction < 0)
        {
            target->track->setVisibility(brls::Visibility::INVISIBLE);
        }
        else
        {
            target->track->setVisibility(brls::Visibility::VISIBLE);
            target->fill->setWidth(std::round(TRACK_WIDTH * std::min(1.0f, fraction)));
        }
    });
}

void Progress::close(std::function<void()> then)
{
    std::shared_ptr<Views> target = views;
    brls::sync([target, then]() {
        {
            std::lock_guard<std::mutex> guard(target->lock);
            if (!target->alive)
            {
                // Already closed by its cancel button.
                if (then)
                    then();
                return;
            }
            target->alive = false;
        }
        target->dialog->close([then]() {
            if (then)
                then();
        });
    });
}

bool isInside(brls::View* view, brls::View* ancestor)
{
    for (; view; view = view->getParent())
        if (view == ancestor)
            return true;
    return false;
}

void focusChild(brls::Box* box, size_t index)
{
    auto& children = box->getChildren();
    if (children.empty())
        return;
    size_t start = std::min(index, children.size() - 1);
    // That row, or the nearest focusable one before it, or after it.
    for (size_t i = start + 1; i-- > 0;)
    {
        if (children[i]->isFocusable())
        {
            brls::Application::giveFocus(children[i]);
            return;
        }
    }
    for (size_t i = start + 1; i < children.size(); i++)
    {
        if (children[i]->isFocusable())
        {
            brls::Application::giveFocus(children[i]);
            return;
        }
    }
}

void message(const std::string& text, std::function<void()> then)
{
    auto* dialog = new brls::Dialog(text);
    dialog->addButton("hints/ok"_i18n, [then]() {
        if (then)
            then();
    });
    dialog->open();
}

void confirm(const std::string& text, const std::string& yes, std::function<void()> action)
{
    auto* dialog = new brls::Dialog(text);
    dialog->addButton("app/common/cancel"_i18n, []() {});
    dialog->addButton(yes, [action]() { action(); });
    dialog->open();
}

std::string formatSize(uint64_t bytes)
{
    char number[32];
    std::string unit;
    if (bytes >= 1024ull * 1024 * 1024)
    {
        std::snprintf(number, sizeof(number), "%.1f", bytes / (1024.0 * 1024 * 1024));
        unit = "app/units/gb"_i18n;
    }
    else if (bytes >= 1024 * 1024)
    {
        std::snprintf(number, sizeof(number), "%.1f", bytes / (1024.0 * 1024));
        unit = "app/units/mb"_i18n;
    }
    else if (bytes >= 1024)
    {
        std::snprintf(number, sizeof(number), "%.0f", bytes / 1024.0);
        unit = "app/units/kb"_i18n;
    }
    else
    {
        std::snprintf(number, sizeof(number), "%llu", (unsigned long long)bytes);
        unit = "app/units/b"_i18n;
    }
    return std::string(number) + " " + unit;
}

std::string formatCount(int64_t count)
{
    char text[32];
    if (count >= 1000000)
        std::snprintf(text, sizeof(text), "%.1fM", count / 1000000.0);
    else if (count >= 1000)
        std::snprintf(text, sizeof(text), "%.1fk", count / 1000.0);
    else
        std::snprintf(text, sizeof(text), "%lld", (long long)count);
    return text;
}

} // namespace ui
