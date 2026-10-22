//! Damped spring animation, so opening/closing the panel feels alive with a slight bounce.

pub struct Spring {
    pub value: f32,
    pub target: f32,
    velocity: f32,
    stiffness: f32,
    damping: f32,
    precision: f32,
}

impl Spring {
    /// `damping_ratio` < 1 bounces, = 1 settles smoothly without bounce. `precision` is the
    /// difference that is no longer visible (e.g. 0.05 DIP); below it the animation is done.
    pub fn new(value: f32, stiffness: f32, damping_ratio: f32, precision: f32) -> Self {
        Self {
            value,
            target: value,
            velocity: 0.0,
            stiffness,
            damping: 2.0 * damping_ratio * stiffness.sqrt(),
            precision,
        }
    }

    /// Advance `dt` seconds. Split into small substeps so it stays stable even when frames are late.
    pub fn step(&mut self, dt: f32) {
        const SUBSTEP: f32 = 1.0 / 480.0;
        let mut left = dt.clamp(0.0, 0.05);
        while left > 0.0 {
            let h = left.min(SUBSTEP);
            let accel = -self.stiffness * (self.value - self.target) - self.damping * self.velocity;
            self.velocity += accel * h;
            self.value += self.velocity * h;
            left -= h;
        }
        if (self.value - self.target).abs() < self.precision && self.velocity.abs() < self.precision * 10.0 {
            self.snap(self.target);
        }
    }

    /// Change how bouncy the spring is (see [`Spring::new`]), e.g. bouncy when opening but
    /// smooth when closing.
    pub fn set_damping_ratio(&mut self, damping_ratio: f32) {
        self.damping = 2.0 * damping_ratio * self.stiffness.sqrt();
    }

    /// Jump straight to `value` without animating.
    pub fn snap(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
    }

    pub fn is_moving(&self) -> bool {
        self.value != self.target || self.velocity != 0.0
    }
}
