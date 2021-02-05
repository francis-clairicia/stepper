use core::{
    convert::{TryFrom, TryInto as _},
    task::Poll,
};

use embedded_hal::timer;
use embedded_time::duration::Nanoseconds;

use crate::traits::SetStepMode;

use super::{Error, Stepper};

/// A "future" that can be polled to complete a [`Stepper::set_step_mode`] call
///
/// Please note that this type provides a custom API and does not implement
/// [`core::future::Future`]. This might change, as using futures for embedded
/// development becomes more practical.
pub struct SetStepModeFuture<'r, Driver: SetStepMode, Timer> {
    step_mode: Driver::StepMode,
    stepper: &'r mut Stepper<Driver>,
    timer: &'r mut Timer,
    state: State,
}

impl<'r, Driver, Timer> SetStepModeFuture<'r, Driver, Timer>
where
    Driver: SetStepMode,
    Timer: timer::CountDown,
    Timer::Time: TryFrom<Nanoseconds>,
{
    pub(super) fn new(
        step_mode: Driver::StepMode,
        stepper: &'r mut Stepper<Driver>,
        timer: &'r mut Timer,
    ) -> Self {
        Self {
            step_mode,
            stepper,
            timer,
            state: State::Initial,
        }
    }

    /// Poll the future
    ///
    /// The future must be polled for the operation to make progress. The
    /// operation won't start, until this method has been called once. Returns
    /// [`Poll::Pending`], if the operation is not finished yet, or
    /// [`Poll::Ready`], once it is
    ///
    /// If this method returns [`Poll::Pending`], the user can opt to keep
    /// calling it at a high frequency (see [`Self::wait`]) until the operation
    /// completes, or set up an interrupt that fires once the timer finishes
    /// counting down, and call this method again once it does.
    pub fn poll(
        &mut self,
    ) -> Poll<
        Result<
            (),
            Error<
                Driver::Error,
                <Timer::Time as TryFrom<Nanoseconds>>::Error,
                Timer::Error,
            >,
        >,
    > {
        match self.state {
            State::Initial => {
                self.stepper
                    .driver
                    .apply_mode_config(self.step_mode)
                    .map_err(|err| Error::Pin(err))?;

                let ticks: Timer::Time = Driver::SETUP_TIME
                    .try_into()
                    .map_err(|err| Error::TimeConversion(err))?;
                self.timer
                    .try_start(ticks)
                    .map_err(|err| Error::Timer(err))?;

                self.state = State::ApplyingConfig;
                Poll::Pending
            }
            State::ApplyingConfig => match self.timer.try_wait() {
                Ok(()) => {
                    self.stepper
                        .driver
                        .enable_driver()
                        .map_err(|err| Error::Pin(err))?;

                    let ticks: Timer::Time = Driver::HOLD_TIME
                        .try_into()
                        .map_err(|err| Error::TimeConversion(err))?;
                    self.timer
                        .try_start(ticks)
                        .map_err(|err| Error::Timer(err))?;

                    self.state = State::EnablingDriver;
                    Poll::Ready(Ok(()))
                }
                Err(nb::Error::Other(err)) => {
                    self.state = State::Finished;
                    Poll::Ready(Err(Error::Timer(err)))
                }
                Err(nb::Error::WouldBlock) => Poll::Pending,
            },
            State::EnablingDriver => match self.timer.try_wait() {
                Ok(()) => {
                    self.state = State::Finished;
                    Poll::Ready(Ok(()))
                }
                Err(nb::Error::Other(err)) => {
                    self.state = State::Finished;
                    Poll::Ready(Err(Error::Timer(err)))
                }
                Err(nb::Error::WouldBlock) => Poll::Pending,
            },
            // block!(timer.try_wait()).map_err(|err| Error::Timer(err))?;
            State::Finished => Poll::Ready(Ok(())),
        }
    }

    /// Wait until the operation completes
    ///
    /// This method will call [`Self::poll`] in a busy loop until the operation
    /// has finished.
    pub fn wait(
        &mut self,
    ) -> Result<
        (),
        Error<
            Driver::Error,
            <Timer::Time as TryFrom<Nanoseconds>>::Error,
            Timer::Error,
        >,
    > {
        loop {
            if let Poll::Ready(result) = self.poll() {
                return result;
            }
        }
    }
}

enum State {
    Initial,
    ApplyingConfig,
    EnablingDriver,
    Finished,
}
