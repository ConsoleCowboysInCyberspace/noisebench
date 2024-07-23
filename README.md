# noisebench
A live-updating playground for viewing noise generation algorithms as heightmaps.

## Lua API
Algorithms are built within Lua scripts, which should be placed in `assets/scripts`. The following API is available:
```lua
-- [[ Constructors ]] --
algo = Noise.const(value) -- constant value
algo = Noise.simplex(seed) -- OpenSimplex2 Smooth variant with given seed
algo = Noise.simplexFast(seed) -- OpenSimplex2 Fast variant
algo = Noise.sinefield(freq, amp) -- sin(x) + cos(y) with given frequency and amplitude defaulting to 1 for both

-- [[ Basic arithmetic operations ]] --
algo = algo + 2
algo = algo - 2
algo = algo * 2
algo = algo / 2
algo = algo ^ 2 -- powf (exponentiation)
algo = algo % 2 -- modf (modulo)

-- [[ Methods ]] --
algo = algo:rem_euclid() -- Euclidean remainder, see Rust docs for `f32::rem_euclid`
algo = algo:floor()
algo = algo:ceil()
algo = algo:abs()
algo = algo:min(value)
algo = algo:max(value)
algo = algo:clamp(min, max)
algo = algo:toSignedUnit() -- unsigned unit interval to signed ([0, 1] -> [-1, 1])
algo = algo:toSignedUnit() -- signed unit interval to unsigned ([-1, 1] -> [0, 1])
algo = algo:signedPow() -- powf, but preserves sign of input
algo = algo:translate(x, y) -- translates input coordinates
algo = algo:scale(x, y) -- scales input coordinates

-- builds fractal noise by stacking `octaves` samples at (by default) doubled frequencies with halved amplitudes
-- recommended to keep `ampScale` between (0, 1] and `freqScale` > 1
-- query your favorite search engine for "fractal noise" to learn more
algo = algo:octaves(octaves, ampScale, freqScale)

-- other algos can be used instead of constants
algo2 = Noise.simplex(42)
algo3 = Noise.sinefield()
algo = algo + algo2
algo = algo:min(algo3)

-- [[ Every script must end by returning the final output algorithm ]] --
return algo
```
