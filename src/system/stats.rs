//! System monitor: CPU, GPU (load and temperature), RAM, C: drive, and network speed. CPU and GPU use
//! Windows Performance Counters (the same numbers as Task Manager). Only sampled while the
//! System page is open; otherwise the objects are released so they do not use memory.

use std::time::Instant;

use windows::Wdk::Graphics::Direct3D::{
    D3DKMT_ADAPTER_PERFDATA, D3DKMT_ADAPTERINFO, D3DKMT_CLOSEADAPTER, D3DKMT_ENUMADAPTERS2,
    D3DKMT_QUERYADAPTERINFO, D3DKMTCloseAdapter, D3DKMTEnumAdapters2, D3DKMTQueryAdapterInfo,
    KMTQAITYPE_ADAPTERPERFDATA,
};
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::Performance::{
    PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhGetFormattedCounterValue, PdhOpenQueryW,
};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::core::{PCWSTR, w};

#[derive(Clone, Default, Debug)]
pub struct Stats {
    /// Percent, 0..100.
    pub cpu: f32,
    /// 3D load percent; `None` if the GPU counter is unavailable.
    pub gpu: Option<f32>,
    /// Hottest GPU temperature in °C, if the driver reports it.
    pub gpu_temp: Option<f32>,
    pub ram_used: u64,
    pub ram_total: u64,
    pub disk_free: u64,
    pub disk_total: u64,
    /// Bytes per second.
    pub down: f64,
    pub up: f64,
}

pub struct Monitor {
    query: PDH_HQUERY,
    cpu: PDH_HCOUNTER,
    gpu: Option<PDH_HCOUNTER>,
    /// Last network byte totals, to compute speed.
    network: Option<(Instant, u64, u64)>,
}

impl Monitor {
    pub fn new() -> Option<Self> {
        unsafe {
            let mut query = PDH_HQUERY(std::ptr::null_mut());
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                return None;
            }
            let mut cpu = PDH_HCOUNTER(std::ptr::null_mut());
            if PdhAddEnglishCounterW(query, w!("\\Processor Information(_Total)\\% Processor Utility"), 0, &mut cpu) != 0 {
                PdhCloseQuery(query);
                return None;
            }
            let mut gpu = PDH_HCOUNTER(std::ptr::null_mut());
            let gpu = (PdhAddEnglishCounterW(query, w!("\\GPU Engine(*engtype_3D)\\Utilization Percentage"), 0, &mut gpu)
                == 0)
                .then_some(gpu);
            // Percentages come from two samples; the first is only a baseline.
            PdhCollectQueryData(query);
            Some(Self { query, cpu, gpu, network: network_totals().map(|(down, up)| (Instant::now(), down, up)) })
        }
    }

    pub fn sample(&mut self) -> Stats {
        let mut stats = Stats::default();
        unsafe {
            PdhCollectQueryData(self.query);
            stats.cpu = value(self.cpu).unwrap_or(0.0).clamp(0.0, 100.0) as f32;
            stats.gpu = self.gpu.and_then(|gpu| sum(gpu)).map(|v| v.clamp(0.0, 100.0) as f32);
            stats.gpu_temp = gpu_temperature();

            let mut memory = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
            if GlobalMemoryStatusEx(&mut memory).is_ok() {
                stats.ram_total = memory.ullTotalPhys;
                stats.ram_used = memory.ullTotalPhys.saturating_sub(memory.ullAvailPhys);
            }
            let _ = GetDiskFreeSpaceExW(w!("C:\\"), None, Some(&mut stats.disk_total), Some(&mut stats.disk_free));
        }
        if let Some((down, up)) = network_totals() {
            let now = Instant::now();
            if let Some((then, last_down, last_up)) = self.network {
                let secs = now.duration_since(then).as_secs_f64().max(0.001);
                stats.down = down.saturating_sub(last_down) as f64 / secs;
                stats.up = up.saturating_sub(last_up) as f64 / secs;
            }
            self.network = Some((now, down, up));
        }
        stats
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        unsafe { PdhCloseQuery(self.query) };
    }
}

unsafe fn value(counter: PDH_HCOUNTER) -> Option<f64> {
    let mut out = PDH_FMT_COUNTERVALUE::default();
    let status = unsafe { PdhGetFormattedCounterValue(counter, PDH_FMT_DOUBLE, None, &mut out) };
    (status == 0 && out.CStatus <= 1).then_some(unsafe { out.Anonymous.doubleValue })
}

/// Sum of all instances of a wildcard counter (e.g. each process's 3D engine).
unsafe fn sum(counter: PDH_HCOUNTER) -> Option<f64> {
    let (mut bytes, mut count) = (0u32, 0u32);
    unsafe { PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut bytes, &mut count, None) };
    if bytes == 0 {
        return None;
    }
    let item = size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
    let mut items = vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); bytes as usize / item + 1];
    let status = unsafe { PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut bytes, &mut count, Some(items.as_mut_ptr())) };
    if status != 0 {
        return None;
    }
    let total = items
        .iter()
        .take(count as usize)
        .filter(|i| i.FmtValue.CStatus <= 1)
        .map(|i| unsafe { i.FmtValue.Anonymous.doubleValue })
        .sum();
    Some(total)
}

/// Total bytes received/sent over connected physical network adapters.
fn network_totals() -> Option<(u64, u64)> {
    unsafe {
        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut table).is_err() || table.is_null() {
            return None;
        }
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        // Bit 0 = hardware interface, bit 1 = filter (a copy of another interface),
        // bit 2 = has a physical connector.
        let totals = rows
            .iter()
            .filter(|r| {
                let flags = r.InterfaceAndOperStatusFlags._bitfield;
                flags & 0b001 != 0 && flags & 0b010 == 0 && flags & 0b100 != 0 && r.OperStatus == IfOperStatusUp
            })
            .fold((0u64, 0u64), |(down, up), r| (down + r.InOctets, up + r.OutOctets));
        FreeMibTable(table.cast());
        Some(totals)
    }
}

/// Hottest GPU temperature in °C, from the graphics driver's performance data (the same source
/// as Task Manager). `None` if the driver does not report it, e.g. while the GPU is asleep.
pub fn gpu_temperature() -> Option<f32> {
    unsafe {
        let mut list = D3DKMT_ENUMADAPTERS2::default();
        if D3DKMTEnumAdapters2(&mut list).is_err() || list.NumAdapters == 0 {
            return None;
        }
        let mut adapters = vec![D3DKMT_ADAPTERINFO::default(); list.NumAdapters as usize];
        list.pAdapters = adapters.as_mut_ptr();
        if D3DKMTEnumAdapters2(&mut list).is_err() {
            return None;
        }
        let mut hottest: Option<u32> = None;
        for adapter in adapters.iter().take(list.NumAdapters as usize) {
            let mut perf = D3DKMT_ADAPTER_PERFDATA::default();
            let mut query = D3DKMT_QUERYADAPTERINFO {
                hAdapter: adapter.hAdapter,
                Type: KMTQAITYPE_ADAPTERPERFDATA,
                pPrivateDriverData: (&mut perf as *mut D3DKMT_ADAPTER_PERFDATA).cast(),
                PrivateDriverDataSize: size_of::<D3DKMT_ADAPTER_PERFDATA>() as u32,
            };
            // Units are tenths of a degree.
            if D3DKMTQueryAdapterInfo(&mut query).is_ok() && perf.Temperature > 0 {
                hottest = Some(hottest.unwrap_or(0).max(perf.Temperature));
            }
            let _ = D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: adapter.hAdapter });
        }
        hottest.map(|t| t as f32 / 10.0)
    }
}

/// "9.8 GB"
pub fn gigabytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
}

/// Short file size: "850 KB", "12.4 MB", or "1.2 GB".
pub fn size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1_073_741_824.0 {
        format!("{:.1} GB", b / 1_073_741_824.0)
    } else if b >= 1_048_576.0 {
        format!("{:.1} MB", b / 1_048_576.0)
    } else {
        format!("{:.0} KB", (b / 1024.0).max(1.0))
    }
}

/// "1.2 MB/s" or "80 KB/s"
pub fn rate(bytes_per_sec: f64) -> String {
    if bytes_per_sec >= 1_048_576.0 {
        format!("{:.1} MB/s", bytes_per_sec / 1_048_576.0)
    } else {
        format!("{:.0} KB/s", bytes_per_sec / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sizes_and_rates() {
        assert_eq!(gigabytes(10_522_669_875), "9.8 GB");
        assert_eq!(rate(1_258_291.0), "1.2 MB/s");
        assert_eq!(rate(81_920.0), "80 KB/s");
        assert_eq!(size(13_002_342), "12.4 MB");
        assert_eq!(size(870_400), "850 KB");
    }

    /// Live sampling on this machine; two samples so the CPU percentage is filled in.
    #[test]
    fn samples_this_machine() {
        let mut monitor = Monitor::new().expect("CPU counter available");
        std::thread::sleep(std::time::Duration::from_millis(300));
        let stats = monitor.sample();
        assert!(stats.ram_total > 0 && stats.ram_used <= stats.ram_total);
        assert!(stats.disk_total > 0 && stats.disk_free <= stats.disk_total);
        assert!((0.0..=100.0).contains(&stats.cpu));
        assert!(stats.gpu_temp.is_none_or(|t| (1.0..130.0).contains(&t)));
    }
}
