#![feature(iter_intersperse)]

use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    str::FromStr,
};

use fxhash::FxHashMap;
use gputter_core::{get_device, init_gputter_blocking, wgpu, GpuDevice};
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use regex::Regex;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, token, Ident, LitStr, Visibility,
};
use unfmt::unformat;

enum StorageType {
    Uniform,
    Storage {
        host_rw_mode: &'static str,
        shader_read_only: bool,
    },
}

struct BuildShader {
    vis: Visibility,
    name: Ident,
    _comma: token::Comma,
    shader: LitStr,
}

impl Parse for BuildShader {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(BuildShader {
            vis: input.parse()?,
            name: input.parse()?,
            _comma: input.parse()?,
            shader: input.parse()?,
        })
    }
}

fn type_resolver(s: &str, uint_consts: &FxHashMap<&str, Option<u32>>) -> String {
    match s {
        "f32" => return "f32".into(),
        "u32" => return "u32".into(),
        "i32" => return "i32".into(),
        "vec2f" => return "gputter::types::AlignedVec2<f32>".into(),
        "vec3f" => return "gputter::types::AlignedVec3<f32>".into(),
        "vec4f" => return "gputter::types::AlignedVec4<f32>".into(),
        "vec2u" => return "gputter::types::AlignedVec2<u32>".into(),
        "vec3u" => return "gputter::types::AlignedVec3<u32>".into(),
        "vec4u" => return "gputter::types::AlignedVec4<u32>".into(),
        "vec2i" => return "gputter::types::AlignedVec2<i32>".into(),
        "vec3i" => return "gputter::types::AlignedVec3<i32>".into(),
        "vec4i" => return "gputter::types::AlignedVec4<i32>".into(),
        _ => {}
    }
    
    if let Some(inner) = unformat!("array<{}>", s) {
        if let Some((ty, mut count)) = unformat!("{},{}", inner) {
            count = count.trim();
            if let Ok(count) = usize::from_str(count) {
                // count is a literal integer
                return format!("[{}; {}]", type_resolver(ty.trim(), uint_consts), count);
            } else if let Some(&count) = uint_consts.get(count) {
                if let Some(count) = count {
                    // count is a constant value
                    return format!("[{}; {}]", type_resolver(ty.trim(), uint_consts), count);
                } else {
                    // If count is None, it is a build time constant that is not known at compile time
                    // of the host code
                    return format!("[{}]", type_resolver(ty.trim(), uint_consts));
                }
            } else {
                panic!("Invalid array count: {count} in {s} {uint_consts:?}");
            }
        }
        // There is no count
        return format!("[{}]", type_resolver(inner.trim(), uint_consts));
    }
    
    // It is some custom type that is used verbatim in the shader and the host
    s.into()
}

#[proc_macro]
pub fn build_shader(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let BuildShader {
        vis, name, shader, ..
    } = parse_macro_input!(input as BuildShader);

    let shader = {
        let mut tmp = String::with_capacity(shader.value().len() + 1);
        tmp.push_str("alias NonZeroU32 = u32;\nalias NonZeroI32 = i32;\n");
        tmp.push_str(&shader.value());
        tmp
    };

    let re = Regex::new(r"@compute[\s@a-zA-Z0-9\(\)_,]+fn\s+([a-zA-Z0-9]+)\s*\(").unwrap();
    let compute_fns: Vec<_> = re.captures_iter(&shader).map(|caps| {
        let (_, [fn_name]) = caps.extract();
        fn_name
    }).collect();
    
    let re = Regex::new(
            r"const\s*([a-zA-Z0-9]+)\s*:\s*([a-zA-Z0-9]+)\s*=\s*([\{\}a-zA-Z0-9]+)\s*;?",
        )
        .unwrap();

    let uint_consts: FxHashMap<_, _> = re.captures_iter(&shader).filter_map(|caps| {
        let (_, [const_name, const_ty, const_val]) = caps.extract();
        if const_ty != "u32" && const_ty != "NonZeroU32" {
            return None;
        }

        if const_val.starts_with("{{") && const_val.ends_with("}}") {
            return Some((const_name, None));
        }

        let Ok(n) = u32::from_str(&const_val) else {
            panic!(r#"Constant "{const_name}" is not a valid u32 (was {const_val})"#);
        };
        Some((const_name, Some(n)))
    })
    .collect();

    let re = Regex::new(r"#\[buffer\(([a-zA-Z0-9]+)\)\]").unwrap();

    let buffer_rw_modes: Vec<_> = re.captures_iter(&shader).map(|caps| {
        let (_, [rw_mode]) = caps.extract();
        match rw_mode {
            "HostHidden" => "HostHidden",
            "HostReadOnly" => "HostReadOnly",
            "HostWriteOnly" => "HostWriteOnly",
            "HostReadWrite" => "HostReadWrite",
            _ => panic!("Unsupported buffer host read-write mode: {rw_mode}"),
        }
    }).collect();

    let mut buffer_storage_types = vec![];
    let mut buffer_types = vec![];
    let splitted: Vec<_> = re.split(&shader).collect();

    let buffer_names: Vec<_> = splitted
        .iter()
        .skip(1)
        .map(|&s| {
            let (_, storage_ty, name, ty) = unformat!("{}var<{}>{}:{};", s)
                .ok_or_else(|| {
                    panic!(
                        "Format of buffer is incorrect: {}",
                        s.split('\n').next().unwrap_or_default()
                    )
                })
                .unwrap();

            buffer_types.push(type_resolver(ty.trim(), &uint_consts));
            let host_rw_mode = buffer_rw_modes[buffer_storage_types.len()];
            buffer_storage_types.push(if storage_ty.trim() == "uniform" {
                if host_rw_mode != "HostWriteOnly" {
                    panic!("Uniform buffer must be HostWriteOnly");
                }
                StorageType::Uniform
            } else if let Some((_, shader_rw_mode)) = unformat!("{}storage,{}", storage_ty) {
                match shader_rw_mode.trim() {
                    "read_write" => StorageType::Storage {
                        host_rw_mode,
                        shader_read_only: false,
                    },
                    "read" => StorageType::Storage {
                        host_rw_mode,
                        shader_read_only: true,
                    },
                    _ => panic!("Unsupported shader read-write mode: {shader_rw_mode}"),
                }
            } else {
                panic!("Unsupported buffer storage type: {storage_ty}")
            });
            name.trim()
        })
        .collect();

    let re = Regex::new(
        r"(const\s+[a-zA-Z0-9]+\s*:\s*([a-zA-Z0-9]+)\s*=\s*)\{\{([a-zA-Z0-9]+)\}\}\s*(;?)",
    )
    .unwrap();

    let mut const_types = vec![];
    let mut const_names = vec![];
    let mut binding_index = 0usize;

    let shader: Vec<_> = splitted
        .into_iter()
        .map(String::from)
        .intersperse_with(|| {
            let out = format!("<<GRP_SUBSTITUTE{binding_index}>>");
            binding_index += 1;
            out
        })
        .flat_map(|s| {
            re.captures_iter(&s).for_each(|caps| {
                let (_, [_, type_name, const_name, _]) = caps.extract();
                const_types.push(type_name.to_owned());
                const_names.push(const_name.to_owned());
            });
            let mut const_index = 0usize;
            let splitted: Vec<_> = re
                .replace_all(&s, "$1<<SUBSTITUTE>>$4")
                .split("<<SUBSTITUTE>>")
                .map(String::from)
                .intersperse_with(|| {
                    let out = format!("<<SUBSTITUTE{}>>", const_types[const_index]);
                    const_index += 1;
                    out
                })
                .collect();
            splitted
        })
        .collect();

    let tmp_shader: String = shader
        .iter()
        .map(|s| {
            if let Some(i) = unformat!("<<GRP_SUBSTITUTE{}>>", s) {
                format!("@group(0) @binding({i})")
            } else if let Some(ty) = unformat!("<<SUBSTITUTE{}>>", s) {
                match ty {
                    "f32" => "0.0".to_owned(),
                    "u32" => "0".to_owned(),
                    "i32" => "0".to_owned(),
                    "bool" => "false".to_owned(),
                    "NonZeroU32" => "1".to_owned(),
                    "NonZeroI32" => "1".to_owned(),
                    _ => panic!("Unsupported type for substitution: {}", ty),
                }
            } else {
                s.clone()
            }
        })
        .collect();

    // Check that it compiles
    init_gputter_blocking()
        .expect("Failed to initialize gputter");
    let GpuDevice { device, .. } = get_device();
    if let Err(panic) = catch_unwind(AssertUnwindSafe(|| {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl((&tmp_shader).into()),
        });
    })) {
        let payload: Box<String> = panic.downcast().unwrap();
        panic!(
            "Failed to compile the following mock shader:\n\n{}\n\n{}",
            tmp_shader, payload
        );
    }

    let mut binding_index = 0usize;
    let mut const_index = 0usize;

    let shader: String = shader
        .iter()
        .map(|s| {
            if let Some(_) = unformat!("<<GRP_SUBSTITUTE{}>>", s) {
                let out = format!("{{{}}}", buffer_names[binding_index]);
                binding_index += 1;
                out
            } else if let Some(_) = unformat!("<<SUBSTITUTE{}>>", s) {
                let out = format!("{{{}}}", const_names[const_index]);
                const_index += 1;
                out
            } else {
                s.replace('{', "{{").replace('}', "}}")
            }
        })
        .collect();

    let buffer_def: Vec<_> = buffer_names
        .iter()
        .zip(buffer_types.iter())
        .zip(buffer_storage_types.iter())
        .map(|((&name, ty), storage_ty)| {
            let stream = match storage_ty {
                StorageType::Uniform => format!("{name}: gputter::shader::BufferGroupBinding<gputter::buffers::uniform::UniformBuffer<{ty}>, S>"),
                StorageType::Storage { host_rw_mode, shader_read_only } => {
                    let shader_read_only = if *shader_read_only { "ShaderReadOnly" } else { "ShaderReadWrite" };
                    format!("{name}: gputter::shader::BufferGroupBinding<gputter::buffers::storage::StorageBuffer<{ty}, gputter::buffers::storage::{host_rw_mode}, gputter::buffers::storage::{shader_read_only}>, S>")
                }
            };
            proc_macro2::TokenStream::from_str(&stream).unwrap()
        })
        .collect();

    let const_def = const_names
        .iter()
        .zip(const_types.iter())
        .map(|(name, ty)| proc_macro2::TokenStream::from_str(&format!("{name}: {ty}")).unwrap());

    let const_idents = const_names.iter().map(|name| format_ident!("{name}"));
    let buffer_idents = buffer_names.iter().map(|&name| format_ident!("{name}"));

    let compute_count = compute_fns.len();
    let compile_out = compute_fns.iter()
        .map(|&name| {
            proc_macro2::TokenStream::from_str(&format!("gputter::shader::ComputeFn::new_unchecked(shader.clone(), {name:?})")).unwrap()
        });
    
    // Build the output, possibly using quasi-quotation
    let expanded = quote! {
        #vis struct #name<S> {
            #(#buffer_def,)*
            #(#const_def,)*
        }
        impl<S> #name<S> {
            #vis fn compile(&self) -> [gputter::shader::ComputeFn<S>; #compute_count] {
                #(let #buffer_idents = &self.#buffer_idents;)*
                #(let #const_idents = &self.#const_idents;)*
                let shader = format!(#shader);
                let shader = gputter::get_device().device.create_shader_module(gputter::wgpu::ShaderModuleDescriptor {
                    label: None,
                    source: gputter::wgpu::ShaderSource::Wgsl(shader.into()),
                });
                let shader = std::sync::Arc::new(shader);
                [
                    #(#compile_out,)*
                ]
            }
        }
    };

    // Hand the output tokens back to the compiler
    TokenStream::from(expanded)
}
