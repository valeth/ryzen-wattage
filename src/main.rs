#![allow(dead_code)]

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
    thread,
    time::Duration,
};

type MsrMap = BTreeMap<u32, Msr>;

#[derive(Debug)]
struct Cpu {
    smt_enabled: bool,
    core_count: u32,
    core_msr: MsrMap,
}

impl Cpu {
    pub fn new() -> io::Result<Self> {
        let smt_status = fs::read_to_string("/sys/devices/system/cpu/smt/control")?;
        let smt_enabled = smt_status.trim_end() == "on";

        let cores_online = fs::read_to_string("/sys/devices/system/cpu/online")?;
        let (_, max) = cores_online.trim_end().split_once("-").unwrap();
        let core_count = max.parse::<u32>().unwrap() + 1;

        let core_msr = Self::get_msr_info(smt_enabled, core_count);

        Ok(Self {
            smt_enabled,
            core_count,
            core_msr,
        })
    }

    fn get_msr_info(smt_enabled: bool, core_count: u32) -> MsrMap {
        let mut map = MsrMap::new();

        for core in 0..core_count {
            if smt_enabled && core % 2 == 0 {
                let msr = Msr::new(core);
                map.insert(core, msr);
            }
        }

        map
    }

    pub fn package_energy(&self) -> f64 {
        let (_, energy) = self
            .core_msr
            .iter()
            .map(|(core, msr)| (core, msr.package_energy().unwrap()))
            .next()
            .unwrap();

        energy
    }

    pub fn core_energy(&self) -> BTreeMap<u32, f64> {
        self.core_msr
            .iter()
            .map(|(core, msr)| (*core, msr.core_energy().unwrap()))
            .collect()
    }

    pub fn power(&self, duration: Duration) -> (f64, BTreeMap<u32, f64>) {
        let package_energy_before = self.package_energy();
        let core_energy_before = self.core_energy();

        thread::sleep(duration);

        let package_energy_after = self.package_energy();
        let core_energy_after = self.core_energy();

        let duration = duration.as_secs() as f64;

        let package_energy = (package_energy_after - package_energy_before) / duration;

        let cores_energy = core_energy_before
            .iter()
            .zip(&core_energy_after)
            .map(|((&core, &before), (_, &after))| (core, (after - before) / duration))
            .collect();

        (package_energy, cores_energy)
    }
}

#[derive(Debug)]
struct Msr {
    path: PathBuf,
}

impl Msr {
    const POWER_UNIT_OFFSET: u64 = 0xC0010299;
    const CORE_ENERGY_OFFSET: u64 = 0xC001029A;
    const PACKAGE_ENERGY_OFFSET: u64 = 0xC001029B;
    const ENERGY_UNIT_MASK: u64 = 0x1F00;

    pub fn new(core: u32) -> Self {
        let path = PathBuf::from(format!("/dev/cpu/{}/msr", core));
        Self { path }
    }

    pub fn core_energy(&self) -> io::Result<f64> {
        let core_energy = self.read_register(Self::CORE_ENERGY_OFFSET)?;
        let core_energy = core_energy as f64 * self.energy_unit()?;
        Ok(core_energy)
    }

    pub fn package_energy(&self) -> io::Result<f64> {
        let energy = self.read_register(Self::PACKAGE_ENERGY_OFFSET)?;
        let energy = energy as f64 * self.energy_unit()?;
        Ok(energy)
    }

    fn energy_unit(&self) -> io::Result<f64> {
        let units = self.read_register(Self::POWER_UNIT_OFFSET)?;
        let unit = (units & Self::ENERGY_UNIT_MASK) >> 8;
        Ok((0.5_f64).powf(unit as f64))
    }

    fn read_register(&self, offset: u64) -> io::Result<u64> {
        let mut msr_file = File::open(&self.path)?;
        msr_file.seek(SeekFrom::Start(offset))?;

        let mut data = [0u8; 8];
        msr_file.read_exact(&mut data)?;

        let data = u64::from_ne_bytes(data);
        Ok(data)
    }
}

fn main() {
    let cpu = Cpu::new().unwrap();

    let (package_power, cores_power) = cpu.power(Duration::from_secs(1));

    println!("Package: {:.2}W", package_power);

    let mut core_sum = 0.0;

    for (core, core_power) in cores_power {
        core_sum += core_power;
        println!("Core {}: {:.2}W", core, core_power);
    }

    println!("Cores Total: {:.2}W", core_sum);
}
