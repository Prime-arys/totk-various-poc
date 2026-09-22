#include "util/worker.hpp"

#include <algorithm>
#include <atomic>
#include <mutex>
#include <vector>

#include <borealis/core/thread.hpp>
#include <switch.h>

#include "util/log.hpp"

namespace worker
{

namespace
{
    struct Job
    {
        Thread thread;
        std::function<void()> job;
        std::function<void()> done;
    };

    std::mutex lock;
    /** Jobs whose thread has not been closed yet. */
    std::vector<Job*> jobs;
    std::atomic<int> running{ 0 };

    void forget(Job* job)
    {
        std::lock_guard<std::mutex> guard(lock);
        jobs.erase(std::remove(jobs.begin(), jobs.end(), job), jobs.end());
    }

    void entry(void* argument)
    {
        auto* job = (Job*)argument;
        job->job();
        running--;
        brls::sync([job]() {
            forget(job);
            threadWaitForExit(&job->thread);
            threadClose(&job->thread);
            if (job->done)
                job->done();
            delete job;
        });
    }
} // namespace

void run(std::function<void()> job, std::function<void()> done, size_t stackSize)
{
    auto* data = new Job{ {}, std::move(job), std::move(done) };
    // Priority just below the interface's, on any core but the one the
    // interface renders on.
    Result rc = threadCreate(&data->thread, entry, data, nullptr, stackSize, 0x2C, -2);
    if (R_SUCCEEDED(rc))
    {
        {
            std::lock_guard<std::mutex> guard(lock);
            jobs.push_back(data);
        }
        running++;
        rc = threadStart(&data->thread);
        if (R_FAILED(rc))
        {
            running--;
            forget(data);
        }
    }
    if (R_FAILED(rc))
    {
        applog::write("could not start a worker thread, running on the interface thread");
        threadClose(&data->thread);
        data->job();
        if (data->done)
            data->done();
        delete data;
    }
}

bool busy()
{
    return running > 0;
}

void shutdown()
{
    std::vector<Job*> left;
    {
        std::lock_guard<std::mutex> guard(lock);
        left.swap(jobs);
    }
    for (Job* job : left)
    {
        threadWaitForExit(&job->thread);
        threadClose(&job->thread);
    }
    if (!left.empty())
        applog::write("waited for " + std::to_string(left.size()) + " background job(s)");
}

} // namespace worker
