use std::sync::Arc;

use anyhow::Context;
use bevy::math::{dvec2, DVec2};
use dyn_clone::DynClone;
use mlua::prelude::*;
use mlua::{UserData, Value};

use crate::AResult;

thread_local! {
	static luaInst: Lua = {
		let lua = Lua::new();
		lua.globals().set("Noise", NoiseCtors);
		lua
	};
}

pub fn construct_noisegen(code: &str) -> AResult<Arc<Noise>> {
	luaInst.with(|lua| {
		let chunk = lua.load(code);
		let noise = LuaErrorContext::context(
			chunk.call::<_, LuaAnyUserData>(()),
			"eval of Lua script failed",
		)?;
		let noise: Noise =
			LuaErrorContext::context(noise.take(), "Lua script did not return a Noise")?;
		Ok(Arc::new(noise))
	})
}

pub trait NoiseFunc: Send + Sync + DynClone {
	fn eval(&self, pos: DVec2) -> f32;
}

impl<Func: Clone + Send + Sync + Fn(DVec2) -> f32> NoiseFunc for Func {
	fn eval(&self, pos: DVec2) -> f32 {
		self(pos)
	}
}

type NoisePtr = Box<Noise>;

pub enum Noise {
	Const(f32),
	Func(Box<dyn NoiseFunc>),
	Simplex(i64),
	SimplexFast(i64),
	Octaves {
		func: NoisePtr,
		octaves: usize,
		ampScale: f32,
		freqScale: f32,
	},

	Add(NoisePtr, NoisePtr),
	Sub(NoisePtr, NoisePtr),
	Mul(NoisePtr, NoisePtr),
	Div(NoisePtr, NoisePtr),
	Pow(NoisePtr, NoisePtr),
	Rem(NoisePtr, NoisePtr),
	RemEuclid(NoisePtr, NoisePtr),
	SignedPow(NoisePtr, NoisePtr),
	Floor(NoisePtr),
	Ceil(NoisePtr),
	Abs(NoisePtr),
	Min(NoisePtr, NoisePtr),
	Max(NoisePtr, NoisePtr),
	Clamp {
		func: NoisePtr,
		min: f32,
		max: f32,
	},
	ToUnsignedUnit(NoisePtr),
	ToSignedUnit(NoisePtr),

	CoordTranslate(NoisePtr, DVec2),
	CoordScale(NoisePtr, DVec2),
}

impl Noise {
	pub fn eval(&self, pos: DVec2) -> f32 {
		use Noise::*;
		match self {
			&Const(v) => v,
			Func(func) => func.eval(pos),
			&Simplex(seed) => opensimplex2::smooth::noise2(seed, pos.x, pos.y),
			&SimplexFast(seed) => opensimplex2::fast::noise2(seed, pos.x, pos.y),
			Octaves {
				func,
				octaves,
				ampScale,
				freqScale,
			} => {
				let freqScale = *freqScale as f64;
				let mut res = 0.0;
				let mut amp = 1.0;
				let mut freq = 1.0;
				for _ in 0 .. *octaves {
					res += amp * func.eval(pos * freq);
					amp *= ampScale;
					freq *= freqScale;
				}
				res
			},

			Add(l, r) => l.eval(pos) + r.eval(pos),
			Sub(l, r) => l.eval(pos) - r.eval(pos),
			Mul(l, r) => l.eval(pos) * r.eval(pos),
			Div(l, r) => l.eval(pos) / r.eval(pos),
			Pow(l, r) => l.eval(pos).powf(r.eval(pos)),
			Rem(l, r) => l.eval(pos) % r.eval(pos),
			RemEuclid(l, r) => l.eval(pos).rem_euclid(r.eval(pos)),
			Floor(v) => v.eval(pos).floor(),
			Ceil(v) => v.eval(pos).ceil(),
			Abs(v) => v.eval(pos).abs(),
			Min(l, r) => l.eval(pos).min(r.eval(pos)),
			Max(l, r) => l.eval(pos).max(r.eval(pos)),
			Clamp { func, min, max } => func.eval(pos).clamp(*min, *max),
			ToUnsignedUnit(v) => (v.eval(pos) + 1.0) / 2.0,
			ToSignedUnit(v) => v.eval(pos) * 2.0 - 1.0,
			SignedPow(l, r) => {
				let l = l.eval(pos);
				let r = r.eval(pos);
				l.powf(r).copysign(l)
			},

			CoordTranslate(func, translation) => func.eval(pos + *translation),
			CoordScale(func, scale) => func.eval(pos * *scale),
		}
	}
}

impl Clone for Noise {
	fn clone(&self) -> Self {
		use Noise::*;
		match self {
			&Const(v) => Const(v),
			Func(f) => Func(dyn_clone::clone_box(&**f)),
			&Simplex(seed) => Simplex(seed),
			&SimplexFast(seed) => SimplexFast(seed),
			Octaves {
				func,
				octaves,
				freqScale,
				ampScale,
			} => Octaves {
				func: func.clone(),
				octaves: *octaves,
				freqScale: *freqScale,
				ampScale: *ampScale,
			},

			Add(l, r) => Add(l.clone(), r.clone()),
			Sub(l, r) => Sub(l.clone(), r.clone()),
			Mul(l, r) => Mul(l.clone(), r.clone()),
			Div(l, r) => Div(l.clone(), r.clone()),
			Pow(l, r) => Pow(l.clone(), r.clone()),
			Rem(l, r) => Rem(l.clone(), r.clone()),
			RemEuclid(l, r) => RemEuclid(l.clone(), r.clone()),
			SignedPow(l, r) => SignedPow(l.clone(), r.clone()),
			Floor(v) => Floor(v.clone()),
			Ceil(v) => Ceil(v.clone()),
			Abs(v) => Abs(v.clone()),
			Min(l, r) => Min(l.clone(), r.clone()),
			Max(l, r) => Max(l.clone(), r.clone()),
			Clamp { func, min, max } => Clamp {
				func: func.clone(),
				min: *min,
				max: *max,
			},
			ToUnsignedUnit(v) => ToUnsignedUnit(v.clone()),
			ToSignedUnit(v) => ToSignedUnit(v.clone()),

			CoordTranslate(f, v) => CoordTranslate(f.clone(), v.clone()),
			CoordScale(f, v) => CoordScale(f.clone(), v.clone()),
		}
	}
}

struct NoiseCtors;

impl UserData for NoiseCtors {
	fn add_methods<'lua, M: LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
		methods.add_function("const", |lua, val: f32| Ok(Noise::Const(val)));
		methods.add_function("simplex", |lua, seed: i64| Ok(Noise::Simplex(seed)));
		methods.add_function("simplexFast", |lua, seed: i64| Ok(Noise::SimplexFast(seed)));
	}
}

fn rhs_to_noise(rhs: &Value) -> mlua::Result<Noise> {
	Ok(if let Some(v) = rhs.as_number() {
		Noise::Const(v as _)
	} else if let Some(v) = rhs.as_integer() {
		Noise::Const(v as _)
	} else if let Some(v) = rhs.as_userdata() {
		v.borrow::<Noise>()?.clone()
	} else {
		return Err(LuaError::external("expected number or Noise"));
	})
}

impl UserData for Noise {
	fn add_methods<'lua, M: LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
		methods.add_method(
			"octaves",
			|_, this, (octaves, ampScale, freqScale): (usize, Option<f32>, Option<f32>)| {
				let ampScale = ampScale.unwrap_or(0.5);
				let freqScale = freqScale.unwrap_or(2.0);
				Ok(Noise::Octaves {
					func: this.clone().into(),
					octaves,
					ampScale,
					freqScale,
				})
			},
		);

		methods.add_meta_method(LuaMetaMethod::Add, |_, this, rhs: Value| {
			let rhs = rhs_to_noise(&rhs)?;
			Ok(Noise::Add(this.clone().into(), rhs.into()))
		});
		methods.add_meta_method(LuaMetaMethod::Sub, |_, this, rhs: Value| {
			let rhs = rhs_to_noise(&rhs)?;
			Ok(Noise::Sub(this.clone().into(), rhs.into()))
		});
		methods.add_meta_method(LuaMetaMethod::Mul, |_, this, rhs: Value| {
			let rhs = rhs_to_noise(&rhs)?;
			Ok(Noise::Mul(this.clone().into(), rhs.into()))
		});
		methods.add_meta_method(LuaMetaMethod::Div, |_, this, rhs: Value| {
			let rhs = rhs_to_noise(&rhs)?;
			Ok(Noise::Div(this.clone().into(), rhs.into()))
		});
		methods.add_meta_method(LuaMetaMethod::Pow, |_, this, rhs: Value| {
			let rhs = rhs_to_noise(&rhs)?;
			Ok(Noise::Pow(this.clone().into(), rhs.into()))
		});
		methods.add_meta_method(LuaMetaMethod::Mod, |_, this, rhs: Value| {
			let rhs = rhs_to_noise(&rhs)?;
			Ok(Noise::Rem(this.clone().into(), rhs.into()))
		});

		methods.add_method("remEuclid", |_, this, rhs: Value| {
			let rhs = rhs_to_noise(&rhs)?;
			Ok(Noise::RemEuclid(this.clone().into(), rhs.into()))
		});
		methods.add_method("floor", |_, this, rhs: ()| {
			Ok(Noise::Floor(this.clone().into()))
		});
		methods.add_method("ceil", |_, this, rhs: ()| {
			Ok(Noise::Ceil(this.clone().into()))
		});
		methods.add_method("abs", |_, this, rhs: ()| {
			Ok(Noise::Abs(this.clone().into()))
		});
		methods.add_method("min", |_, this, rhs: Value| {
			let rhs = rhs_to_noise(&rhs)?;
			Ok(Noise::Min(this.clone().into(), rhs.into()))
		});
		methods.add_method("max", |_, this, rhs: Value| {
			let rhs = rhs_to_noise(&rhs)?;
			Ok(Noise::Max(this.clone().into(), rhs.into()))
		});
		methods.add_method("clamp", |_, this, (min, max): (f32, f32)| {
			Ok(Noise::Clamp {
				func: this.clone().into(),
				min,
				max,
			})
		});
		methods.add_method("toSignedUnit", |_, this, rhs: ()| {
			Ok(Noise::ToSignedUnit(this.clone().into()))
		});
		methods.add_method("toUnsignedUnit", |_, this, rhs: ()| {
			Ok(Noise::ToUnsignedUnit(this.clone().into()))
		});
		methods.add_method("signedPow", |_, this, rhs: Value| {
			let rhs = rhs_to_noise(&rhs)?;
			Ok(Noise::SignedPow(this.clone().into(), rhs.into()))
		});

		methods.add_method("translate", |_, this, (x, y): (f64, Option<f64>)| {
			let y = y.unwrap_or(x);
			let translation = dvec2(x, y);
			Ok(Noise::CoordTranslate(this.clone().into(), translation))
		});
		methods.add_method("scale", |_, this, (x, y): (f64, Option<f64>)| {
			let y = y.unwrap_or(x);
			let scale = dvec2(x, y);
			Ok(Noise::CoordScale(this.clone().into(), scale))
		});
	}
}
