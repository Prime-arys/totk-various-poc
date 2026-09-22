#include "util/images.hpp"

#include <condition_variable>
#include <deque>
#include <list>
#include <mutex>
#include <utility>

#include <borealis/views/image.hpp>
#include <switch.h>

#include "core/core.hpp"
#include "net/http.hpp"
#include "util/log.hpp"

namespace images
{

namespace
{
    using Setter = std::function<void(const std::string&, size_t)>;

    struct Request
    {
        std::string key; // url, or "mod:" + folder
        Setter set;
    };

    // Two loaders are plenty for thumbnails, and keep the SD card and
    // sockets free for everything else.
    const int LOADERS = 2;
    const size_t CACHE_SIZE = 48;

    std::mutex lock;
    std::condition_variable wake;
    std::deque<Request> queue;
    std::list<std::pair<std::string, std::string>> cache; // most recent first
    bool started  = false;
    bool stopping = false;

    bool cached(const std::string& key, std::string& data)
    {
        for (auto it = cache.begin(); it != cache.end(); ++it)
        {
            if (it->first == key)
            {
                data = it->second;
                cache.splice(cache.begin(), cache, it);
                return true;
            }
        }
        return false;
    }

    void remember(const std::string& key, const std::string& data)
    {
        cache.emplace_front(key, data);
        if (cache.size() > CACHE_SIZE)
            cache.pop_back();
    }

    void loader()
    {
        while (true)
        {
            Request request;
            {
                std::unique_lock<std::mutex> guard(lock);
                wake.wait(guard, [] { return stopping || !queue.empty(); });
                if (stopping)
                    return;
                request = std::move(queue.front());
                queue.pop_front();
            }

            std::string data;
            {
                std::lock_guard<std::mutex> guard(lock);
                if (cached(request.key, data))
                {
                    request.set(data, data.size());
                    continue;
                }
            }

            if (request.key.rfind("mod:", 0) == 0)
            {
                auto bytes = core::loadThumbnail(request.key.substr(4));
                data.assign(bytes.begin(), bytes.end());
            }
            else
            {
                http::Response response = http::get(request.key, 20);
                if (response.ok())
                    data = std::move(response.body);
            }

            if (!data.empty())
            {
                std::lock_guard<std::mutex> guard(lock);
                remember(request.key, data);
            }
            request.set(data, data.size());
        }
    }

    Thread loaders[LOADERS];
    bool loaderStarted[LOADERS] = {};

    void loaderEntry(void*)
    {
        loader();
    }

    void enqueue(brls::Image* image, const std::string& key)
    {
        if (!image || key.empty())
            return;
        image->setImageAsync([key](Setter set) {
            std::lock_guard<std::mutex> guard(lock);
            if (stopping)
                return;
            if (!started)
            {
                started = true;
                // libnx threads: std::thread cannot be detached on the Switch.
                for (int i = 0; i < LOADERS; i++)
                {
                    Result rc = threadCreate(&loaders[i], loaderEntry, nullptr, nullptr, 512 * 1024, 0x2C, -2);
                    if (R_FAILED(rc))
                    {
                        applog::write("could not create an image loader: " + std::to_string(rc));
                        continue;
                    }
                    rc = threadStart(&loaders[i]);
                    if (R_FAILED(rc))
                    {
                        applog::write("could not start an image loader: " + std::to_string(rc));
                        threadClose(&loaders[i]);
                        continue;
                    }
                    loaderStarted[i] = true;
                }
            }
            queue.push_back({ key, std::move(set) });
            wake.notify_one();
        });
    }
} // namespace

void fromUrl(brls::Image* image, const std::string& url)
{
    enqueue(image, url);
}

void fromMod(brls::Image* image, const std::string& folder)
{
    enqueue(image, "mod:" + folder);
}

void shutdown()
{
    {
        std::lock_guard<std::mutex> guard(lock);
        stopping = true;
        queue.clear();
    }
    wake.notify_all();
    // A loader busy with a download finishes it first (http::abort cuts it
    // short).
    for (int i = 0; i < LOADERS; i++)
    {
        if (loaderStarted[i])
        {
            threadWaitForExit(&loaders[i]);
            threadClose(&loaders[i]);
            loaderStarted[i] = false;
        }
    }
}

} // namespace images
