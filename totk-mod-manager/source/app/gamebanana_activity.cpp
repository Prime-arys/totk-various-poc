#include "app/gamebanana_activity.hpp"

#include <ctime>

#include "app/installer.hpp"
#include "app/widgets.hpp"
#include "util/images.hpp"
#include "util/worker.hpp"

using namespace brls::literals;

namespace
{
std::string formatDate(int64_t timestamp)
{
    if (timestamp <= 0)
        return "";
    std::time_t time = (std::time_t)timestamp;
    char text[16];
    std::strftime(text, sizeof(text), "%d/%m/%Y", std::gmtime(&time));
    return text;
}
} // namespace

GameBananaActivity::GameBananaActivity(int64_t id)
    : id(id)
{
}

GameBananaActivity::~GameBananaActivity()
{
    *alive = false;
}

brls::View* GameBananaActivity::createContentView()
{
    ui::ScrollList scroll = ui::scrollList();
    content               = scroll.content;
    content->addView(ui::paragraph("app/browse/loading"_i18n, 20, true));

    frame = new brls::AppletFrame(scroll.frame);
    frame->setTitle("GameBanana");
    return frame;
}

void GameBananaActivity::onContentAvailable()
{
    auto submission = std::make_shared<gamebanana::Submission>();
    int64_t target  = id;
    std::shared_ptr<bool> living = alive;
    worker::run([submission, target]() { *submission = gamebanana::fetchSubmission(target); },
        [this, living, submission]() {
            if (*living)
                show(submission);
        },
        512 * 1024);
}

void GameBananaActivity::show(std::shared_ptr<gamebanana::Submission> submission)
{
    content->clearViews();
    if (!submission->error.empty())
    {
        content->addView(ui::paragraph(brls::getStr("app/browse/error", submission->error), 20));
        return;
    }
    frame->setTitle(submission->name);

    auto* top = new brls::Box(brls::Axis::ROW);
    top->setMarginBottom(20);

    auto* image = new brls::Image();
    image->setWidth(480);
    image->setHeight(270);
    image->setScalingType(brls::ImageScalingType::FIT);
    image->setCornerRadius(6);
    image->setBackgroundColor(nvgRGBA(255, 255, 255, 18));
    image->setMarginRight(30);
    if (!submission->images.empty())
        images::fromUrl(image, submission->images.front());
    top->addView(image);

    auto* facts = new brls::Box(brls::Axis::COLUMN);
    facts->setGrow(1);
    facts->setShrink(1);
    auto* name = new brls::Label();
    name->setText(submission->name);
    name->setFontSize(28);
    name->setIsWrapping(true);
    name->setMarginBottom(14);
    facts->addView(name);
    facts->addView(ui::paragraph(brls::getStr("app/browse/by", submission->submitter), 18, true));
    if (!submission->category.empty())
        facts->addView(ui::paragraph(submission->category, 18, true));
    if (!submission->version.empty())
        facts->addView(ui::paragraph(brls::getStr("app/common/labelled", "app/details/version"_i18n, submission->version), 18, true));
    facts->addView(ui::paragraph(brls::getStr("app/gamebanana/stats", ui::formatCount(submission->likes),
                                     ui::formatCount(submission->downloads), ui::formatCount(submission->views)),
        18, true));
    facts->addView(ui::paragraph(submission->profileUrl, 16, true));
    top->addView(facts);
    content->addView(top);

    if (submission->gameId != 0 && submission->gameId != gamebanana::GAME_ID)
        content->addView(ui::paragraph("app/gamebanana/other_game"_i18n, 20));

    content->addView(ui::header("app/gamebanana/files"_i18n, "app/gamebanana/files_hint"_i18n));
    if (submission->files.empty())
        content->addView(ui::paragraph("app/gamebanana/no_files"_i18n, 18, true));

    for (size_t index = 0; index < submission->files.size(); index++)
    {
        const gamebanana::File& file = submission->files[index];
        auto* row = new ui::ListRow(false);
        row->title->setText(file.name);
        std::string subtitle = ui::formatSize(file.size);
        std::string date     = formatDate(file.added);
        if (!date.empty())
            subtitle += "  ·  " + date;
        subtitle += "  ·  " + brls::getStr("app/gamebanana/downloads", ui::formatCount(file.downloads));
        if (!file.description.empty())
            subtitle += "  ·  " + file.description;
        row->subtitle->setText(subtitle);
        if (file.recommended)
            row->setValue("app/gamebanana/recommended"_i18n, true);
        else if (file.archived)
            row->setValue("app/gamebanana/archived"_i18n, false);

        row->registerClickAction([submission, index](brls::View*) {
            const gamebanana::File& chosen = submission->files[index];
            ui::confirm(brls::getStr("app/gamebanana/install_confirm", chosen.name, ui::formatSize(chosen.size)),
                "app/gamebanana/install"_i18n, [submission, index]() {
                    brls::sync([submission, index]() { installer::install(*submission, submission->files[index]); });
                });
            return true;
        });
        content->addView(row);
    }

    if (!submission->text.empty())
    {
        content->addView(ui::header("app/details/description"_i18n));
        std::string text = submission->text;
        if (text.size() > 3000)
            text = text.substr(0, 3000) + "…";
        content->addView(ui::paragraph(text, 18));
    }

    ui::focusChild(content, 0);
}
