// Copyright 2018 Developers of the Rand project.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Thread-local random number generator

use core::cell::UnsafeCell;

use super::std::Core;
use crate::rngs::adapter::ReseedingRng;
use crate::rngs::OsRng;
use crate::{CryptoRng, Error, RngCore, SeedableRng};
use std::marker::PhantomData;

// Rationale for using `UnsafeCell` in `ThreadRng`:
//
// Previously we used a `RefCell`, with an overhead of ~15%. There will only
// ever be one mutable reference to the interior of the `UnsafeCell`, because
// we only have such a reference inside `next_u32`, `next_u64`, etc. Within a
// single thread (which is the definition of `ThreadRng`), there will only ever
// be one of these methods active at a time.
//
// A possible scenario where there could be multiple mutable references is if
// `ThreadRng` is used inside `next_u32` and co. But the implementation is
// completely under our control. We just have to ensure none of them use
// `ThreadRng` internally, which is nonsensical anyway. We should also never run
// `ThreadRng` in destructors of its implementation, which is also nonsensical.


// Number of generated bytes after which to reseed `ThreadRng`.
// According to benchmarks, reseeding has a noticable impact with thresholds
// of 32 kB and less. We choose 64 kB to avoid significant overhead.
const THREAD_RNG_RESEED_THRESHOLD: u64 = 1024 * 64;

/// The type returned by [`thread_rng`], essentially just a reference to the
/// PRNG in thread-local memory.
///
/// `ThreadRng` uses the same PRNG as [`StdRng`] for security and performance.
/// As hinted by the name, the generator is thread-local. `ThreadRng` is a
/// handle to this generator and thus supports `Copy`, but not `Send` or `Sync`.
///
/// Unlike `StdRng`, `ThreadRng` uses the  [`ReseedingRng`] wrapper to reseed
/// the PRNG from fresh entropy every 64 kiB of random data.
/// [`OsRng`] is used to provide seed data.
///
/// Note that the reseeding is done as an extra precaution against side-channel
/// attacks and mis-use (e.g. if somehow weak entropy were supplied initially).
/// The PRNG algorithms used are assumed to be secure.
///
/// [`ReseedingRng`]: crate::rngs::adapter::ReseedingRng
/// [`StdRng`]: crate::rngs::StdRng
#[derive(Copy, Clone, Debug)]
pub struct ThreadRng {
    // inner raw pointer implies type is neither Send nor Sync
    opaque: PhantomData<*mut ()>
}

thread_local!(
    static THREAD_RNG_KEY: UnsafeCell<ReseedingRng<Core, OsRng>> = {
        let r = Core::from_rng(OsRng).unwrap_or_else(|err|
                panic!("could not initialize thread_rng: {}", err));
        let rng = ReseedingRng::new(r,
                                    THREAD_RNG_RESEED_THRESHOLD,
                                    OsRng);
        UnsafeCell::new(rng)
    }
);

/// Retrieve the lazily-initialized thread-local random number generator,
/// seeded by the system. Intended to be used in method chaining style,
/// e.g. `thread_rng().gen::<i32>()`, or cached locally, e.g.
/// `let mut rng = thread_rng();`.  Invoked by the `Default` trait, making
/// `ThreadRng::default()` equivalent.
///
/// For more information see [`ThreadRng`].
pub fn thread_rng() -> ThreadRng {
    ThreadRng {
        opaque: PhantomData
    }
}

impl Default for ThreadRng {
    fn default() -> ThreadRng {
        crate::prelude::thread_rng()
    }
}

impl RngCore for ThreadRng {
    #[inline(always)]
    fn next_u32(&mut self) -> u32 {
        THREAD_RNG_KEY.with(|rng| {
            unsafe { (*rng.get()).next_u32() }
        })
    }

    #[inline(always)]
    fn next_u64(&mut self) -> u64 {
        THREAD_RNG_KEY.with(|rng| {
            unsafe { (*rng.get()).next_u64() }
        })
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        THREAD_RNG_KEY.with(|rng| {
            unsafe { (*rng.get()).fill_bytes(dest) }
        })
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
        THREAD_RNG_KEY.try_with(|rng| {
            unsafe { (*rng.get()).try_fill_bytes(dest) }
        }).map_err(|e| Error::new(e))?
    }
}

impl CryptoRng for ThreadRng {}


#[cfg(test)]
mod test {
    #[test]
    fn test_thread_rng() {
        use crate::Rng;
        let mut r = crate::thread_rng();
        r.gen::<i32>();
        assert_eq!(r.gen_range(0, 1), 0);
    }

    // Causes use-after-free on OSX. The following flags are needed to disable the "fast"
    // implementation on OSX and turn use-after-destroy into use-after-free.
    // CARGO_BUILD_RUSTFLAGS="-C link-arg=-mmacosx-version-min=10.14" MACOSX_DEPLOYMENT_TARGET=10.6 cargo test test_lifetime
    #[test]
    #[cfg(feature = "std")]
    fn test_lifetime(){
        use std::thread::spawn;
        use crate::Rng;

        struct Zombie(crate::rngs::ThreadRng);
        thread_local!(
            static ZOMBIE: Zombie = Zombie(crate::thread_rng());
        );
        impl Drop for Zombie {
            fn drop(&mut self) {
                self.0.gen_bool(0.5);
            }
        }
        for i in 0..100{
            spawn(move || {
                crate::thread_rng();
                ZOMBIE.with(|_| {});
            }).join().unwrap();
        }
    }
}

