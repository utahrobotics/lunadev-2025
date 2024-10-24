#![feature(iter_intersperse)]

use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    str::FromStr,
};

use gputter_core::{get_device, init_gputter, wgpu, GpuDevice};
use pollster::FutureExt;
use proc_macro::TokenStream;
use proc_macro2::Literal;
use quote::{format_ident, quote};
use regex::Regex;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, token, DeriveInput, Ident, LitStr, Visibility,
};
use unfmt::unformat;

struct BuildShader {
    vis: Visibility,
    name: Ident,
    comma: token::Comma,
    shader: LitStr,
}

impl Parse for BuildShader {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(BuildShader {
            vis: input.parse()?,
            name: input.parse()?,
            comma: input.parse()?,
            shader: input.parse()?,
        })
    }
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

    let mut buffer_types = vec![];
    let splitted: Vec<_> = shader.split("#[buffer]").collect();
    // panic!("{splitted:?}");

    let buffer_names: Vec<_> = splitted
        .iter()
        .skip(1)
        .map(|&s| {
            let (_, _, name, mut ty) = unformat!("{}var<{}>{}:{};", s)
                .ok_or_else(|| {
                    panic!(
                        "Format of buffer is incorrect: {}",
                        s.split('\n').next().unwrap_or_default()
                    )
                })
                .unwrap();

            ty = ty.trim();
            buffer_types.push(
                match ty {
                    "f32" => "f32",
                    "u32" => "u32",
                    "i32" => "i32",
                    _ => panic!("Unsupported buffer type: {ty}"),
                }
            );
            name.trim()
        })
        .collect();

    let re = Regex::new(
        r"(const\s*[a-zA-Z0-9]+\s*:\s*([a-zA-Z0-9]+)\s*=\s*)\{\{([a-zA-Z0-9]+)\}\}\s*(;?)",
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
    init_gputter()
        .block_on()
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
        .map(|(&name, &ty)| {
            proc_macro2::TokenStream::from_str(&format!("{name}: gputter::shader::BufferGroupBinding<{ty}, S>")).unwrap()
        })
        .collect();

    let const_def = const_names
        .iter()
        .zip(const_types.iter())
        .map(|(name, ty)| proc_macro2::TokenStream::from_str(&format!("{name}: {ty}")).unwrap());

    let const_idents = const_names.iter().map(|name| format_ident!("{name}"));
    let buffer_idents = buffer_names.iter().map(|&name| format_ident!("{name}"));

    // Build the output, possibly using quasi-quotation
    let expanded = quote! {
        #vis struct #name<S> {
            #(#buffer_def,)*
            #(#const_def,)*
        }
        impl<S> #name<S> {
            #vis fn compile(&self) -> gputter::shader::CompiledShader<S> {
                #(let #buffer_idents = &self.#buffer_idents;)*
                #(let #const_idents = &self.#const_idents;)*
                let shader = format!(#shader);
                gputter::get_device().device.create_shader_module(gputter::wgpu::ShaderModuleDescriptor {
                    label: None,
                    source: gputter::wgpu::ShaderSource::Wgsl(shader.into()),
                }).into()
            }
        }
    };

    // Hand the output tokens back to the compiler
    TokenStream::from(expanded)
}
