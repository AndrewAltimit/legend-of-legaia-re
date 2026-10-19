//! A small interpreter over naga IR, for the host shader-parity tests.
//!
//! Both play hosts' shaders are parsed into naga's IR (WGSL for the native
//! renderer, GLSL for the browser play page) and run here on the CPU, so one
//! set of inputs can be pushed through both texts and the outputs compared.
//! It covers the IR the vertex and fragment shaders of this repo use -
//! scalar, vector and matrix arithmetic, arrays, structs, locals, loops,
//! calls, `discard`, texel loads and filtered / comparison samples through
//! host-supplied mocks, and screen-space derivatives of stage inputs off
//! host-supplied gradients - and panics, naming the construct, on anything
//! else, so a shader that grows a new construct fails loudly rather than
//! evaluating wrongly.
//!
//! What it does not model: GPU float rounding and real texture filtering (a
//! test's mock decides what a sample returns). The comparison is of the two
//! laws as written, evaluated with the same arithmetic.

use naga::{
    Arena, BinaryOperator as Bo, Expression as E, Function, Handle, Literal, MathFunction as Mf,
    Module, RelationalFunction, ScalarKind, Statement as S, TypeInner, UnaryOperator,
};
use std::collections::HashMap;

/// A runtime value.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    F(Vec<f32>),
    U(Vec<u32>),
    I(Vec<i32>),
    B(Vec<bool>),
    Arr(Vec<Val>),
    Struct(Vec<Val>),
    /// A pointer into a local or global, with the component path below it.
    Ptr(Root, Vec<usize>),
    /// An opaque resource (image or sampler) global.
    Resource(Handle<naga::GlobalVariable>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Root {
    Local(usize),
    Global(usize),
}

impl Val {
    pub fn f(v: &[f32]) -> Val {
        Val::F(v.to_vec())
    }
    pub fn u(v: &[u32]) -> Val {
        Val::U(v.to_vec())
    }
    pub fn floats(&self) -> &[f32] {
        match self {
            Val::F(v) => v,
            other => panic!("expected floats, got {other:?}"),
        }
    }
    fn len(&self) -> usize {
        match self {
            Val::F(v) => v.len(),
            Val::U(v) => v.len(),
            Val::I(v) => v.len(),
            Val::B(v) => v.len(),
            Val::Arr(v) | Val::Struct(v) => v.len(),
            _ => 1,
        }
    }
    fn index(&self, i: usize) -> Val {
        match self {
            Val::F(v) => Val::F(vec![v[i]]),
            Val::U(v) => Val::U(vec![v[i]]),
            Val::I(v) => Val::I(vec![v[i]]),
            Val::B(v) => Val::B(vec![v[i]]),
            Val::Arr(v) | Val::Struct(v) => v[i].clone(),
            other => panic!("index into {other:?}"),
        }
    }
    fn set_index(&mut self, i: usize, x: Val) {
        match (self, x) {
            (Val::F(v), Val::F(x)) => v[i] = x[0],
            (Val::U(v), Val::U(x)) => v[i] = x[0],
            (Val::I(v), Val::I(x)) => v[i] = x[0],
            (Val::B(v), Val::B(x)) => v[i] = x[0],
            (Val::Arr(v) | Val::Struct(v), x) => v[i] = x,
            (s, x) => panic!("store {x:?} into {s:?}"),
        }
    }
    fn truthy(&self) -> bool {
        match self {
            Val::B(v) if v.len() == 1 => v[0],
            other => panic!("condition {other:?}"),
        }
    }
    fn as_index(&self) -> usize {
        match self {
            Val::U(v) => v[0] as usize,
            Val::I(v) => v[0] as usize,
            other => panic!("index value {other:?}"),
        }
    }
}

enum Flow {
    Next,
    Break,
    Continue,
    Return(Option<Val>),
    Kill,
}

/// One texel fetch: `(global name, x, y, layer)` -> the `vec4<u32>` it loads.
pub type ImageFetch = dyn Fn(&str, i32, i32) -> [u32; 4];

/// One float texel load (a sampled-float or depth image): `(global name, x,
/// y)` -> the texel; a depth image reads lane 0.
pub type FloatFetch = dyn Fn(&str, i32, i32) -> [f32; 4];

/// One filtered sample: `(image global name, uv, array layer, depth
/// reference)` -> the sampled value; a comparison sample reads lane 0 as its
/// 0..1 visibility. The host's mock decides what filtering means.
pub type SampleFn = dyn Fn(&str, [f32; 2], Option<i32>, Option<f32>) -> [f32; 4];

/// A module plus its global state.
pub struct Machine<'m> {
    pub module: &'m Module,
    pub globals: Vec<Val>,
    fetch: Box<ImageFetch>,
    /// Float texel loads; unset, a float load panics.
    pub fetch_f: Option<Box<FloatFetch>>,
    /// Filtered samples; unset, a sample panics.
    pub sample: Option<Box<SampleFn>>,
    /// Screen-space derivatives of named inputs (a WGSL entry argument's
    /// struct member, or a GLSL stage-input global): `name -> (d/dx, d/dy)`.
    /// A derivative of anything else panics, and so does one of an input
    /// with no entry here.
    pub gradients: HashMap<String, (Vec<f32>, Vec<f32>)>,
}

struct Frame {
    exprs: Vec<Option<Val>>,
    locals: Vec<Val>,
    args: Vec<Val>,
    arg_types: Vec<Handle<naga::Type>>,
}

impl<'m> Machine<'m> {
    pub fn new(module: &'m Module, fetch: Box<ImageFetch>) -> Self {
        let mut m = Machine {
            module,
            globals: Vec::new(),
            fetch,
            fetch_f: None,
            sample: None,
            gradients: HashMap::new(),
        };
        let mut globals = Vec::new();
        for (_, g) in module.global_variables.iter() {
            let v = match g.init {
                Some(h) => m.eval_const(h),
                None => m.zero(g.ty),
            };
            globals.push(v);
        }
        m.globals = globals;
        m
    }

    fn global_index(&self, name: &str) -> usize {
        self.module
            .global_variables
            .iter()
            .position(|(_, g)| g.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("no global `{name}`"))
    }

    /// Set a named global (a GLSL stage input, `gl_FragCoord`, ...).
    pub fn set_global(&mut self, name: &str, v: Val) {
        let i = self.global_index(name);
        self.globals[i] = v;
    }

    pub fn global(&self, name: &str) -> &Val {
        &self.globals[self.global_index(name)]
    }

    /// Set the member `member` of whichever uniform-struct global declares it.
    pub fn set_uniform(&mut self, member: &str, v: Val) {
        for (i, (_, g)) in self.module.global_variables.iter().enumerate() {
            if g.space != naga::AddressSpace::Uniform {
                continue;
            }
            if let TypeInner::Struct { members, .. } = &self.module.types[g.ty].inner
                && let Some(k) = members
                    .iter()
                    .position(|m| m.name.as_deref() == Some(member))
            {
                let want = self.zero(members[k].ty);
                assert_eq!(
                    std::mem::discriminant(&want),
                    std::mem::discriminant(&v),
                    "uniform `{member}` kind"
                );
                assert_eq!(want.len(), v.len(), "uniform `{member}` size");
                self.globals[i].set_index(k, v);
                return;
            }
        }
        panic!("no uniform member `{member}`");
    }

    pub fn function(&self, name: &str) -> &'m Function {
        let module: &'m Module = self.module;
        if let Some((_, f)) = module
            .functions
            .iter()
            .find(|(_, f)| f.name.as_deref() == Some(name))
        {
            return f;
        }
        module
            .entry_points
            .iter()
            .find(|e| e.name == name)
            .map(|e| &e.function)
            .unwrap_or_else(|| panic!("no function `{name}`"))
    }

    /// The zero value of a type.
    pub fn zero(&self, ty: Handle<naga::Type>) -> Val {
        match &self.module.types[ty].inner {
            TypeInner::Scalar(s) => zero_scalar(s.kind, 1),
            TypeInner::Vector { size, scalar } => zero_scalar(scalar.kind, *size as usize),
            TypeInner::Matrix { columns, rows, .. } => {
                Val::Arr(vec![Val::F(vec![0.0; *rows as usize]); *columns as usize])
            }
            TypeInner::Array { base, size, .. } => {
                let n = match size {
                    naga::ArraySize::Constant(n) => n.get() as usize,
                    other => panic!("array size {other:?}"),
                };
                Val::Arr(vec![self.zero(*base); n])
            }
            TypeInner::Struct { members, .. } => {
                Val::Struct(members.iter().map(|m| self.zero(m.ty)).collect())
            }
            TypeInner::Image { .. } | TypeInner::Sampler { .. } => Val::Struct(Vec::new()),
            other => panic!("zero of {other:?}"),
        }
    }

    /// The members of a struct type, by name, for building an argument.
    pub fn struct_member_names(&self, ty: Handle<naga::Type>) -> Vec<String> {
        match &self.module.types[ty].inner {
            TypeInner::Struct { members, .. } => members
                .iter()
                .map(|m| m.name.clone().unwrap_or_default())
                .collect(),
            other => panic!("not a struct: {other:?}"),
        }
    }

    fn eval_const(&self, h: Handle<E>) -> Val {
        let mut frame = Frame {
            exprs: vec![None; self.module.global_expressions.len()],
            locals: Vec::new(),
            args: Vec::new(),
            arg_types: Vec::new(),
        };
        self.eval(&self.module.global_expressions, &mut frame, h)
    }

    /// Call `f` with `args`. `Err(())` is a `discard`.
    pub fn call(&mut self, f: &'m Function, args: Vec<Val>) -> Result<Option<Val>, ()> {
        let mut frame = Frame {
            exprs: vec![None; f.expressions.len()],
            locals: Vec::new(),
            args,
            arg_types: f.arguments.iter().map(|a| a.ty).collect(),
        };
        for (_, l) in f.local_variables.iter() {
            let v = match l.init {
                Some(h) => self.eval(&f.expressions, &mut frame, h),
                None => self.zero(l.ty),
            };
            frame.locals.push(v);
        }
        match self.exec(f, &mut frame, &f.body) {
            Flow::Return(v) => Ok(v),
            Flow::Next => Ok(None),
            Flow::Kill => Err(()),
            Flow::Break | Flow::Continue => panic!("break outside a loop"),
        }
    }

    fn exec(&mut self, f: &'m Function, frame: &mut Frame, block: &'m naga::Block) -> Flow {
        for st in block.iter() {
            let flow = match st {
                S::Emit(range) => {
                    for h in range.clone() {
                        frame.exprs[h.index()] = None;
                        let v = self.eval(&f.expressions, frame, h);
                        frame.exprs[h.index()] = Some(v);
                    }
                    Flow::Next
                }
                S::Block(b) => self.exec(f, frame, b),
                S::If {
                    condition,
                    accept,
                    reject,
                } => {
                    if self.eval(&f.expressions, frame, *condition).truthy() {
                        self.exec(f, frame, accept)
                    } else {
                        self.exec(f, frame, reject)
                    }
                }
                S::Switch { selector, cases } => {
                    let sel = self.eval(&f.expressions, frame, *selector);
                    let key = match &sel {
                        Val::I(v) => v[0] as i64,
                        Val::U(v) => v[0] as i64,
                        other => panic!("switch on {other:?}"),
                    };
                    let start = cases
                        .iter()
                        .position(|c| match c.value {
                            naga::SwitchValue::I32(x) => x as i64 == key,
                            naga::SwitchValue::U32(x) => x as i64 == key,
                            naga::SwitchValue::Default => false,
                        })
                        .or_else(|| {
                            cases
                                .iter()
                                .position(|c| c.value == naga::SwitchValue::Default)
                        });
                    let mut out = Flow::Next;
                    if let Some(mut i) = start {
                        loop {
                            match self.exec(f, frame, &cases[i].body) {
                                Flow::Break => break,
                                Flow::Next if cases[i].fall_through && i + 1 < cases.len() => {
                                    i += 1;
                                }
                                Flow::Next => break,
                                other => {
                                    out = other;
                                    break;
                                }
                            }
                        }
                    }
                    out
                }
                S::Loop {
                    body,
                    continuing,
                    break_if,
                } => {
                    let mut out = Flow::Next;
                    for _ in 0..1_000_000 {
                        match self.exec(f, frame, body) {
                            Flow::Break => break,
                            Flow::Next | Flow::Continue => {}
                            other => {
                                out = other;
                                break;
                            }
                        }
                        match self.exec(f, frame, continuing) {
                            Flow::Next => {}
                            Flow::Break => break,
                            other => {
                                out = other;
                                break;
                            }
                        }
                        if let Some(b) = break_if
                            && self.eval(&f.expressions, frame, *b).truthy()
                        {
                            break;
                        }
                    }
                    out
                }
                S::Break => Flow::Break,
                S::Continue => Flow::Continue,
                S::Return { value } => {
                    Flow::Return(value.map(|h| self.eval(&f.expressions, frame, h)))
                }
                S::Kill => Flow::Kill,
                S::Store { pointer, value } => {
                    let p = self.eval(&f.expressions, frame, *pointer);
                    let v = self.eval(&f.expressions, frame, *value);
                    self.store(frame, &p, v);
                    Flow::Next
                }
                S::Call {
                    function,
                    arguments,
                    result,
                } => {
                    let args = arguments
                        .iter()
                        .map(|a| self.eval(&f.expressions, frame, *a))
                        .collect();
                    let module: &'m Module = self.module;
                    let callee = &module.functions[*function];
                    match self.call(callee, args) {
                        Ok(v) => {
                            if let (Some(r), Some(v)) = (result, v) {
                                frame.exprs[r.index()] = Some(v);
                            }
                            Flow::Next
                        }
                        Err(()) => Flow::Kill,
                    }
                }
                other => panic!("statement not modelled: {other:?}"),
            };
            match flow {
                Flow::Next => {}
                other => return other,
            }
        }
        Flow::Next
    }

    fn store(&mut self, frame: &mut Frame, p: &Val, v: Val) {
        let Val::Ptr(root, path) = p else {
            panic!("store through {p:?}")
        };
        let mut slot = match root {
            Root::Local(i) => &mut frame.locals[*i],
            Root::Global(i) => &mut self.globals[*i],
        };
        for &k in path {
            slot = match slot {
                Val::Arr(v) | Val::Struct(v) => &mut v[k],
                leaf => {
                    leaf.set_index(k, v);
                    return;
                }
            };
        }
        *slot = v;
    }

    fn load(&self, frame: &Frame, p: &Val) -> Val {
        let Val::Ptr(root, path) = p else {
            panic!("load through {p:?}")
        };
        let mut v = match root {
            Root::Local(i) => frame.locals[*i].clone(),
            Root::Global(i) => self.globals[*i].clone(),
        };
        for &k in path {
            v = v.index(k);
        }
        v
    }

    /// The stage input a derivative is taken of.
    fn input_name(&self, arena: &Arena<E>, frame: &Frame, h: Handle<E>) -> String {
        match &arena[h] {
            E::Load { pointer } => match &arena[*pointer] {
                E::GlobalVariable(g) => self.module.global_variables[*g]
                    .name
                    .clone()
                    .unwrap_or_default(),
                other => panic!("derivative of a load through {other:?}"),
            },
            E::AccessIndex { base, index } => match &arena[*base] {
                E::FunctionArgument(a) => {
                    let names = self.struct_member_names(frame.arg_types[*a as usize]);
                    names[*index as usize].clone()
                }
                other => panic!("derivative of a member of {other:?}"),
            },
            other => panic!("derivative of {other:?}"),
        }
    }

    fn eval(&self, arena: &Arena<E>, frame: &mut Frame, h: Handle<E>) -> Val {
        if let Some(v) = &frame.exprs[h.index()] {
            return v.clone();
        }
        match &arena[h] {
            E::Literal(l) => literal(l),
            E::Constant(c) => self.eval_const(self.module.constants[*c].init),
            E::ZeroValue(ty) => self.zero(*ty),
            E::Compose { ty, components } => {
                let parts: Vec<Val> = components
                    .iter()
                    .map(|c| self.eval(arena, frame, *c))
                    .collect();
                match &self.module.types[*ty].inner {
                    TypeInner::Vector { .. } => flatten(parts),
                    TypeInner::Array { .. } => Val::Arr(parts),
                    TypeInner::Struct { .. } => Val::Struct(parts),
                    TypeInner::Matrix { .. } => Val::Arr(parts),
                    other => panic!("compose {other:?}"),
                }
            }
            E::Access { base, index } => {
                let b = self.eval(arena, frame, *base);
                let i = self.eval(arena, frame, *index).as_index();
                access(b, i)
            }
            E::AccessIndex { base, index } => {
                let b = self.eval(arena, frame, *base);
                access(b, *index as usize)
            }
            E::Splat { size, value } => {
                let v = self.eval(arena, frame, *value);
                flatten(vec![v; *size as usize])
            }
            E::Swizzle {
                size,
                vector,
                pattern,
            } => {
                let v = self.eval(arena, frame, *vector);
                flatten(
                    pattern[..*size as usize]
                        .iter()
                        .map(|c| v.index(*c as usize))
                        .collect(),
                )
            }
            E::FunctionArgument(i) => frame.args[*i as usize].clone(),
            E::GlobalVariable(g) => {
                let var = &self.module.global_variables[*g];
                if var.space == naga::AddressSpace::Handle {
                    Val::Resource(*g)
                } else {
                    Val::Ptr(Root::Global(g.index()), Vec::new())
                }
            }
            E::LocalVariable(l) => Val::Ptr(Root::Local(l.index()), Vec::new()),
            E::Load { pointer } => {
                let p = self.eval(arena, frame, *pointer);
                self.load(frame, &p)
            }
            E::ImageLoad {
                image,
                coordinate,
                array_index,
                ..
            } => {
                assert!(array_index.is_none(), "arrayed image load");
                let Val::Resource(g) = self.eval(arena, frame, *image) else {
                    panic!("image load from a non-resource")
                };
                let c = self.eval(arena, frame, *coordinate);
                let (x, y) = match &c {
                    Val::I(v) => (v[0], v[1]),
                    Val::U(v) => (v[0] as i32, v[1] as i32),
                    other => panic!("image coordinate {other:?}"),
                };
                let var = &self.module.global_variables[g];
                let name = var.name.clone().unwrap_or_default();
                match &self.module.types[var.ty].inner {
                    TypeInner::Image {
                        class:
                            naga::ImageClass::Sampled {
                                kind: ScalarKind::Uint,
                                ..
                            },
                        ..
                    } => Val::U((self.fetch)(&name, x, y).to_vec()),
                    TypeInner::Image {
                        class:
                            naga::ImageClass::Sampled {
                                kind: ScalarKind::Float,
                                ..
                            },
                        ..
                    } => Val::F(self.fetch_f.as_ref().expect("float fetch")(&name, x, y).to_vec()),
                    TypeInner::Image {
                        class: naga::ImageClass::Depth { .. },
                        ..
                    } => Val::F(vec![
                        self.fetch_f.as_ref().expect("float fetch")(&name, x, y)[0],
                    ]),
                    other => panic!("image load from {other:?}"),
                }
            }
            E::ImageSample {
                image,
                coordinate,
                array_index,
                depth_ref,
                gather,
                offset,
                ..
            } => {
                assert!(
                    gather.is_none() && offset.is_none(),
                    "gather / offset sample"
                );
                let Val::Resource(g) = self.eval(arena, frame, *image) else {
                    panic!("sample from a non-resource")
                };
                let name = self.module.global_variables[g]
                    .name
                    .clone()
                    .unwrap_or_default();
                let uv = self.eval(arena, frame, *coordinate);
                let uv = [uv.floats()[0], uv.floats()[1]];
                let layer = array_index.map(|h| match self.eval(arena, frame, h) {
                    Val::I(v) => v[0],
                    Val::U(v) => v[0] as i32,
                    Val::F(v) => v[0] as i32,
                    other => panic!("array index {other:?}"),
                });
                let dref = depth_ref.map(|h| self.eval(arena, frame, h).floats()[0]);
                let r = self.sample.as_ref().expect("sampler")(&name, uv, layer, dref);
                if dref.is_some() {
                    Val::F(vec![r[0]])
                } else {
                    Val::F(r.to_vec())
                }
            }
            E::Unary { op, expr } => {
                let v = self.eval(arena, frame, *expr);
                unary(*op, v)
            }
            E::Binary { op, left, right } => {
                let a = self.eval(arena, frame, *left);
                let b = self.eval(arena, frame, *right);
                binary(*op, a, b)
            }
            E::Select {
                condition,
                accept,
                reject,
            } => {
                let c = self.eval(arena, frame, *condition);
                let a = self.eval(arena, frame, *accept);
                let r = self.eval(arena, frame, *reject);
                match c {
                    Val::B(cv) if cv.len() == 1 => {
                        if cv[0] {
                            a
                        } else {
                            r
                        }
                    }
                    Val::B(cv) => flatten(
                        cv.iter()
                            .enumerate()
                            .map(|(i, &t)| if t { a.index(i) } else { r.index(i) })
                            .collect(),
                    ),
                    other => panic!("select on {other:?}"),
                }
            }
            E::Derivative { axis, expr, .. } => {
                let name = self.input_name(arena, frame, *expr);
                let (dx, dy) = self
                    .gradients
                    .get(&name)
                    .unwrap_or_else(|| panic!("no gradient for input `{name}`"));
                Val::F(match axis {
                    naga::DerivativeAxis::X => dx.clone(),
                    naga::DerivativeAxis::Y => dy.clone(),
                    naga::DerivativeAxis::Width => {
                        dx.iter().zip(dy).map(|(a, b)| a.abs() + b.abs()).collect()
                    }
                })
            }
            E::Relational { fun, argument } => {
                let v = self.eval(arena, frame, *argument);
                match (fun, v) {
                    (RelationalFunction::All, Val::B(b)) => Val::B(vec![b.iter().all(|x| *x)]),
                    (RelationalFunction::Any, Val::B(b)) => Val::B(vec![b.iter().any(|x| *x)]),
                    (RelationalFunction::IsNan, Val::F(f)) => {
                        Val::B(f.iter().map(|x| x.is_nan()).collect())
                    }
                    (RelationalFunction::IsInf, Val::F(f)) => {
                        Val::B(f.iter().map(|x| x.is_infinite()).collect())
                    }
                    (f, v) => panic!("relational {f:?} on {v:?}"),
                }
            }
            E::Math {
                fun,
                arg,
                arg1,
                arg2,
                ..
            } => {
                let a = self.eval(arena, frame, *arg);
                let b = arg1.map(|h| self.eval(arena, frame, h));
                let c = arg2.map(|h| self.eval(arena, frame, h));
                math(*fun, a, b, c)
            }
            E::As {
                expr,
                kind,
                convert,
            } => {
                let v = self.eval(arena, frame, *expr);
                cast(v, *kind, convert.is_some())
            }
            E::CallResult(_) => panic!("call result read before its call"),
            other => panic!("expression not modelled: {other:?}"),
        }
    }
}

fn zero_scalar(kind: ScalarKind, n: usize) -> Val {
    match kind {
        ScalarKind::Float | ScalarKind::AbstractFloat => Val::F(vec![0.0; n]),
        ScalarKind::Uint => Val::U(vec![0; n]),
        ScalarKind::Sint | ScalarKind::AbstractInt => Val::I(vec![0; n]),
        ScalarKind::Bool => Val::B(vec![false; n]),
    }
}

fn literal(l: &Literal) -> Val {
    match *l {
        Literal::F32(x) => Val::F(vec![x]),
        Literal::F64(x) | Literal::AbstractFloat(x) => Val::F(vec![x as f32]),
        Literal::U32(x) => Val::U(vec![x]),
        Literal::I32(x) => Val::I(vec![x]),
        Literal::AbstractInt(x) | Literal::I64(x) => Val::I(vec![x as i32]),
        Literal::U64(x) => Val::U(vec![x as u32]),
        Literal::Bool(b) => Val::B(vec![b]),
        Literal::F16(x) => Val::F(vec![f32::from(x)]),
    }
}

/// Concatenate scalar / vector parts into one vector.
fn flatten(parts: Vec<Val>) -> Val {
    let mut it = parts.into_iter();
    let mut out = it.next().expect("empty compose");
    for p in it {
        match (&mut out, p) {
            (Val::F(a), Val::F(b)) => a.extend(b),
            (Val::U(a), Val::U(b)) => a.extend(b),
            (Val::I(a), Val::I(b)) => a.extend(b),
            (Val::B(a), Val::B(b)) => a.extend(b),
            (a, b) => panic!("compose {a:?} with {b:?}"),
        }
    }
    out
}

fn access(base: Val, i: usize) -> Val {
    match base {
        Val::Ptr(root, mut path) => {
            path.push(i);
            Val::Ptr(root, path)
        }
        v => v.index(i),
    }
}

/// Broadcast a scalar to `n` lanes.
fn widen(v: Val, n: usize) -> Val {
    if v.len() == n || v.len() != 1 {
        return v;
    }
    match v {
        Val::F(x) => Val::F(vec![x[0]; n]),
        Val::U(x) => Val::U(vec![x[0]; n]),
        Val::I(x) => Val::I(vec![x[0]; n]),
        Val::B(x) => Val::B(vec![x[0]; n]),
        other => other,
    }
}

fn unary(op: UnaryOperator, v: Val) -> Val {
    match (op, v) {
        (UnaryOperator::Negate, Val::F(x)) => Val::F(x.iter().map(|a| -a).collect()),
        (UnaryOperator::Negate, Val::I(x)) => Val::I(x.iter().map(|a| a.wrapping_neg()).collect()),
        (UnaryOperator::LogicalNot, Val::B(x)) => Val::B(x.iter().map(|a| !a).collect()),
        (UnaryOperator::BitwiseNot, Val::U(x)) => Val::U(x.iter().map(|a| !a).collect()),
        (UnaryOperator::BitwiseNot, Val::I(x)) => Val::I(x.iter().map(|a| !a).collect()),
        (UnaryOperator::BitwiseNot, Val::B(x)) => Val::B(x.iter().map(|a| !a).collect()),
        (op, v) => panic!("unary {op:?} on {v:?}"),
    }
}

macro_rules! lanes {
    ($a:expr, $b:expr, $f:expr) => {
        $a.iter().zip($b.iter()).map(|(x, y)| $f(*x, *y)).collect()
    };
}

/// A matrix value: its columns, all float vectors.
fn columns(v: &Val) -> Option<Vec<Vec<f32>>> {
    match v {
        Val::Arr(cols) if !cols.is_empty() && cols.iter().all(|c| matches!(c, Val::F(_))) => {
            Some(cols.iter().map(|c| c.floats().to_vec()).collect())
        }
        _ => None,
    }
}

fn mat_vec(m: &[Vec<f32>], v: &[f32]) -> Vec<f32> {
    (0..m[0].len())
        .map(|r| m.iter().zip(v).map(|(col, x)| col[r] * x).sum())
        .collect()
}

fn binary(op: Bo, a: Val, b: Val) -> Val {
    if op == Bo::Multiply {
        match (columns(&a), columns(&b)) {
            (Some(m), None) if matches!(b, Val::F(_)) && b.len() == m.len() => {
                return Val::F(mat_vec(&m, b.floats()));
            }
            (Some(m), Some(n)) => {
                return Val::Arr(n.iter().map(|c| Val::F(mat_vec(&m, c))).collect());
            }
            (None, Some(m)) if matches!(a, Val::F(_)) => {
                let v = a.floats();
                return Val::F(
                    m.iter()
                        .map(|c| c.iter().zip(v).map(|(x, y)| x * y).sum())
                        .collect(),
                );
            }
            _ => {}
        }
    }
    let n = a.len().max(b.len());
    let (a, b) = (widen(a, n), widen(b, n));
    match (a, b) {
        (Val::F(x), Val::F(y)) => match op {
            Bo::Add => Val::F(lanes!(x, y, |p: f32, q: f32| p + q)),
            Bo::Subtract => Val::F(lanes!(x, y, |p: f32, q: f32| p - q)),
            Bo::Multiply => Val::F(lanes!(x, y, |p: f32, q: f32| p * q)),
            Bo::Divide => Val::F(lanes!(x, y, |p: f32, q: f32| p / q)),
            Bo::Modulo => Val::F(lanes!(x, y, |p: f32, q: f32| p - q * (p / q).trunc())),
            Bo::Equal => Val::B(lanes!(x, y, |p: f32, q: f32| p == q)),
            Bo::NotEqual => Val::B(lanes!(x, y, |p: f32, q: f32| p != q)),
            Bo::Less => Val::B(lanes!(x, y, |p: f32, q: f32| p < q)),
            Bo::LessEqual => Val::B(lanes!(x, y, |p: f32, q: f32| p <= q)),
            Bo::Greater => Val::B(lanes!(x, y, |p: f32, q: f32| p > q)),
            Bo::GreaterEqual => Val::B(lanes!(x, y, |p: f32, q: f32| p >= q)),
            op => panic!("float {op:?}"),
        },
        (Val::U(x), Val::U(y)) => match op {
            Bo::Add => Val::U(lanes!(x, y, |p: u32, q: u32| p.wrapping_add(q))),
            Bo::Subtract => Val::U(lanes!(x, y, |p: u32, q: u32| p.wrapping_sub(q))),
            Bo::Multiply => Val::U(lanes!(x, y, |p: u32, q: u32| p.wrapping_mul(q))),
            Bo::Divide => Val::U(lanes!(x, y, |p: u32, q: u32| p.checked_div(q).unwrap_or(0))),
            Bo::Modulo => Val::U(lanes!(x, y, |p: u32, q: u32| p.checked_rem(q).unwrap_or(0))),
            Bo::And => Val::U(lanes!(x, y, |p: u32, q: u32| p & q)),
            Bo::InclusiveOr => Val::U(lanes!(x, y, |p: u32, q: u32| p | q)),
            Bo::ExclusiveOr => Val::U(lanes!(x, y, |p: u32, q: u32| p ^ q)),
            Bo::ShiftLeft => Val::U(lanes!(x, y, |p: u32, q: u32| p.wrapping_shl(q))),
            Bo::ShiftRight => Val::U(lanes!(x, y, |p: u32, q: u32| p.wrapping_shr(q))),
            Bo::Equal => Val::B(lanes!(x, y, |p: u32, q: u32| p == q)),
            Bo::NotEqual => Val::B(lanes!(x, y, |p: u32, q: u32| p != q)),
            Bo::Less => Val::B(lanes!(x, y, |p: u32, q: u32| p < q)),
            Bo::LessEqual => Val::B(lanes!(x, y, |p: u32, q: u32| p <= q)),
            Bo::Greater => Val::B(lanes!(x, y, |p: u32, q: u32| p > q)),
            Bo::GreaterEqual => Val::B(lanes!(x, y, |p: u32, q: u32| p >= q)),
            op => panic!("uint {op:?}"),
        },
        (Val::I(x), Val::I(y)) => match op {
            Bo::Add => Val::I(lanes!(x, y, |p: i32, q: i32| p.wrapping_add(q))),
            Bo::Subtract => Val::I(lanes!(x, y, |p: i32, q: i32| p.wrapping_sub(q))),
            Bo::Multiply => Val::I(lanes!(x, y, |p: i32, q: i32| p.wrapping_mul(q))),
            Bo::Divide => Val::I(lanes!(x, y, |p: i32, q: i32| if q == 0 {
                0
            } else {
                p.wrapping_div(q)
            })),
            Bo::Modulo => Val::I(lanes!(x, y, |p: i32, q: i32| if q == 0 {
                0
            } else {
                p.wrapping_rem(q)
            })),
            Bo::And => Val::I(lanes!(x, y, |p: i32, q: i32| p & q)),
            Bo::InclusiveOr => Val::I(lanes!(x, y, |p: i32, q: i32| p | q)),
            Bo::ExclusiveOr => Val::I(lanes!(x, y, |p: i32, q: i32| p ^ q)),
            Bo::ShiftLeft => Val::I(lanes!(x, y, |p: i32, q: i32| p.wrapping_shl(q as u32))),
            Bo::ShiftRight => Val::I(lanes!(x, y, |p: i32, q: i32| p.wrapping_shr(q as u32))),
            Bo::Equal => Val::B(lanes!(x, y, |p: i32, q: i32| p == q)),
            Bo::NotEqual => Val::B(lanes!(x, y, |p: i32, q: i32| p != q)),
            Bo::Less => Val::B(lanes!(x, y, |p: i32, q: i32| p < q)),
            Bo::LessEqual => Val::B(lanes!(x, y, |p: i32, q: i32| p <= q)),
            Bo::Greater => Val::B(lanes!(x, y, |p: i32, q: i32| p > q)),
            Bo::GreaterEqual => Val::B(lanes!(x, y, |p: i32, q: i32| p >= q)),
            op => panic!("sint {op:?}"),
        },
        // A signed value shifted by an unsigned amount (WGSL `i >> 5u`).
        (Val::I(x), Val::U(y)) => match op {
            Bo::ShiftLeft => Val::I(lanes!(x, y, |p: i32, q: u32| p.wrapping_shl(q))),
            Bo::ShiftRight => Val::I(lanes!(x, y, |p: i32, q: u32| p.wrapping_shr(q))),
            op => panic!("sint/uint {op:?}"),
        },
        (Val::B(x), Val::B(y)) => match op {
            Bo::LogicalAnd | Bo::And => Val::B(lanes!(x, y, |p: bool, q: bool| p && q)),
            Bo::LogicalOr | Bo::InclusiveOr => Val::B(lanes!(x, y, |p: bool, q: bool| p || q)),
            Bo::Equal => Val::B(lanes!(x, y, |p: bool, q: bool| p == q)),
            Bo::NotEqual | Bo::ExclusiveOr => Val::B(lanes!(x, y, |p: bool, q: bool| p != q)),
            op => panic!("bool {op:?}"),
        },
        (a, b) => panic!("binary {op:?} on {a:?}, {b:?}"),
    }
}

fn fmap(v: Val, f: impl Fn(f32) -> f32) -> Val {
    match v {
        Val::F(x) => Val::F(x.into_iter().map(f).collect()),
        other => panic!("float function on {other:?}"),
    }
}

fn fmap2(a: Val, b: Val, f: impl Fn(f32, f32) -> f32) -> Val {
    let n = a.len().max(b.len());
    match (widen(a, n), widen(b, n)) {
        (Val::F(x), Val::F(y)) => Val::F(lanes!(x, y, f)),
        (a, b) => panic!("float function on {a:?}, {b:?}"),
    }
}

fn math(fun: Mf, a: Val, b: Option<Val>, c: Option<Val>) -> Val {
    let n = a
        .len()
        .max(b.as_ref().map_or(1, Val::len))
        .max(c.as_ref().map_or(1, Val::len));
    match fun {
        Mf::Abs => match a {
            Val::I(x) => Val::I(x.iter().map(|v| v.wrapping_abs()).collect()),
            v => fmap(v, f32::abs),
        },
        Mf::Min | Mf::Max => {
            let max = fun == Mf::Max;
            match (widen(a, n), widen(b.unwrap(), n)) {
                (Val::F(x), Val::F(y)) => Val::F(lanes!(x, y, |p: f32, q: f32| if max {
                    p.max(q)
                } else {
                    p.min(q)
                })),
                (Val::U(x), Val::U(y)) => Val::U(lanes!(x, y, |p: u32, q: u32| if max {
                    p.max(q)
                } else {
                    p.min(q)
                })),
                (Val::I(x), Val::I(y)) => Val::I(lanes!(x, y, |p: i32, q: i32| if max {
                    p.max(q)
                } else {
                    p.min(q)
                })),
                (a, b) => panic!("min/max on {a:?}, {b:?}"),
            }
        }
        Mf::Clamp => {
            let lo = math(Mf::Max, a, b, None);
            math(Mf::Min, lo, c, None)
        }
        Mf::Saturate => fmap(a, |x| x.clamp(0.0, 1.0)),
        Mf::Floor => fmap(a, f32::floor),
        Mf::Ceil => fmap(a, f32::ceil),
        Mf::Round => fmap(a, f32::round_ties_even),
        Mf::Fract => fmap(a, |x| x - x.floor()),
        Mf::Trunc => fmap(a, f32::trunc),
        Mf::Sqrt => fmap(a, f32::sqrt),
        Mf::InverseSqrt => fmap(a, |x| 1.0 / x.sqrt()),
        Mf::Exp => fmap(a, f32::exp),
        Mf::Exp2 => fmap(a, f32::exp2),
        Mf::Log => fmap(a, f32::ln),
        Mf::Log2 => fmap(a, f32::log2),
        Mf::Sin => fmap(a, f32::sin),
        Mf::Cos => fmap(a, f32::cos),
        Mf::Sign => fmap(a, |x| {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                0.0
            }
        }),
        Mf::Pow => fmap2(a, b.unwrap(), f32::powf),
        Mf::Atan2 => fmap2(a, b.unwrap(), f32::atan2),
        Mf::Step => fmap2(a, b.unwrap(), |edge, x| if x < edge { 0.0 } else { 1.0 }),
        Mf::Dot => {
            let (x, y) = (a, b.unwrap());
            Val::F(vec![
                x.floats().iter().zip(y.floats()).map(|(p, q)| p * q).sum(),
            ])
        }
        Mf::Cross => {
            let (x, y) = (a.floats().to_vec(), b.unwrap().floats().to_vec());
            Val::F(vec![
                x[1] * y[2] - x[2] * y[1],
                x[2] * y[0] - x[0] * y[2],
                x[0] * y[1] - x[1] * y[0],
            ])
        }
        Mf::Length => Val::F(vec![a.floats().iter().map(|x| x * x).sum::<f32>().sqrt()]),
        Mf::Distance => {
            let d = binary(Bo::Subtract, a, b.unwrap());
            math(Mf::Length, d, None, None)
        }
        Mf::Normalize => {
            let l = a.floats().iter().map(|x| x * x).sum::<f32>().sqrt();
            fmap(a, |x| x / l)
        }
        Mf::Mix => {
            let (x, y, t) = (widen(a, n), widen(b.unwrap(), n), widen(c.unwrap(), n));
            let (x, y, t) = (
                x.floats().to_vec(),
                y.floats().to_vec(),
                t.floats().to_vec(),
            );
            Val::F((0..n).map(|i| x[i] * (1.0 - t[i]) + y[i] * t[i]).collect())
        }
        Mf::Fma => {
            let (x, y, z) = (widen(a, n), widen(b.unwrap(), n), widen(c.unwrap(), n));
            let (x, y, z) = (
                x.floats().to_vec(),
                y.floats().to_vec(),
                z.floats().to_vec(),
            );
            Val::F((0..n).map(|i| x[i] * y[i] + z[i]).collect())
        }
        Mf::SmoothStep => {
            let (e0, e1, x) = (widen(a, n), widen(b.unwrap(), n), widen(c.unwrap(), n));
            let (e0, e1, x) = (
                e0.floats().to_vec(),
                e1.floats().to_vec(),
                x.floats().to_vec(),
            );
            Val::F(
                (0..n)
                    .map(|i| {
                        let t = ((x[i] - e0[i]) / (e1[i] - e0[i])).clamp(0.0, 1.0);
                        t * t * (3.0 - 2.0 * t)
                    })
                    .collect(),
            )
        }
        Mf::Unpack2x16float => {
            let w = match a {
                Val::U(x) => x[0],
                other => panic!("unpack2x16float on {other:?}"),
            };
            Val::F(vec![f16_to_f32(w as u16), f16_to_f32((w >> 16) as u16)])
        }
        other => panic!("math function not modelled: {other:?}"),
    }
}

/// IEEE binary16 -> f32.
fn f16_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((h >> 10) & 0x1F) as i32;
    let man = (h & 0x3FF) as f32;
    match exp {
        0 => sign * man * 2f32.powi(-24),
        0x1F if man == 0.0 => sign * f32::INFINITY,
        0x1F => f32::NAN,
        e => sign * (1.0 + man / 1024.0) * 2f32.powi(e - 15),
    }
}

fn cast(v: Val, kind: ScalarKind, convert: bool) -> Val {
    if !convert {
        return match (v, kind) {
            (Val::U(x), ScalarKind::Sint) => Val::I(x.into_iter().map(|a| a as i32).collect()),
            (Val::I(x), ScalarKind::Uint) => Val::U(x.into_iter().map(|a| a as u32).collect()),
            (Val::F(x), ScalarKind::Uint) => Val::U(x.into_iter().map(f32::to_bits).collect()),
            (Val::U(x), ScalarKind::Float) => Val::F(x.into_iter().map(f32::from_bits).collect()),
            (v, k) => panic!("bitcast {v:?} to {k:?}"),
        };
    }
    match (v, kind) {
        (Val::F(x), ScalarKind::Float) => Val::F(x),
        (Val::F(x), ScalarKind::Uint) => Val::U(x.into_iter().map(|a| a as u32).collect()),
        (Val::F(x), ScalarKind::Sint) => Val::I(x.into_iter().map(|a| a as i32).collect()),
        (Val::F(x), ScalarKind::Bool) => Val::B(x.into_iter().map(|a| a != 0.0).collect()),
        (Val::U(x), ScalarKind::Float) => Val::F(x.into_iter().map(|a| a as f32).collect()),
        (Val::U(x), ScalarKind::Uint) => Val::U(x),
        (Val::U(x), ScalarKind::Sint) => Val::I(x.into_iter().map(|a| a as i32).collect()),
        (Val::U(x), ScalarKind::Bool) => Val::B(x.into_iter().map(|a| a != 0).collect()),
        (Val::I(x), ScalarKind::Float) => Val::F(x.into_iter().map(|a| a as f32).collect()),
        (Val::I(x), ScalarKind::Uint) => Val::U(x.into_iter().map(|a| a as u32).collect()),
        (Val::I(x), ScalarKind::Sint) => Val::I(x),
        (Val::I(x), ScalarKind::Bool) => Val::B(x.into_iter().map(|a| a != 0).collect()),
        (Val::B(x), ScalarKind::Float) => {
            Val::F(x.into_iter().map(|a| if a { 1.0 } else { 0.0 }).collect())
        }
        (Val::B(x), ScalarKind::Uint) => Val::U(x.into_iter().map(u32::from).collect()),
        (Val::B(x), ScalarKind::Sint) => Val::I(x.into_iter().map(i32::from).collect()),
        (Val::B(x), ScalarKind::Bool) => Val::B(x),
        (v, k) => panic!("convert {v:?} to {k:?}"),
    }
}

/// The struct-member index lookup an argument builder needs.
pub fn by_name(names: &[String], values: &HashMap<&str, Val>, zero: &[Val]) -> Val {
    Val::Struct(
        names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                values
                    .get(n.as_str())
                    .cloned()
                    .unwrap_or_else(|| zero[i].clone())
            })
            .collect(),
    )
}
