#pragma once

#include <cstdint>
#include <string>
#include <vector>

// sys-clk (github.com/retronx-team/sys-clk), when it runs: its temporary
// overrides, the ones its overlay sets, raise the CPU and memory clocks while
// a merge runs, and are put back as they were afterwards.
namespace sysclk
{

struct Info
{
    bool running        = false;
    uint32_t apiVersion = 0;
    /** Frequencies it can set, in Hz, lowest first (empty when it cannot
     *  tell). */
    std::vector<uint32_t> cpu;
    std::vector<uint32_t> memory;
};

/** Whether sys-clk runs, and what it can set. */
Info query();

/** The highest frequency of `list` not above `wantedMhz`, in Hz (`wantedMhz`
 *  itself when the list is empty: sys-clk picks the nearest). */
uint32_t pick(const std::vector<uint32_t>& list, uint32_t wantedMhz);

/** Sets overrides (0 leaves a clock alone). Returns what was done, for logs
 *  and the progress title, or an empty string when nothing was. */
std::string boost(uint32_t cpuHz, uint32_t memoryHz);

/** Puts back the overrides that were set before boost(). */
void unboost();

} // namespace sysclk
