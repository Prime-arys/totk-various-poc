#include "net/gamebanana.hpp"

#include <cstdlib>
#include <cstring>
#include <strings.h>

#include <borealis/extern/nlohmann/json.hpp>

#include "net/http.hpp"

using json = nlohmann::json;

namespace gamebanana
{

namespace
{
    const char* ROOT = "https://gamebanana.com/apiv12";

    std::string str(const json& object, const char* key)
    {
        auto found = object.find(key);
        return found != object.end() && found->is_string() ? found->get<std::string>() : std::string();
    }

    int64_t number(const json& object, const char* key)
    {
        auto found = object.find(key);
        if (found == object.end())
            return 0;
        if (found->is_number())
            return found->get<int64_t>();
        if (found->is_string())
            return std::strtoll(found->get<std::string>().c_str(), nullptr, 10);
        return 0;
    }

    bool flag(const json& object, const char* key)
    {
        auto found = object.find(key);
        return found != object.end() && found->is_boolean() && found->get<bool>();
    }

    /** "[" and "]" spelled out: libcurl sends URLs as they are. */
    std::string filter(const std::string& name)
    {
        return "_aFilters%5B" + name + "%5D";
    }

    /** First screenshot of a submission, at `sizeKey` ("_sFile220"...). */
    std::string image(const json& media, const char* sizeKey)
    {
        for (auto& picture : media.value("_aImages", json::array()))
        {
            std::string base = str(picture, "_sBaseUrl");
            std::string file = str(picture, sizeKey);
            if (!base.empty() && !file.empty())
                return base + "/" + file;
        }
        return "";
    }
} // namespace

const char* sortApiName(Sort sort)
{
    switch (sort)
    {
        case Sort::MostLiked:
            return "Generic_MostLiked";
        case Sort::MostViewed:
            return "Generic_MostViewed";
        case Sort::Newest:
            return "Generic_Newest";
        case Sort::LatestUpdated:
            return "Generic_LatestUpdated";
        case Sort::MostDownloaded:
        default:
            return "Generic_MostDownloaded";
    }
}

Page fetchPage(int page, Sort sort, const std::string& search)
{
    const int perPage = 20;
    std::string url = std::string(ROOT) + "/Mod/Index?" + filter("Generic_Game") + "=" + std::to_string(GAME_ID) + "&"
        + filter("Generic_ContentRatings") + "=-&_nPage=" + std::to_string(page) + "&_sSort=" + sortApiName(sort)
        + "&_nPerpage=" + std::to_string(perPage);
    if (search.size() > 2)
        url += "&" + filter("Generic_Name") + "=contains," + http::escape(search);

    Page result;
    http::Response response = http::get(url);
    if (!response.ok())
    {
        result.error = response.error.empty() ? "HTTP " + std::to_string(response.status) : response.error;
        return result;
    }
    json root = json::parse(response.body, nullptr, false);
    if (!root.is_object())
    {
        result.error = "invalid answer from GameBanana";
        return result;
    }

    const json& metadata = root.value("_aMetadata", json::object());
    result.total = (int)number(metadata, "_nRecordCount");
    result.last  = flag(metadata, "_bIsComplete");

    for (auto& entry : root.value("_aRecords", json::array()))
    {
        // TKMM hides these as well.
        if (flag(entry, "_bIsObsolete") || flag(entry, "_bHasContentRatings"))
            continue;
        Record record;
        record.id        = number(entry, "_idRow");
        record.name      = str(entry, "_sName");
        record.version   = str(entry, "_sVersion");
        record.submitter = str(entry.value("_aSubmitter", json::object()), "_sName");
        record.category  = str(entry.value("_aRootCategory", json::object()), "_sName");
        record.thumbnail = image(entry.value("_aPreviewMedia", json::object()), "_sFile220");
        record.likes     = number(entry, "_nLikeCount");
        record.views     = number(entry, "_nViewCount");
        record.updated   = number(entry, "_tsDateUpdated");
        result.records.push_back(std::move(record));
    }
    if (result.records.empty() && root.value("_aRecords", json::array()).empty())
        result.last = true;
    return result;
}

Submission fetchSubmission(int64_t id)
{
    Submission result;
    http::Response response = http::get(std::string(ROOT) + "/Mod/" + std::to_string(id) + "/ProfilePage");
    if (!response.ok())
    {
        result.error = response.error.empty() ? "HTTP " + std::to_string(response.status) : response.error;
        return result;
    }
    json root = json::parse(response.body, nullptr, false);
    if (!root.is_object())
    {
        result.error = "invalid answer from GameBanana";
        return result;
    }

    result.id         = number(root, "_idRow");
    result.name       = str(root, "_sName");
    result.version    = str(root, "_sVersion");
    result.submitter  = str(root.value("_aSubmitter", json::object()), "_sName");
    result.category   = str(root.value("_aCategory", json::object()), "_sName");
    result.profileUrl = str(root, "_sProfileUrl");
    result.text       = htmlToText(str(root, "_sText"));
    result.gameId     = number(root.value("_aGame", json::object()), "_idRow");
    result.likes      = number(root, "_nLikeCount");
    result.views      = number(root, "_nViewCount");
    result.downloads  = number(root, "_nDownloadCount");

    for (auto& picture : root.value("_aPreviewMedia", json::object()).value("_aImages", json::array()))
    {
        std::string base = str(picture, "_sBaseUrl");
        std::string medium = str(picture, "_sFile530");
        std::string full = str(picture, "_sFile");
        if (base.empty())
            continue;
        if (!medium.empty())
            result.images.push_back(base + "/" + medium);
        if (!full.empty())
            result.fullImages.push_back(base + "/" + full);
    }

    auto readFiles = [&](const char* key, bool archived) {
        for (auto& entry : root.value(key, json::array()))
        {
            File file;
            file.id          = number(entry, "_idRow");
            file.name        = str(entry, "_sFile");
            file.description = str(entry, "_sDescription");
            file.url         = str(entry, "_sDownloadUrl");
            file.md5         = str(entry, "_sMd5Checksum");
            file.size        = (uint64_t)number(entry, "_nFilesize");
            file.downloads   = number(entry, "_nDownloadCount");
            file.added       = number(entry, "_tsDateAdded");
            file.archived    = archived || flag(entry, "_bIsArchived");
            for (auto& integration : entry.value("_aModManagerIntegrations", json::array()))
                if (str(integration, "_sModManagerAlias") == "TotkModManager")
                    file.recommended = true;
            if (!file.url.empty())
                result.files.push_back(std::move(file));
        }
    };
    readFiles("_aFiles", false);
    readFiles("_aArchivedFiles", true);
    return result;
}

std::string htmlToText(const std::string& html)
{
    std::string out;
    out.reserve(html.size());
    size_t i = 0;
    auto startsWith = [&](const char* tag) {
        size_t length = std::strlen(tag);
        return html.size() - i >= length && strncasecmp(html.c_str() + i, tag, length) == 0;
    };

    while (i < html.size())
    {
        char c = html[i];
        if (c == '<')
        {
            bool lineBreak = startsWith("<br") || startsWith("</p") || startsWith("</div") || startsWith("</h")
                || startsWith("</li") || startsWith("<hr");
            bool bullet = startsWith("<li");
            size_t end  = html.find('>', i);
            if (end == std::string::npos)
                break;
            i = end + 1;
            if (lineBreak && (out.empty() || out.back() != '\n'))
                out.push_back('\n');
            if (bullet)
                out += "• ";
            continue;
        }
        if (c == '&')
        {
            static const std::pair<const char*, const char*> entities[] = {
                { "&amp;", "&" }, { "&lt;", "<" }, { "&gt;", ">" }, { "&quot;", "\"" }, { "&#39;", "'" },
                { "&apos;", "'" }, { "&nbsp;", " " },
            };
            bool replaced = false;
            for (auto& [entity, text] : entities)
            {
                if (startsWith(entity))
                {
                    out += text;
                    i += std::strlen(entity);
                    replaced = true;
                    break;
                }
            }
            if (replaced)
                continue;
        }
        if (c == '\r' || c == '\n' || c == '\t')
            c = ' ';
        if (c == ' ' && !out.empty() && (out.back() == ' ' || out.back() == '\n'))
        {
            i++;
            continue;
        }
        out.push_back(c);
        i++;
    }

    // At most one blank line in a row, nothing dangling at the ends.
    std::string compact;
    int newlines = 0;
    for (char c : out)
    {
        if (c == '\n')
        {
            if (++newlines > 2)
                continue;
        }
        else
        {
            newlines = 0;
        }
        compact.push_back(c);
    }
    size_t begin = compact.find_first_not_of(" \n");
    size_t last  = compact.find_last_not_of(" \n");
    return begin == std::string::npos ? "" : compact.substr(begin, last - begin + 1);
}

} // namespace gamebanana
