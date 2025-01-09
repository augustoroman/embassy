// Repro case for the integrated timer bug. When core1 is running, the interrupt
// executor on core0 eventually hangs. Example of failed output looks like:
//     > cargo run --bin timer-bug
//     ...
//     0.001740 INFO  timer-bug.rs:24  | Hello World!
//     0.000380 INFO  timer-bug.rs:34  | core1 starting up
//     0.000525 INFO  timer-bug.rs:69  | # Starting int
//     0.000580 INFO  timer-bug.rs:69  | # Starting core1
//     0.000713 INFO  timer-bug.rs:69  | # Starting core0
//     1.003938 INFO  timer-bug.rs:73  | still running int   -> 200 iters
//     1.004254 INFO  timer-bug.rs:73  | still running core0 -> 250 iters
//     1.008524 INFO  timer-bug.rs:73  | still running core1 -> 125 iters
//     2.003302 INFO  timer-bug.rs:73  | still running core0 -> 249 iters
//     2.006394 INFO  timer-bug.rs:73  | still running int   -> 200 iters
//     2.016524 INFO  timer-bug.rs:73  | still running core1 -> 126 iters
//     3.002366 INFO  timer-bug.rs:73  | still running core0 -> 249 iters
//     3.003833 INFO  timer-bug.rs:73  | still running int   -> 199 iters
//     3.024524 INFO  timer-bug.rs:73  | still running core1 -> 126 iters
//     4.005350 INFO  timer-bug.rs:73  | still running core0 -> 250 iters
//     4.032524 INFO  timer-bug.rs:73  | still running core1 -> 126 iters
//     5.004139 INFO  timer-bug.rs:73  | still running core0 -> 249 iters
//     5.040524 INFO  timer-bug.rs:73  | still running core1 -> 126 iters
//     6.006946 INFO  timer-bug.rs:73  | still running core0 -> 250 iters
//     6.048524 INFO  timer-bug.rs:73  | still running core1 -> 126 iters
//     7.005743 INFO  timer-bug.rs:73  | still running core0 -> 249 iters

#![no_std]
#![no_main]

use defmt::{info, unwrap};
use embassy_executor::{Executor, InterruptExecutor};
use embassy_futures::select::{select, Either};
use embassy_rp::interrupt;
use embassy_rp::interrupt::{InterruptExt, Priority};
use embassy_rp::multicore::Stack;
use embassy_time::{Duration, Ticker, Timer};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

static mut CORE1_STACK: Stack<8192> = Stack::new();
static EXECUTOR0: StaticCell<Executor> = StaticCell::new();         // core 0
static EXECUTOR0_INT: InterruptExecutor = InterruptExecutor::new(); // core 0
static EXECUTOR1: StaticCell<Executor> = StaticCell::new();         // core 1

#[interrupt]
unsafe fn SWI_IRQ_0() { EXECUTOR0_INT.on_interrupt() }

#[cortex_m_rt::entry]
fn main() -> ! {
    info!("Hello World!");
    let p = embassy_rp::init(Default::default());

    // Running core1 breaks the interrupt on core0. Comment out this call and
    // the interrupt runs fine. Alternatively, slow down the core0 task (e.g.
    // 8000us) and it works more reliably but still fails after a while.
    embassy_rp::multicore::spawn_core1(
        p.CORE1,
        unsafe { &mut *core::ptr::addr_of_mut!(CORE1_STACK) },
        move || {
            info!("core1 starting up");
            let executor1 = EXECUTOR1.init(Executor::new());
            executor1.run(|spawner| {
                // Fails almost immediately:
                // spawner.spawn(run_core1(Duration::from_micros(50), Duration::from_micros(47))).unwrap();

                // Fails after a while:
                spawner.spawn(run_core1(Duration::from_micros(8000), Duration::from_micros(47))).unwrap();
            });
        },
    );

    interrupt::SWI_IRQ_1.set_priority(Priority::P2);
    let spawner = EXECUTOR0_INT.start(interrupt::SWI_IRQ_0);
    unwrap!(spawner.spawn(run_interrupt(Duration::from_millis(1), Duration::from_millis(5))));

    let executor = EXECUTOR0.init(Executor::new());
    executor.run(|spawner| {
        unwrap!(spawner.spawn(run_core0(Duration::from_micros(500), Duration::from_millis(4))));
    });
}

#[embassy_executor::task]
async fn run_core0(rate: Duration, delay: Duration) { looper("core0", rate, delay).await }

#[embassy_executor::task]
async fn run_core1(rate: Duration, delay: Duration) { looper("core1", rate, delay).await }

#[embassy_executor::task]
async fn run_interrupt(rate: Duration, delay: Duration) { looper("int  ", rate, delay).await }

async fn looper(name: &'static str, rate: Duration, delay: Duration) {
    let mut logger = Ticker::every(Duration::from_secs(1) + rate);
    let mut trigger = Ticker::every(rate);
    let mut count = 0;
    info!("# Starting {}", name);
    loop {
        match select(logger.next(), trigger.next()).await {
            Either::First(_) => {
                info!("still running {} -> {} iters", name, count);
                count = 0;
            },
            Either::Second(_) => {
                count += 1;
                Timer::after(delay).await;
            }
        }
    }
}

