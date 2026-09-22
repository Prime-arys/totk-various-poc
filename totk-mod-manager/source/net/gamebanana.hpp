#pragma once

#include <cstdint>
#include <string>
#include <vector>

// GameBanana's API for Tears of the Kingdom mods, as TKMM uses it
// (TkSharp.Extensions.GameBanana): apiv12 feeds and profile pages.
namespace gamebanana
{

constexpr int GAME_ID = 7617;

enum class Sort
{
    MostDownloaded,
    MostLiked,
    MostViewed,
    Newest,
    LatestUpdated,
};

const char* sortApiName(Sort sort);

struct Record
{
    int64_t id = 0;
    std::string name;
    std::string submitter;
    std::string version;
    std::string category;
    std::string thumbnail; // small JPEG, may be empty
    int64_t likes = 0;
    int64_t views = 0;
    int64_t updated = 0;
};

struct Page
{
    std::vector<Record> records;
    int total    = 0;
    bool last    = true;
    std::string error;
};

/** `page` starts at 1. `search` is ignored below 3 characters, like TKMM. */
Page fetchPage(int page, Sort sort, const std::string& search);

struct File
{
    int64_t id = 0;
    std::string name;
    std::string description;
    std::string url;
    std::string md5;
    uint64_t size      = 0;
    int64_t downloads  = 0;
    int64_t added      = 0;
    bool archived      = false;
    /** Marked by the author for TKMM ("TotkModManager" integration). */
    bool recommended   = false;
};

struct Submission
{
    int64_t id = 0;
    std::string name;
    std::string version;
    std::string submitter;
    std::string category;
    std::string text; // plain text, from the page's HTML
    std::string profileUrl;
    std::vector<std::string> images;      // 530 px JPEGs
    std::vector<std::string> fullImages;  // originals
    std::vector<File> files;
    int64_t gameId    = 0;
    int64_t likes     = 0;
    int64_t views     = 0;
    int64_t downloads = 0;
    std::string error;
};

Submission fetchSubmission(int64_t id);

/** Best effort HTML -> text for descriptions. */
std::string htmlToText(const std::string& html);

} // namespace gamebanana
