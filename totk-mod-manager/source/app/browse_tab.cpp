#include "app/browse_tab.hpp"

#include <algorithm>
#include <memory>

#include "app/gamebanana_activity.hpp"
#include "app/widgets.hpp"
#include "util/images.hpp"
#include "util/worker.hpp"

using namespace brls::literals;

namespace
{
// Kept while the tab is switched away (borealis frees inactive tabs).
std::string searchTerm;
int sortIndex  = 0;
int pageNumber = 1;
std::shared_ptr<gamebanana::Page> cachedPage;
std::string cachedKey;

const gamebanana::Sort SORTS[] = {
    gamebanana::Sort::MostDownloaded, gamebanana::Sort::MostLiked, gamebanana::Sort::MostViewed,
    gamebanana::Sort::Newest,         gamebanana::Sort::LatestUpdated,
};

std::string requestKey()
{
    return searchTerm + "|" + std::to_string(sortIndex) + "|" + std::to_string(pageNumber);
}
} // namespace

BrowseTab::BrowseTab()
    : brls::Box(brls::Axis::COLUMN)
{
    this->setGrow(1);

    ui::ScrollList scroll = ui::scrollList();
    frame                 = scroll.frame;
    list                  = scroll.content;

    auto* controls = new brls::Box(brls::Axis::COLUMN);
    controls->setPadding(20, 40, 0, 40);

    searchCell = new brls::DetailCell();
    searchCell->setText("app/browse/search"_i18n);
    searchCell->setDetailText(searchTerm.empty() ? "app/browse/search_none"_i18n : searchTerm);
    searchCell->registerClickAction([this](brls::View*) {
        brls::Application::getImeManager()->openForText(
            [this](std::string text) {
                searchTerm = text;
                pageNumber = 1;
                searchCell->setDetailText(searchTerm.empty() ? "app/browse/search_none"_i18n : searchTerm);
                load();
            },
            "app/browse/search"_i18n, "app/browse/search_hint"_i18n, 64, searchTerm);
        return true;
    });
    controls->addView(searchCell);

    sortCell = new brls::SelectorCell();
    sortCell->init("app/browse/sort"_i18n,
        { "app/browse/sort_downloads"_i18n, "app/browse/sort_likes"_i18n, "app/browse/sort_views"_i18n,
            "app/browse/sort_newest"_i18n, "app/browse/sort_updated"_i18n },
        sortIndex, [this](int selected) {
            if (selected == sortIndex)
                return;
            sortIndex  = selected;
            pageNumber = 1;
            load();
        });
    controls->addView(sortCell);

    statusLabel = new brls::Label();
    statusLabel->setFontSize(16);
    statusLabel->setMargins(10, 16, 0, 16);
    statusLabel->setTextColor(brls::Application::getTheme()["brls/header/subtitle"]);
    controls->addView(statusLabel);

    this->addView(controls);
    this->addView(frame);

    if (cachedPage && cachedKey == requestKey())
        show(*cachedPage);
    else
        load();
}

void BrowseTab::load()
{
    statusLabel->setText("app/browse/loading"_i18n);
    refocus = refocus || ui::isInside(brls::Application::getCurrentFocus(), list);
    if (refocus)
        brls::Application::giveFocus(searchCell);
    list->clearViews();
    frame->setContentOffsetY(0, false);

    int id           = ++requestId;
    auto result      = std::make_shared<gamebanana::Page>();
    int page         = pageNumber;
    std::string term = searchTerm;
    gamebanana::Sort sort = SORTS[sortIndex];
    std::string key  = requestKey();

    ASYNC_RETAIN
    worker::run([result, page, sort, term]() { *result = gamebanana::fetchPage(page, sort, term); },
        [ASYNC_TOKEN, id, result, key]() {
            ASYNC_RELEASE
            if (id != this->requestId)
                return;
            if (result->error.empty())
            {
                cachedPage = result;
                cachedKey  = key;
            }
            this->show(*result);
        },
        512 * 1024);
}

void BrowseTab::show(const gamebanana::Page& page)
{
    refocus = refocus || ui::isInside(brls::Application::getCurrentFocus(), list);
    list->clearViews();
    if (!page.error.empty())
    {
        statusLabel->setText(brls::getStr("app/browse/error", page.error));
        auto* retry = new ui::ListRow(false);
        retry->title->setText("app/browse/retry"_i18n);
        retry->registerClickAction([this](brls::View*) {
            brls::sync([this]() { load(); });
            return true;
        });
        list->addView(retry);
        return;
    }

    int pages = (page.total + 19) / 20;
    statusLabel->setText(brls::getStr("app/browse/status", page.total, pageNumber, std::max(pages, 1)));

    if (page.records.empty())
        list->addView(ui::paragraph("app/browse/empty"_i18n, 20));

    for (const gamebanana::Record& record : page.records)
    {
        auto* row = new ui::ListRow(true, 176, 99);
        row->title->setText(record.name);
        std::string subtitle = brls::getStr("app/browse/by", record.submitter);
        if (!record.category.empty())
            subtitle += "  ·  " + record.category;
        if (!record.version.empty())
            subtitle += "  ·  v" + record.version;
        row->subtitle->setText(subtitle);
        row->setValue(brls::getStr("app/browse/likes", ui::formatCount(record.likes)), false);
        images::fromUrl(row->image, record.thumbnail);
        int64_t id = record.id;
        row->registerClickAction([id](brls::View*) {
            brls::Application::pushActivity(new GameBananaActivity(id));
            return true;
        });
        list->addView(row);
    }

    if (pageNumber > 1)
    {
        auto* previous = new ui::ListRow(false);
        previous->title->setText("app/browse/previous"_i18n);
        previous->registerClickAction([this](brls::View*) {
            pageNumber--;
            brls::sync([this]() { load(); });
            return true;
        });
        list->addView(previous);
    }
    if (pageNumber < pages)
    {
        auto* next = new ui::ListRow(false);
        next->title->setText("app/browse/next"_i18n);
        next->registerClickAction([this](brls::View*) {
            pageNumber++;
            brls::sync([this]() { load(); });
            return true;
        });
        list->addView(next);
    }

    if (refocus)
    {
        refocus = false;
        ui::focusChild(list, 0);
    }
}
