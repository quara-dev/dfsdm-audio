/// First-order IIR high-pass filter, mirroring `audio_filter_hp()` in the
/// Quara firmware (`src/criq/audio_dsp/audio_processing.c`).
///
/// Difference equation (Q14 fixed-point):
///
/// ```text
/// acc   = B0 * (x[n] - x[n-1]) + A1 * y[n-1]
/// y[n]  = clamp(acc >> 14, -32767, 32767)
/// ```
///
/// State field `x1b0` stores the *previous input already multiplied by B0*
/// (matching the C implementation's optimisation of pre-scaling `x[n-1]`).
pub struct AudioHpFilter {
    /// Previous input × B0 (Q14 pre-scaled).
    x1b0: i64,
    /// Previous output sample (Q0).
    y1: i64,
}

impl AudioHpFilter {
    const B0: i64 = 8116;  // feedforward coefficient, Q14
    const A1: i64 = 16078; // feedback coefficient, Q14
    const Q:  u32 = 14;    // fractional bits
    const MAX: i64 =  32_767;
    const MIN: i64 = -32_767;

    pub fn new() -> Self {
        Self { x1b0: 0, y1: 0 }
    }

    /// Apply filter from `input` into `output` without modifying `input`.
    /// Mirrors `audio_filter_hp()` in `audio_processing.c` exactly.
    pub fn process_into(&mut self, input: &[i16], output: &mut [i16]) {
        debug_assert_eq!(input.len(), output.len());
        let mut x1b0 = self.x1b0;
        let mut y1   = self.y1;

        for (&s_in, s_out) in input.iter().zip(output.iter_mut()) {
            let x0        = (s_in as i64).clamp(Self::MIN, Self::MAX);
            let x0_scaled = x0 * Self::B0;
            let acc       = -x1b0 + x0_scaled + y1 * Self::A1;
            x1b0          = x0_scaled;
            y1            = (acc >> Self::Q).clamp(Self::MIN, Self::MAX);
            *s_out        = y1 as i16;
        }

        self.x1b0 = x1b0;
        self.y1   = y1;
    }

    /// Filter `samples` in-place.
    pub fn process(&mut self, samples: &mut [i16]) {
        let mut x1b0 = self.x1b0;
        let mut y1   = self.y1;

        for s in samples.iter_mut() {
            let x0        = (*s as i64).clamp(Self::MIN, Self::MAX);
            let x0_scaled = x0 * Self::B0;
            let acc       = -x1b0 + x0_scaled + y1 * Self::A1;
            x1b0          = x0_scaled;
            y1            = (acc >> Self::Q).clamp(Self::MIN, Self::MAX);
            *s            = y1 as i16;
        }

        self.x1b0 = x1b0;
        self.y1   = y1;
    }
}
