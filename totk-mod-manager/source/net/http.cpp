#include "net/http.hpp"

#include <atomic>
#include <cctype>
#include <cstdio>

#include <curl/curl.h>

#ifdef __SWITCH__
#include <switch.h>
#endif

#include "util/log.hpp"

namespace http
{

namespace
{
    const char* USER_AGENT = "totk-mod-manager/" APP_VERSION;
    std::atomic<bool> aborting{ false };

    size_t appendToString(char* data, size_t size, size_t count, void* user)
    {
        auto* body = (std::string*)user;
        body->append(data, size * count);
        return size * count;
    }

    struct FileSink
    {
        FILE* file;
        Progress* progress;
    };

    size_t appendToFile(char* data, size_t size, size_t count, void* user)
    {
        auto* sink = (FileSink*)user;
        return fwrite(data, size, count, sink->file) * size;
    }

    int onProgress(void* user, curl_off_t total, curl_off_t done, curl_off_t, curl_off_t)
    {
        auto* sink = (FileSink*)user;
        if (aborting || (sink->progress && *sink->progress && !(*sink->progress)((uint64_t)done, (uint64_t)total)))
            return 1; // cancels the transfer
        return 0;
    }

    int abortCheck(void*, curl_off_t, curl_off_t, curl_off_t, curl_off_t)
    {
        return aborting ? 1 : 0;
    }

    CURL* newHandle(const std::string& url, long timeoutSeconds)
    {
        CURL* curl = curl_easy_init();
        if (!curl)
            return nullptr;
        curl_easy_setopt(curl, CURLOPT_URL, url.c_str());
        curl_easy_setopt(curl, CURLOPT_USERAGENT, USER_AGENT);
        curl_easy_setopt(curl, CURLOPT_FOLLOWLOCATION, 1L);
        curl_easy_setopt(curl, CURLOPT_MAXREDIRS, 8L);
        curl_easy_setopt(curl, CURLOPT_CONNECTTIMEOUT, 20L);
        if (timeoutSeconds > 0)
            curl_easy_setopt(curl, CURLOPT_TIMEOUT, timeoutSeconds);
        // Abort a download that stalls rather than wait forever.
        curl_easy_setopt(curl, CURLOPT_LOW_SPEED_LIMIT, 64L);
        curl_easy_setopt(curl, CURLOPT_LOW_SPEED_TIME, 60L);
        curl_easy_setopt(curl, CURLOPT_ACCEPT_ENCODING, "");
        curl_easy_setopt(curl, CURLOPT_BUFFERSIZE, 512L * 1024L);
        return curl;
    }
} // namespace

void init()
{
#ifdef __SWITCH__
    // libcurl's libnx TLS backend talks to the ssl service.
    sslInitialize(0x3);
    csrngInitialize();
#endif
    curl_global_init(CURL_GLOBAL_DEFAULT);
}

void abort()
{
    aborting = true;
}

void cleanup()
{
    curl_global_cleanup();
#ifdef __SWITCH__
    csrngExit();
    sslExit();
#endif
}

Response get(const std::string& url, long timeoutSeconds)
{
    Response response;
    CURL* curl = newHandle(url, timeoutSeconds);
    if (!curl)
    {
        response.error = "curl_easy_init failed";
        return response;
    }
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, appendToString);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &response.body);
    curl_easy_setopt(curl, CURLOPT_NOPROGRESS, 0L);
    curl_easy_setopt(curl, CURLOPT_XFERINFOFUNCTION, abortCheck);
    CURLcode code = curl_easy_perform(curl);
    if (code != CURLE_OK)
        response.error = curl_easy_strerror(code);
    curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &response.status);
    curl_easy_cleanup(curl);
    if (!response.ok())
        applog::write("GET " + url + " -> " + std::to_string(response.status) + " " + response.error);
    return response;
}

std::string download(const std::string& url, const std::string& path, Progress progress)
{
    FILE* file = fopen(path.c_str(), "wb");
    if (!file)
        return "cannot create " + path;
    std::vector<char> buffer(1024 * 1024);
    setvbuf(file, buffer.data(), _IOFBF, buffer.size());

    CURL* curl = newHandle(url, 0);
    if (!curl)
    {
        fclose(file);
        return "curl_easy_init failed";
    }
    FileSink sink{ file, &progress };
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, appendToFile);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &sink);
    curl_easy_setopt(curl, CURLOPT_NOPROGRESS, 0L);
    curl_easy_setopt(curl, CURLOPT_XFERINFOFUNCTION, onProgress);
    curl_easy_setopt(curl, CURLOPT_XFERINFODATA, &sink);

    CURLcode code = curl_easy_perform(curl);
    long status   = 0;
    curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &status);
    curl_easy_cleanup(curl);
    bool closed = fclose(file) == 0;

    std::string error;
    if (code == CURLE_ABORTED_BY_CALLBACK)
        error = "cancelled";
    else if (code != CURLE_OK)
        error = curl_easy_strerror(code);
    else if (status < 200 || status >= 300)
        error = "HTTP " + std::to_string(status);
    else if (!closed)
        error = "could not write " + path + " (SD card full?)";
    if (!error.empty())
    {
        remove(path.c_str());
        applog::write("download " + url + " failed: " + error);
    }
    return error;
}

std::string escape(const std::string& text)
{
    static const char* digits = "0123456789ABCDEF";
    std::string out;
    for (unsigned char c : text)
    {
        if (std::isalnum(c) || c == '-' || c == '_' || c == '.' || c == '~')
        {
            out.push_back((char)c);
        }
        else
        {
            out.push_back('%');
            out.push_back(digits[c >> 4]);
            out.push_back(digits[c & 15]);
        }
    }
    return out;
}

} // namespace http
