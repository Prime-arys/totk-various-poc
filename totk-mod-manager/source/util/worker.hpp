#pragma once

#include <cstddef>
#include <functional>

// Long jobs (merging, downloads, installs) run on a thread of their own with
// a large stack; `done` then runs on the interface thread.
namespace worker
{

void run(std::function<void()> job, std::function<void()> done = nullptr, size_t stackSize = 8 * 1024 * 1024);

/** Whether a job is still running. */
bool busy();

/** After the interface has stopped: waits for the jobs still running and
 *  frees their threads (their `done` never runs). A thread left behind would
 *  outlive the homebrew and crash the homebrew menu loaded next into this
 *  process. */
void shutdown();

} // namespace worker
