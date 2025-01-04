use lazy_static::lazy_static;
use std::cmp::{max, min};
use std::fmt::{Display, Formatter};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

pub struct LatRet {
    nanos: Vec<f64>,
    p_tiles: Vec<f64>,
    total: u64,
    max_nanos: u64,
    snap_time_millis: u128,
    duration_nanos: u64,
}

impl Display for LatRet {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if self.total == 0 {
            return write!(f, "No data points");
        }
        //{:.3}
        let mut s:String = format!("max:{}", time_format(self.max_nanos as f64)).to_string();
        for (i, nano) in self.nanos.iter().rev().enumerate() {
            s+= format!("{}%:{}", self.p_tiles[i], time_format(*nano)).as_str();
        }
        let thruput = self.total as f64 / (self.duration_nanos as f64/1000_000_000.);
        s+=format!("total:{} duration_nanos:{} ops/sec:{:.2}", self.total, self.duration_nanos, thruput).as_str();
        f.write_str(s.as_str())
    }
}

fn time_format(t: f64) -> String {
    if t < 1000. {
        format!("{:.2}ns ", t)
    } else if t < 1000. * 1000. {
        format!("{:.2}us ", t/1000.)
    } else if t < 1000. * 1000. * 1000. {
        format!("{:.2}ms ", t / (1000.*1000.))
    } else {
        format!("{:.2}s ", t / (1000.*1000. * 1000.))
    }
}

pub struct DefaultPrinter {}
impl LatPrinter for DefaultPrinter {
    fn log(&self, name: &str, lat_ret: &LatRet) {
        println!("{}, {}", name, lat_ret.to_string());
    }
}

pub trait LatPrinter {
    fn log(&self, name: &str, lat_ret: &LatRet);
}

lazy_static! {
    static ref START: Instant = Instant::now();
}

pub struct LatencyTimer<T> where T : LatPrinter + Send + Sync {
    name: String,
    lat_printer: T,
    bins: Vec<AtomicU64>,
    max_nanos: AtomicU64,
    last_count: AtomicU64,
    p_tiles: Vec<f64>,
    die: AtomicBool,
    last_snap_time_nanos: AtomicU64,

}

impl<T> LatencyTimer<T>
where T : LatPrinter + Send + Sync {
    fn new_with_printer(t: T) -> Self
    where T: LatPrinter
    {
        Self {
            name: String::from("noname",),
            lat_printer: t,
            bins: Vec::with_capacity(4000),
            max_nanos: AtomicU64::new(0),
            last_count: AtomicU64::new(0),
            die: AtomicBool::new(false),
            p_tiles: vec![1., 50., 75., 90., 95., 99., 99.9],
            last_snap_time_nanos: AtomicU64::new(get_time2())
        }
    }
}

impl Default for LatencyTimer<DefaultPrinter> {
    fn default () -> Self {
        Self {
            name: String::from("noname"),
            lat_printer: DefaultPrinter {},
            bins: (0..4000)
                .map(|_| AtomicU64::new(0))
                .collect(),
            max_nanos: AtomicU64::new(0),
            last_count: AtomicU64::new(0),
            p_tiles: vec![1., 50., 75., 90., 95., 99., 99.9],
            die: Default::default(),
            last_snap_time_nanos: AtomicU64::new(get_time2()),
        }
    }


}

fn get_time2() -> u64 {
    Instant::now().duration_since(*START).as_nanos() as u64

    // tick_counter::x86_64_tick_counter()
}
impl<T> LatencyTimer<T> where T : LatPrinter + Send + Sync + 'static{
    pub fn count1(&self) {
        let now: u64 = get_time2();
        let last_count = self.last_count.swap(now, Relaxed);
        if last_count != 0 {
            let diff:i64 = now as i64 - last_count as i64;
            self.count2(max(0, diff) as u64);
        }
    }
    fn count2(&self, mut latency_nanos: u64) {
        let mut index = 0;
        self.max_nanos.fetch_max(latency_nanos, Relaxed);

        while latency_nanos >= 1000 {
            latency_nanos /= 1000;
            index += 1000;
        }
        let array_index = min(index + latency_nanos, (self.bins.len() - 1) as u64) as usize;
        self.bins[array_index].fetch_add(1, Relaxed);
    }

    fn reset(& self) {
        for bin in self.bins.iter() {
            bin.store(0, Relaxed);
        }
        self.max_nanos.store(0, Relaxed);
    }

    fn die(&self) {
        self.die.store(false, Relaxed);
    }

    fn snap(&self) -> LatRet {
        let mybins = self.bins.iter().map(|bin| bin.load(Relaxed)).collect::<Vec<u64>>();
        let total = mybins.iter().sum();
        let uniqs = mybins.iter().map(|bin| if *bin > 0 { 1 } else { 0 }).sum::<u64>();
        println!("uniqs {}", uniqs);
        println!("Total: {}", total);
        let my_max_nanos = self.max_nanos.load(Relaxed);
        let mut nanos : Vec<f64> = Vec::with_capacity(self.p_tiles.len());
        for p_tile in self.p_tiles.iter().rev() {
            let mut cumulative:u64 = 0;
            let max:u64 = ((total as f64)*p_tile/100.) as u64;
            let mut index = 0;

            while index < mybins.len() && mybins[index] + cumulative < max {
                cumulative += mybins[index];
                index += 1;
            }
            let mut mul: u64 = 1;
            let mut temp = index as u64;
            while temp >= 1000 {
                temp -= 1000;
                mul *= 1000;
            }
            nanos.push(((temp + 1) * mul) as f64);
        }
        self.reset();

        let cur_time = get_time2();
        let ret = LatRet { nanos, p_tiles: self.p_tiles.clone(), total, max_nanos: my_max_nanos, snap_time_millis: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis(), duration_nanos: cur_time - self.last_snap_time_nanos.load(Relaxed) };
        self.last_snap_time_nanos.store(cur_time, Relaxed);
        ret

    }
}
pub fn  run<T>(t: &Arc<LatencyTimer<T>>)
where T : LatPrinter + Send + Sync + 'static  {
    let tt = t.clone();
    thread::spawn(move || {
        let obj = tt;
        loop {
            thread::sleep(Duration::from_secs(2));

            let ret = obj.snap();
            if obj.die.load(Relaxed) {
                return;
            }
            obj.lat_printer.log(obj.name.as_str(), &ret);
        }
    }
    );
}