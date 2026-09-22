#include "app/installer.hpp"

#include <sys/stat.h>

#include <memory>
#include <strings.h>

#include <borealis.hpp>
#include <switch.h>

#include "app/app.hpp"
#include "app/widgets.hpp"
#include "core/core.hpp"
#include "net/http.hpp"
#include "util/archive.hpp"
#include "util/log.hpp"
#include "util/md5.hpp"
#include "util/worker.hpp"

using namespace brls::literals;

namespace installer
{

namespace
{
    const std::string DOWNLOADS_SD   = "sd:/totk/downloads";
    const std::string DOWNLOADS_SDMC = "sdmc:/totk/downloads";

    struct Job
    {
        gamebanana::Submission submission;
        gamebanana::File file;
        std::string sdDir;   // sd:/totk/downloads/<file id>
        std::string sdmcDir; // the same, for C++ file access
        std::string thumbnail;
        std::vector<core::Candidate> candidates;
        std::string error;
    };

    bool endsWith(const std::string& text, const char* suffix)
    {
        size_t length = std::char_traits<char>::length(suffix);
        return text.size() >= length && strcasecmp(text.c_str() + text.size() - length, suffix) == 0;
    }

    u64 milliseconds()
    {
        return armTicksToNs(armGetSystemTick()) / 1000000;
    }

    /** Downloads, checks and unpacks; fills job->candidates. Worker thread. */
    void prepare(Job* job, ui::Progress* progress)
    {
        core::removeTree(job->sdDir);
        mkdir("sdmc:/totk", 0777);
        mkdir(DOWNLOADS_SDMC.c_str(), 0777);
        mkdir(job->sdmcDir.c_str(), 0777);

        std::string archivePath = job->sdmcDir + "/" + job->file.name;
        u64 last                = 0;
        std::string error = http::download(job->file.url, archivePath, [&](uint64_t done, uint64_t total) {
            if (progress->cancelled())
                return false;
            if (total == 0)
                total = job->file.size;
            u64 now = milliseconds();
            if (now - last >= 150)
            {
                last = now;
                progress->update(brls::getStr("app/install/downloading", ui::formatSize(done),
                                     total ? ui::formatSize(total) : "?"),
                    total ? (float)done / (float)total : -1, job->file.name);
            }
            return true;
        });
        if (!error.empty())
        {
            job->error = error == "cancelled" ? "" : brls::getStr("app/install/download_failed", error);
            return;
        }

        if (!job->file.md5.empty())
        {
            progress->update("app/install/checking"_i18n, -1, job->file.name);
            std::string digest = md5::ofFile(archivePath);
            if (strcasecmp(digest.c_str(), job->file.md5.c_str()) != 0)
            {
                job->error = "app/install/checksum"_i18n;
                return;
            }
        }

        if (!job->submission.images.empty())
        {
            std::string thumbnail = job->sdmcDir + "/thumbnail.jpg";
            if (http::download(job->submission.images.front(), thumbnail, nullptr).empty())
                job->thumbnail = job->sdDir + "/thumbnail.jpg";
        }

        std::string searchDir = job->sdDir;
        if (endsWith(job->file.name, ".tkcl"))
        {
            // A package is a mod as it is.
        }
        else if (unpack::isArchive(job->file.name))
        {
            progress->update("app/install/extracting"_i18n, 0, job->file.name);
            std::string extracted = job->sdmcDir + "/extracted";
            last = 0;
            error = unpack::extract(archivePath, extracted, [&](uint64_t done, uint64_t total) {
                if (progress->cancelled())
                    return false;
                u64 now = milliseconds();
                if (now - last >= 150)
                {
                    last = now;
                    progress->update(brls::getStr("app/install/extracting_size", ui::formatSize(done)),
                        total ? (float)done / (float)total : -1, job->file.name);
                }
                return true;
            });
            if (!error.empty())
            {
                job->error = error == "cancelled" ? "" : brls::getStr("app/install/extract_failed", error);
                return;
            }
            remove(archivePath.c_str());
            searchDir = job->sdDir + "/extracted";
        }
        else
        {
            job->error = brls::getStr("app/install/unsupported", job->file.name);
            return;
        }

        progress->update("app/install/analyzing"_i18n, -1, "");
        job->candidates = core::findMods(searchDir);
    }

    std::string lastComponent(const std::string& label)
    {
        std::string trimmed = label;
        while (!trimmed.empty() && trimmed.back() == '/')
            trimmed.pop_back();
        size_t slash = trimmed.rfind('/');
        std::string name = slash == std::string::npos ? trimmed : trimmed.substr(slash + 1);
        if (endsWith(name, ".tkcl"))
            name = name.substr(0, name.size() - 5);
        return name;
    }

    void finish(std::shared_ptr<Job> job, const core::Candidate& candidate)
    {
        bool several = job->candidates.size() > 1;
        std::string label = lastComponent(candidate.label);
        std::string name = job->submission.name;
        if (several && !label.empty())
            name += " - " + label;
        std::string folder = core::folderName(name);

        core::InstallInfo info;
        info.name        = name;
        info.version     = job->submission.version;
        info.author      = job->submission.submitter;
        info.description = job->submission.text.size() > 4000 ? job->submission.text.substr(0, 4000) + "…"
                                                                : job->submission.text;
        info.url         = job->submission.profileUrl;
        info.thumbnail   = job->thumbnail;

        auto error = std::make_shared<std::string>();
        worker::run(
            [job, candidate, folder, info, error]() {
                try
                {
                    std::string path = core::install(candidate, folder, info);
                    applog::write("installed " + candidate.path + " as " + path);
                }
                catch (const std::exception& e)
                {
                    *error = e.what();
                }
                core::removeTree(job->sdDir);
            },
            [job, candidate, folder, error]() {
                app::reload();
                if (!error->empty())
                {
                    ui::message(brls::getStr("app/install/failed", *error));
                    return;
                }
                std::string text = brls::getStr("app/install/done", folder, app::state().profile);
                if (candidate.hasCode)
                    text += "\n\n" + "app/install/has_code"_i18n;
                ui::message(text);
            });
    }

    void choose(std::shared_ptr<Job> job)
    {
        if (!job->error.empty())
        {
            core::removeTree(job->sdDir);
            ui::message(job->error);
            return;
        }
        if (job->candidates.empty())
        {
            core::removeTree(job->sdDir);
            ui::message("app/install/nothing"_i18n);
            return;
        }
        if (job->candidates.size() == 1)
        {
            finish(job, job->candidates.front());
            return;
        }

        std::vector<std::string> labels;
        for (auto& candidate : job->candidates)
        {
            std::string label = candidate.label.empty() ? job->file.name : candidate.label;
            labels.push_back(label + "  (" + app::kindLabel(candidate.kind) + ", " + ui::formatSize(candidate.size) + ")");
        }
        auto* dropdown = new brls::Dropdown(
            "app/install/choose"_i18n, labels,
            [job](int selected) {
                if (selected < 0 || selected >= (int)job->candidates.size())
                    return;
                core::Candidate candidate = job->candidates[selected];
                brls::sync([job, candidate]() { finish(job, candidate); });
            },
            -1,
            [job](int selected) {
                // Dismissed without a choice.
                if (selected < 0)
                    core::removeTree(job->sdDir);
            });
        brls::Application::pushActivity(new brls::Activity(dropdown));
    }
} // namespace

void install(const gamebanana::Submission& submission, const gamebanana::File& file)
{
    auto job        = std::make_shared<Job>();
    job->submission = submission;
    job->file       = file;
    job->sdDir      = DOWNLOADS_SD + "/" + std::to_string(file.id);
    job->sdmcDir    = DOWNLOADS_SDMC + "/" + std::to_string(file.id);

    auto progress = ui::Progress::open(brls::getStr("app/install/title", submission.name), true);
    progress->update("app/install/starting"_i18n, 0, file.name);
    worker::run([job, progress]() { prepare(job.get(), progress.get()); },
        [job, progress]() {
            bool cancelled = progress->cancelled();
            progress->close([job, cancelled]() {
                if (cancelled)
                {
                    core::removeTree(job->sdDir);
                    return;
                }
                choose(job);
            });
        });
}

} // namespace installer
