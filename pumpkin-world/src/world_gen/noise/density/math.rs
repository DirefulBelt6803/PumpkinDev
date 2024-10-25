use std::sync::Arc;

use log::warn;

use super::{
    Applier, ApplierImpl, DensityFunction, DensityFunctionImpl, NoisePos, UnaryDensityFunction,
    Visitor, VisitorImpl,
};

#[derive(Clone)]
pub enum LinearType {
    Mul,
    Add,
}

#[derive(Clone)]
pub struct LinearFunction<'a> {
    action: LinearType,
    input: Arc<DensityFunction<'a>>,
    min: f64,
    max: f64,
    arg: f64,
}

impl<'a> DensityFunctionImpl<'a> for LinearFunction<'a> {
    fn apply(&self, visitor: &Visitor<'a>) -> Arc<DensityFunction<'a>> {
        let new_function = self.input.apply(visitor);
        let d = new_function.min();
        let e = new_function.max();

        let (f, g) = match self.action {
            LinearType::Add => (d + self.arg, e + self.arg),
            LinearType::Mul => {
                if self.arg >= 0f64 {
                    (d * self.arg, e * self.arg)
                } else {
                    (e * self.arg, d * self.arg)
                }
            }
        };

        Arc::new(DensityFunction::Linear(LinearFunction {
            action: self.action.clone(),
            input: new_function,
            min: f,
            max: g,
            arg: self.arg,
        }))
    }

    fn sample(&self, pos: &NoisePos) -> f64 {
        self.apply_density(self.input.sample(pos))
    }

    fn fill(&self, densities: &mut [f64], applier: &Applier<'a>) {
        self.input.fill(densities, applier);
        densities
            .iter_mut()
            .for_each(|val| *val = self.apply_density(*val))
    }

    fn min(&self) -> f64 {
        self.min
    }

    fn max(&self) -> f64 {
        self.max
    }
}

impl<'a> UnaryDensityFunction<'a> for LinearFunction<'a> {
    fn apply_density(&self, density: f64) -> f64 {
        match self.action {
            LinearType::Mul => density * self.arg,
            LinearType::Add => density + self.arg,
        }
    }
}

#[derive(Clone)]
pub enum BinaryType {
    Mul,
    Add,
    Min,
    Max,
}

#[derive(Clone)]
pub struct BinaryFunction<'a> {
    action: BinaryType,
    arg1: Arc<DensityFunction<'a>>,
    arg2: Arc<DensityFunction<'a>>,
    min: f64,
    max: f64,
}

impl<'a> BinaryFunction<'a> {
    pub fn create(
        action: BinaryType,
        arg1: Arc<DensityFunction<'a>>,
        arg2: Arc<DensityFunction<'a>>,
    ) -> DensityFunction<'a> {
        let d = arg1.min();
        let e = arg2.min();
        let f = arg1.max();
        let g = arg2.max();

        match action {
            BinaryType::Min | BinaryType::Max => {
                if d >= e || e >= f {
                    warn!("Density function does not overlap");
                }
            }
            _ => {}
        }

        let h = match action {
            BinaryType::Add => d + e,
            BinaryType::Mul => {
                if d > 0f64 && e > 0f64 {
                    d * e
                } else if f < 0f64 && g < 0f64 {
                    f * g
                } else {
                    (d * g).min(f * e)
                }
            }
            BinaryType::Min => d.min(e),
            BinaryType::Max => d.max(e),
        };

        let i = match action {
            BinaryType::Add => f + g,
            BinaryType::Mul => {
                if d > 0f64 && e > 0f64 {
                    f * g
                } else if f < 0f64 && g < 0f64 {
                    d * e
                } else {
                    (d * e).max(f * g)
                }
            }
            BinaryType::Min => f.min(g),
            BinaryType::Max => f.max(g),
        };

        match action {
            BinaryType::Mul | BinaryType::Add => {
                let action = match action {
                    BinaryType::Add => LinearType::Add,
                    BinaryType::Mul => LinearType::Mul,
                    _ => unreachable!(),
                };

                if let DensityFunction::Constant(func) = arg1.as_ref() {
                    return DensityFunction::Linear(LinearFunction {
                        action,
                        input: arg2,
                        min: h,
                        max: i,
                        arg: func.value,
                    });
                }

                if let DensityFunction::Constant(func) = arg2.as_ref() {
                    return DensityFunction::Linear(LinearFunction {
                        action,
                        input: arg1,
                        min: h,
                        max: i,
                        arg: func.value,
                    });
                }
            }
            _ => {}
        }

        DensityFunction::Binary(BinaryFunction {
            action,
            arg1,
            arg2,
            min: h,
            max: i,
        })
    }
}

impl<'a> DensityFunctionImpl<'a> for BinaryFunction<'a> {
    fn sample(&self, pos: &NoisePos) -> f64 {
        let d = self.arg1.sample(pos);
        let e = self.arg2.sample(pos);

        match self.action {
            BinaryType::Add => d + e,
            BinaryType::Mul => d * e,
            BinaryType::Min => {
                if d < self.arg2.min() {
                    d
                } else {
                    d.min(e)
                }
            }
            BinaryType::Max => {
                if d > self.arg2.max() {
                    d
                } else {
                    d.max(e)
                }
            }
        }
    }

    fn fill(&self, densities: &mut [f64], applier: &Applier<'a>) {
        self.arg1.fill(densities, applier);
        match self.action {
            BinaryType::Add => {
                let mut ds = Vec::with_capacity(densities.len());
                densities.iter().for_each(|_| ds.push(0f64));
                self.arg2.fill(&mut ds, applier);
                densities
                    .iter_mut()
                    .zip(ds)
                    .for_each(|(real, temp)| *real += temp);
            }
            BinaryType::Mul => {
                densities.iter_mut().enumerate().for_each(|(i, val)| {
                    if *val != 0f64 {
                        *val *= self.arg2.sample(&applier.at(i));
                    };
                });
            }
            BinaryType::Min => {
                let e = self.arg2.min();

                densities.iter_mut().enumerate().for_each(|(i, val)| {
                    if *val >= e {
                        *val = val.min(self.arg2.sample(&applier.at(i)));
                    }
                });
            }
            BinaryType::Max => {
                let e = self.arg2.max();

                densities.iter_mut().enumerate().for_each(|(i, val)| {
                    if *val <= e {
                        *val = val.max(self.arg2.sample(&applier.at(i)))
                    }
                });
            }
        }
    }

    fn apply(&self, visitor: &Visitor<'a>) -> Arc<DensityFunction<'a>> {
        visitor.apply(Arc::new(BinaryFunction::create(
            self.action.clone(),
            self.arg1.apply(visitor),
            self.arg2.apply(visitor),
        )))
    }

    fn max(&self) -> f64 {
        self.max
    }

    fn min(&self) -> f64 {
        self.min
    }
}
