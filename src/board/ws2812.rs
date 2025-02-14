//! [ws2812](https://www.sparkfun.com/datasheets/LCD/HD44780.pdf)

use embassy_time::Timer;
use fixed::types::U24F8;

use embassy_rp::{
    bind_interrupts,
    clocks::clk_sys_freq,
    dma::AnyChannel,
    peripherals::{DMA_CH1, PIN_15, PIO1},
    pio::{
        Common, Config, FifoJoin, Instance, InterruptHandler, LoadedProgram, Pio, ShiftConfig,
        ShiftDirection, StateMachine,
    },
};
use embassy_rp::{Peripheral, PeripheralRef};

bind_interrupts!(struct Irqs {
    PIO1_IRQ_0 => InterruptHandler<PIO1>;
});

const T1: u8 = 2; // start bit
const T2: u8 = 5; // data bit
const T3: u8 = 3; // stop bit
const CYCLES_PER_BIT: u32 = (T1 + T2 + T3) as u32;

/// This struct represents a ws2812 program loaded into pio instruction memory.
struct PioWs2812Program<'a, PIO: Instance> {
    prg: LoadedProgram<'a, PIO>,
}

impl<'a, PIO: Instance> PioWs2812Program<'a, PIO> {
    /// Load the ws2812 program into the given pio
    fn new(common: &mut Common<'a, PIO>) -> Self {
        let side_set = pio::SideSet::new(false, 1, false);
        let mut a: pio::Assembler<32> = pio::Assembler::new_with_side_set(side_set);

        let mut wrap_target = a.label();
        let mut wrap_source = a.label();
        let mut do_zero = a.label();
        a.set_with_side_set(pio::SetDestination::PINDIRS, 1, 0);
        a.bind(&mut wrap_target);
        // Do stop bit
        a.out_with_delay_and_side_set(pio::OutDestination::X, 1, T3 - 1, 0);
        // Do start bit
        a.jmp_with_delay_and_side_set(pio::JmpCondition::XIsZero, &mut do_zero, T1 - 1, 1);
        // Do data bit = 1
        a.jmp_with_delay_and_side_set(pio::JmpCondition::Always, &mut wrap_target, T2 - 1, 1);
        a.bind(&mut do_zero);
        // Do data bit = 0
        a.nop_with_delay_and_side_set(T2 - 1, 0);
        a.bind(&mut wrap_source);

        let prg = a.assemble_with_wrap(wrap_source, wrap_target);
        let prg = common.load_program(&prg);

        Self { prg }
    }
}

/// Pio backed ws2812 driver
pub struct Ws2812 {
    dma: PeripheralRef<'static, AnyChannel>,
    sm: StateMachine<'static, PIO1, 0>,
}

impl Ws2812 {
    /// Configure a pio state machine to use the loaded ws2812 program.
    pub fn new(pio: PIO1, dma: DMA_CH1, pin: PIN_15) -> Self {
        let Pio {
            mut common,
            mut sm0,
            ..
        } = Pio::new(pio, Irqs);

        let dma = dma.into_ref();

        // Setup sm0
        let mut cfg = Config::default();

        // Pin config
        let out_pin = common.make_pio_pin(pin);
        cfg.set_out_pins(&[&out_pin]);
        cfg.set_set_pins(&[&out_pin]);

        let program = PioWs2812Program::new(&mut common);
        cfg.use_program(&program.prg, &[&out_pin]);

        // Clock config, measured in kHz to avoid overflows
        let clock_freq = U24F8::from_num(clk_sys_freq() / 1000);
        let ws2812_freq = U24F8::from_num(800);
        let bit_freq = ws2812_freq * CYCLES_PER_BIT;
        cfg.clock_divider = clock_freq / bit_freq;

        // FIFO config
        cfg.fifo_join = FifoJoin::TxOnly;
        cfg.shift_out = ShiftConfig {
            auto_fill: true,
            threshold: 24,
            direction: ShiftDirection::Left,
        };

        sm0.set_config(&cfg);
        sm0.set_enable(true);

        Self {
            dma: dma.map_into(),
            sm: sm0,
        }
    }

    pub async fn write<const N: usize>(&mut self, data: &[u32; N]) {
        // DMA transfer
        self.sm.tx().dma_push(self.dma.reborrow(), data).await;

        Timer::after_micros(55).await;
    }
}
