//! Derive macro for the `Generable` trait.
//!
//! This crate provides a proc-macro derive for generating JSON Schema
//! from Rust types for use with structured generation in fm-rs.

//! Proc-macro derive for fm-rs Generable schema generation.

#![allow(clippy::pedantic)]

use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use syn::{
    Attribute, Data, DataEnum, DataStruct, DeriveInput, Fields, Lit, LitBool, LitStr, Meta, Path,
    Type, parse_macro_input, spanned::Spanned,
};

/// Derives the `Generable` trait for a struct or enum.
///
/// This generates a JSON Schema that describes the type's structure,
/// which can be used for structured generation with FoundationModels.
///
/// # Attributes
///
/// ## Container attributes (`#[generable(...)]` or `#[serde(...)]`)
/// - `rename_all = "..."` - Rename all fields/variants (camelCase, snake_case, etc.)
/// - `description = "..."` - Add a description to the schema
///
/// ## Field/variant attributes (`#[generable(...)]` or `#[serde(...)]`)
/// - `rename = "..."` - Rename this field/variant
/// - `skip` - Skip this field/variant in the schema
/// - `description = "..."` - Add a description
/// - `minimum = N` / `maximum = N` - Numeric bounds
/// - `min_length = N` / `max_length = N` - String length bounds
/// - `pattern = "..."` - Regex pattern for strings
/// - `min_items = N` / `max_items = N` - Array length bounds
///
/// # Example
///
/// ```ignore
/// use fm_rs::Generable;
///
/// #[derive(Generable)]
/// #[generable(rename_all = "camelCase")]
/// struct Person {
///     #[generable(description = "The person's full name")]
///     full_name: String,
///     #[generable(minimum = 0, maximum = 150)]
///     age: u32,
/// }
/// ```
/// Derives `Generable` for structs and unit enums.
#[proc_macro_derive(Generable, attributes(generable, serde))]
pub fn derive_generable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match derive_generable_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

struct ContainerAttrs {
    crate_path: Path,
    rename_all: Option<String>,
    description: Option<String>,
    example: Option<Lit>,
}

impl Default for ContainerAttrs {
    fn default() -> Self {
        let crate_path = match syn::parse_str::<Path>("::fm_rs") {
            Ok(path) => path,
            Err(_) => Path::from(syn::Ident::new("fm_rs", proc_macro2::Span::call_site())),
        };
        Self {
            crate_path,
            rename_all: None,
            description: None,
            example: None,
        }
    }
}

#[derive(Default)]
struct FieldAttrs {
    rename: Option<String>,
    skip: bool,
    default: bool,
    description: Option<String>,
    example: Option<Lit>,
    minimum: Option<Lit>,
    maximum: Option<Lit>,
    min_length: Option<Lit>,
    max_length: Option<Lit>,
    pattern: Option<String>,
    min_items: Option<Lit>,
    max_items: Option<Lit>,
    nullable: bool,
}

struct SchemaInfo {
    expr: proc_macro2::TokenStream,
    optional: bool,
}

fn derive_generable_impl(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let container_attrs = parse_container_attrs(&input.attrs)?;
    let crate_path = &container_attrs.crate_path;
    let ident = &input.ident;
    let schema_body = match &input.data {
        Data::Struct(data) => schema_for_struct(data, &container_attrs)?,
        Data::Enum(data) => schema_for_enum(data, &container_attrs)?,
        Data::Union(_) => {
            return Err(syn::Error::new(
                input.span(),
                "Generable does not support unions",
            ));
        }
    };

    let tokens = quote! {
        impl #crate_path::Generable for #ident {
            fn schema() -> #crate_path::serde_json::Value {
                #schema_body
            }
        }
    };

    Ok(tokens)
}

fn parse_container_attrs(attrs: &[Attribute]) -> syn::Result<ContainerAttrs> {
    let mut out = ContainerAttrs::default();

    for attr in attrs {
        if attr.path().is_ident("generable") {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("crate") {
                    let value = meta.value()?.parse::<LitStr>()?;
                    out.crate_path = syn::parse_str(&value.value())
                        .map_err(|_| syn::Error::new(value.span(), "invalid crate path"))?;
                    return Ok(());
                }
                if meta.path.is_ident("rename_all") {
                    let value = meta.value()?.parse::<LitStr>()?;
                    out.rename_all = Some(value.value());
                    return Ok(());
                }
                if meta.path.is_ident("description") {
                    let value = meta.value()?.parse::<LitStr>()?;
                    out.description = Some(value.value());
                    return Ok(());
                }
                if meta.path.is_ident("example") {
                    let value = meta.value()?.parse::<Lit>()?;
                    out.example = Some(value);
                    return Ok(());
                }
                Err(meta.error("unsupported generable attribute"))
            })?;
        }

        if attr.path().is_ident("serde") {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename_all") {
                    let value = meta.value()?.parse::<LitStr>()?;
                    out.rename_all = Some(value.value());
                    return Ok(());
                }
                Ok(())
            })?;
        }
    }

    if out.description.is_none() {
        out.description = doc_comment(attrs);
    }

    Ok(out)
}

fn parse_field_attrs(attrs: &[Attribute]) -> syn::Result<FieldAttrs> {
    let mut out = FieldAttrs::default();

    for attr in attrs {
        if attr.path().is_ident("generable") {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    let value = meta.value()?.parse::<LitStr>()?;
                    out.rename = Some(value.value());
                    return Ok(());
                }
                if meta.path.is_ident("skip") {
                    out.skip = true;
                    return Ok(());
                }
                if meta.path.is_ident("description") {
                    let value = meta.value()?.parse::<LitStr>()?;
                    out.description = Some(value.value());
                    return Ok(());
                }
                if meta.path.is_ident("example") {
                    let value = meta.value()?.parse::<Lit>()?;
                    out.example = Some(value);
                    return Ok(());
                }
                if meta.path.is_ident("minimum") {
                    let value = meta.value()?.parse::<Lit>()?;
                    out.minimum = Some(value);
                    return Ok(());
                }
                if meta.path.is_ident("maximum") {
                    let value = meta.value()?.parse::<Lit>()?;
                    out.maximum = Some(value);
                    return Ok(());
                }
                if meta.path.is_ident("min_length") {
                    let value = meta.value()?.parse::<Lit>()?;
                    out.min_length = Some(value);
                    return Ok(());
                }
                if meta.path.is_ident("max_length") {
                    let value = meta.value()?.parse::<Lit>()?;
                    out.max_length = Some(value);
                    return Ok(());
                }
                if meta.path.is_ident("pattern") {
                    let value = meta.value()?.parse::<LitStr>()?;
                    out.pattern = Some(value.value());
                    return Ok(());
                }
                if meta.path.is_ident("min_items") {
                    let value = meta.value()?.parse::<Lit>()?;
                    out.min_items = Some(value);
                    return Ok(());
                }
                if meta.path.is_ident("max_items") {
                    let value = meta.value()?.parse::<Lit>()?;
                    out.max_items = Some(value);
                    return Ok(());
                }
                if meta.path.is_ident("nullable") {
                    let value = meta.value()?.parse::<LitBool>()?;
                    out.nullable = value.value;
                    return Ok(());
                }
                Err(meta.error("unsupported generable attribute"))
            })?;
        }

        if attr.path().is_ident("serde") {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    let value = meta.value()?.parse::<LitStr>()?;
                    out.rename = Some(value.value());
                    return Ok(());
                }
                if meta.path.is_ident("skip") || meta.path.is_ident("skip_serializing") {
                    out.skip = true;
                    return Ok(());
                }
                if meta.path.is_ident("default") {
                    out.default = true;
                    return Ok(());
                }
                Ok(())
            })?;
        }
    }

    if out.description.is_none() {
        out.description = doc_comment(attrs);
    }

    Ok(out)
}

fn schema_for_struct(
    data: &DataStruct,
    container: &ContainerAttrs,
) -> syn::Result<proc_macro2::TokenStream> {
    let crate_path = &container.crate_path;
    let mut property_inserts = Vec::new();
    let mut required_fields = Vec::new();
    let mut container_inserts = Vec::new();

    match &data.fields {
        Fields::Named(fields) => {
            for field in &fields.named {
                let field_attrs = parse_field_attrs(&field.attrs)?;
                if field_attrs.skip {
                    continue;
                }
                let ident = field
                    .ident
                    .as_ref()
                    .ok_or_else(|| syn::Error::new(field.span(), "expected named field"))?;
                let name = field_name(ident.to_string(), &field_attrs, container);
                let SchemaInfo {
                    expr: schema_expr,
                    optional,
                } = schema_for_type(&field.ty, &field_attrs, crate_path)?;
                property_inserts.push(quote! {
                    properties.insert(#name.to_string(), #schema_expr);
                });
                if !optional && !field_attrs.default {
                    required_fields.push(name);
                }
            }
        }
        Fields::Unnamed(fields) => {
            if fields.unnamed.len() == 1 {
                let field = fields
                    .unnamed
                    .first()
                    .ok_or_else(|| syn::Error::new(fields.span(), "expected field"))?;
                let field_attrs = parse_field_attrs(&field.attrs)?;
                let schema_info = schema_for_type(&field.ty, &field_attrs, crate_path)?;
                return Ok(schema_info.expr);
            }
            return Err(syn::Error::new(
                fields.span(),
                "Generable only supports named fields or newtype structs",
            ));
        }
        Fields::Unit => {
            return Err(syn::Error::new(
                data.fields.span(),
                "Generable does not support unit structs",
            ));
        }
    }

    let required_values = required_fields
        .iter()
        .map(|name| quote!(#crate_path::serde_json::Value::String(#name.to_string())));

    if let Some(desc) = &container.description {
        let desc_lit = LitStr::new(desc, proc_macro2::Span::call_site());
        container_inserts.push(quote! {
            schema.insert(
                "description".to_string(),
                #crate_path::serde_json::Value::String(#desc_lit.to_string())
            );
        });
    }
    if let Some(example) = &container.example {
        let example_tokens = example.to_token_stream();
        container_inserts.push(quote! {
            schema.insert(
                "example".to_string(),
                #crate_path::serde_json::json!(#example_tokens)
            );
        });
    }

    Ok(quote! {
        let mut schema = #crate_path::serde_json::Map::new();
        schema.insert(
            "type".to_string(),
            #crate_path::serde_json::Value::String("object".to_string())
        );
        let mut properties = #crate_path::serde_json::Map::new();
        #(#property_inserts)*
        schema.insert(
            "properties".to_string(),
            #crate_path::serde_json::Value::Object(properties)
        );
        let mut required = Vec::new();
        #(required.push(#required_values);)*
        if !required.is_empty() {
            schema.insert("required".to_string(), #crate_path::serde_json::Value::Array(required));
        }
        #(#container_inserts)*
        #crate_path::serde_json::Value::Object(schema)
    })
}

fn schema_for_enum(
    data: &DataEnum,
    container: &ContainerAttrs,
) -> syn::Result<proc_macro2::TokenStream> {
    let crate_path = &container.crate_path;
    let mut variants = Vec::new();
    let mut container_inserts = Vec::new();

    for variant in &data.variants {
        match &variant.fields {
            Fields::Unit => {}
            _ => {
                return Err(syn::Error::new(
                    variant.span(),
                    "Generable only supports unit enums (string-only)",
                ));
            }
        }

        let variant_attrs = parse_field_attrs(&variant.attrs)?;
        if variant_attrs.skip {
            continue;
        }
        let variant_name = apply_rename(variant.ident.to_string(), &variant_attrs, container);
        variants.push(variant_name);
    }

    let variant_values = variants
        .iter()
        .map(|name| quote!(#crate_path::serde_json::Value::String(#name.to_string())));

    if let Some(desc) = &container.description {
        let desc_lit = LitStr::new(desc, proc_macro2::Span::call_site());
        container_inserts.push(quote! {
            schema.insert(
                "description".to_string(),
                #crate_path::serde_json::Value::String(#desc_lit.to_string())
            );
        });
    }
    if let Some(example) = &container.example {
        let example_tokens = example.to_token_stream();
        container_inserts.push(quote! {
            schema.insert(
                "example".to_string(),
                #crate_path::serde_json::json!(#example_tokens)
            );
        });
    }

    Ok(quote! {
        let mut schema = #crate_path::serde_json::Map::new();
        let variants = vec![#(#variant_values),*];
        schema.insert(
            "type".to_string(),
            #crate_path::serde_json::Value::String("string".to_string())
        );
        schema.insert("enum".to_string(), #crate_path::serde_json::Value::Array(variants));
        #(#container_inserts)*
        #crate_path::serde_json::Value::Object(schema)
    })
}

fn schema_for_type(ty: &Type, attrs: &FieldAttrs, crate_path: &Path) -> syn::Result<SchemaInfo> {
    if let Some(inner) = option_inner(ty) {
        let mut schema = schema_for_type(inner, attrs, crate_path)?;
        schema.optional = true;
        return Ok(schema);
    }

    if let Some(inner) = vec_inner(ty) {
        let inner_schema = schema_for_type(inner, &FieldAttrs::default(), crate_path)?.expr;
        let mut entries = vec![schema_type_entry(crate_path, "array")];
        entries.push(schema_entry("items", quote!(#inner_schema)));
        add_attr_entries(crate_path, attrs, &mut entries);
        return Ok(SchemaInfo {
            expr: schema_object_expr(crate_path, entries),
            optional: false,
        });
    }

    if let Some((key_ty, value_ty)) = map_inner(ty) {
        if !is_string_type(key_ty) {
            return Err(syn::Error::new(
                key_ty.span(),
                "Generable only supports map keys of type String",
            ));
        }
        let value_schema = schema_for_type(value_ty, &FieldAttrs::default(), crate_path)?.expr;
        let mut entries = vec![schema_type_entry(crate_path, "object")];
        entries.push(schema_entry("additionalProperties", quote!(#value_schema)));
        add_attr_entries(crate_path, attrs, &mut entries);
        return Ok(SchemaInfo {
            expr: schema_object_expr(crate_path, entries),
            optional: false,
        });
    }

    if is_string_type(ty) {
        let mut entries = vec![schema_type_entry(crate_path, "string")];
        add_attr_entries(crate_path, attrs, &mut entries);
        return Ok(SchemaInfo {
            expr: schema_object_expr(crate_path, entries),
            optional: false,
        });
    }

    if is_bool_type(ty) {
        let mut entries = vec![schema_type_entry(crate_path, "boolean")];
        add_attr_entries(crate_path, attrs, &mut entries);
        return Ok(SchemaInfo {
            expr: schema_object_expr(crate_path, entries),
            optional: false,
        });
    }

    if is_integer_type(ty) {
        let mut entries = vec![schema_type_entry(crate_path, "integer")];
        add_attr_entries(crate_path, attrs, &mut entries);
        return Ok(SchemaInfo {
            expr: schema_object_expr(crate_path, entries),
            optional: false,
        });
    }

    if is_number_type(ty) {
        let mut entries = vec![schema_type_entry(crate_path, "number")];
        add_attr_entries(crate_path, attrs, &mut entries);
        return Ok(SchemaInfo {
            expr: schema_object_expr(crate_path, entries),
            optional: false,
        });
    }

    if is_serde_json_value(ty) {
        let mut entries = Vec::new();
        add_attr_entries(crate_path, attrs, &mut entries);
        return Ok(SchemaInfo {
            expr: schema_object_expr(crate_path, entries),
            optional: false,
        });
    }

    let mut entries = Vec::new();
    add_attr_entries(crate_path, attrs, &mut entries);
    let schema = if entries.is_empty() {
        quote!(<#ty as #crate_path::Generable>::schema())
    } else {
        let base = quote!(<#ty as #crate_path::Generable>::schema());
        quote!({
            let mut schema = #base;
            if let #crate_path::serde_json::Value::Object(ref mut map) = schema {
                #(#entries)*
            }
            schema
        })
    };

    Ok(SchemaInfo {
        expr: schema,
        optional: false,
    })
}

fn schema_object_expr(
    crate_path: &Path,
    entries: Vec<proc_macro2::TokenStream>,
) -> proc_macro2::TokenStream {
    quote!({
        let mut map = #crate_path::serde_json::Map::new();
        #(#entries)*
        #crate_path::serde_json::Value::Object(map)
    })
}

fn schema_type_entry(crate_path: &Path, ty: &str) -> proc_macro2::TokenStream {
    let lit = LitStr::new(ty, proc_macro2::Span::call_site());
    schema_entry(
        "type",
        quote!(#crate_path::serde_json::Value::String(#lit.to_string())),
    )
}

fn schema_entry(key: &str, value_expr: proc_macro2::TokenStream) -> proc_macro2::TokenStream {
    let lit = LitStr::new(key, proc_macro2::Span::call_site());
    quote!(map.insert(#lit.to_string(), #value_expr);)
}

fn schema_string_insert(crate_path: &Path, key: &str, value: &str) -> proc_macro2::TokenStream {
    let key_lit = LitStr::new(key, proc_macro2::Span::call_site());
    let val_lit = LitStr::new(value, proc_macro2::Span::call_site());
    quote!(map.insert(#key_lit.to_string(), #crate_path::serde_json::Value::String(#val_lit.to_string()));)
}

fn schema_value_insert(crate_path: &Path, key: &str, value: &Lit) -> proc_macro2::TokenStream {
    let key_lit = LitStr::new(key, proc_macro2::Span::call_site());
    let value_tokens = value.to_token_stream();
    quote!(map.insert(#key_lit.to_string(), #crate_path::serde_json::json!(#value_tokens));)
}

fn add_attr_entries(
    crate_path: &Path,
    attrs: &FieldAttrs,
    entries: &mut Vec<proc_macro2::TokenStream>,
) {
    if let Some(desc) = &attrs.description {
        entries.push(schema_string_insert(crate_path, "description", desc));
    }
    if let Some(example) = &attrs.example {
        entries.push(schema_value_insert(crate_path, "example", example));
    }
    if let Some(minimum) = &attrs.minimum {
        entries.push(schema_value_insert(crate_path, "minimum", minimum));
    }
    if let Some(maximum) = &attrs.maximum {
        entries.push(schema_value_insert(crate_path, "maximum", maximum));
    }
    if let Some(min_length) = &attrs.min_length {
        entries.push(schema_value_insert(crate_path, "minLength", min_length));
    }
    if let Some(max_length) = &attrs.max_length {
        entries.push(schema_value_insert(crate_path, "maxLength", max_length));
    }
    if let Some(pattern) = &attrs.pattern {
        entries.push(schema_string_insert(crate_path, "pattern", pattern));
    }
    if let Some(min_items) = &attrs.min_items {
        entries.push(schema_value_insert(crate_path, "minItems", min_items));
    }
    if let Some(max_items) = &attrs.max_items {
        entries.push(schema_value_insert(crate_path, "maxItems", max_items));
    }
    if attrs.nullable {
        entries.push(schema_entry(
            "nullable",
            quote!(#crate_path::serde_json::Value::Bool(true)),
        ));
    }
}

fn field_name(raw: String, attrs: &FieldAttrs, container: &ContainerAttrs) -> String {
    if let Some(rename) = &attrs.rename {
        return rename.clone();
    }
    if let Some(rule) = &container.rename_all {
        apply_rename_all(&raw, rule)
    } else {
        raw
    }
}

fn apply_rename(raw: String, attrs: &FieldAttrs, container: &ContainerAttrs) -> String {
    if let Some(rename) = &attrs.rename {
        return rename.clone();
    }
    if let Some(rule) = &container.rename_all {
        apply_rename_all(&raw, rule)
    } else {
        raw
    }
}

fn apply_rename_all(name: &str, rule: &str) -> String {
    let words = split_words(name);
    match rule {
        "lowercase" => join_words(&words, "", |w| w.to_ascii_lowercase()),
        "UPPERCASE" => join_words(&words, "", |w| w.to_ascii_uppercase()),
        "snake_case" => join_words(&words, "_", |w| w.to_ascii_lowercase()),
        "SCREAMING_SNAKE_CASE" => join_words(&words, "_", |w| w.to_ascii_uppercase()),
        "kebab-case" => join_words(&words, "-", |w| w.to_ascii_lowercase()),
        "PascalCase" => join_pascal(&words),
        "camelCase" => join_camel(&words),
        _ => name.to_string(),
    }
}

fn split_words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = name.chars().peekable();
    let mut prev_is_upper = false;
    let mut prev_is_lower = false;

    while let Some(ch) = chars.next() {
        if ch == '_' || ch == '-' {
            if !current.is_empty() {
                words.push(current);
                current = String::new();
            }
            prev_is_upper = false;
            prev_is_lower = false;
            continue;
        }

        let is_upper = ch.is_ascii_uppercase();
        let is_lower = ch.is_ascii_lowercase();
        let next_is_lower = chars
            .peek()
            .map(|next| next.is_ascii_lowercase())
            .unwrap_or(false);

        if !current.is_empty()
            && ((prev_is_lower && is_upper) || (prev_is_upper && is_upper && next_is_lower))
        {
            words.push(current);
            current = String::new();
        }

        current.push(ch);
        prev_is_upper = is_upper;
        prev_is_lower = is_lower;
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
}

fn join_words<F>(words: &[String], sep: &str, mut transform: F) -> String
where
    F: FnMut(&str) -> String,
{
    let mut out = String::new();
    for (idx, word) in words.iter().enumerate() {
        if idx > 0 {
            out.push_str(sep);
        }
        out.push_str(&transform(word));
    }
    out
}

fn join_pascal(words: &[String]) -> String {
    let mut out = String::new();
    for word in words {
        if word.is_empty() {
            continue;
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.push_str(&chars.as_str().to_ascii_lowercase());
        }
    }
    out
}

fn join_camel(words: &[String]) -> String {
    let mut out = String::new();
    for (idx, word) in words.iter().enumerate() {
        if word.is_empty() {
            continue;
        }
        if idx == 0 {
            out.push_str(&word.to_ascii_lowercase());
        } else {
            out.push_str(&join_pascal(std::slice::from_ref(word)));
        }
    }
    out
}

fn option_inner(ty: &Type) -> Option<&Type> {
    if let Type::Path(type_path) = ty {
        let segment = type_path.path.segments.last()?;
        if segment.ident == "Option"
            && let syn::PathArguments::AngleBracketed(args) = &segment.arguments
            && let Some(syn::GenericArgument::Type(inner)) = args.args.first()
        {
            return Some(inner);
        }
    }
    None
}

fn vec_inner(ty: &Type) -> Option<&Type> {
    if let Type::Path(type_path) = ty {
        let segment = type_path.path.segments.last()?;
        if (segment.ident == "Vec" || segment.ident == "VecDeque")
            && let syn::PathArguments::AngleBracketed(args) = &segment.arguments
            && let Some(syn::GenericArgument::Type(inner)) = args.args.first()
        {
            return Some(inner);
        }
    }
    None
}

fn map_inner(ty: &Type) -> Option<(&Type, &Type)> {
    if let Type::Path(type_path) = ty {
        let segment = type_path.path.segments.last()?;
        if (segment.ident == "HashMap" || segment.ident == "BTreeMap")
            && let syn::PathArguments::AngleBracketed(args) = &segment.arguments
        {
            let mut iter = args.args.iter();
            let key = match iter.next()? {
                syn::GenericArgument::Type(ty) => ty,
                _ => return None,
            };
            let value = match iter.next()? {
                syn::GenericArgument::Type(ty) => ty,
                _ => return None,
            };
            return Some((key, value));
        }
    }
    None
}

fn is_string_type(ty: &Type) -> bool {
    match ty {
        Type::Path(type_path) => {
            let ident = &type_path.path.segments.last().map(|s| &s.ident);
            matches!(ident, Some(id) if *id == "String")
        }
        Type::Reference(reference) => match &*reference.elem {
            Type::Path(type_path) => {
                let ident = &type_path.path.segments.last().map(|s| &s.ident);
                matches!(ident, Some(id) if *id == "str")
            }
            _ => false,
        },
        _ => false,
    }
}

fn is_bool_type(ty: &Type) -> bool {
    matches!(ty, Type::Path(type_path) if type_path.path.is_ident("bool"))
}

fn is_integer_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty {
        let ident = type_path.path.segments.last().map(|s| s.ident.to_string());
        if let Some(name) = ident {
            return matches!(
                name.as_str(),
                "u8" | "u16"
                    | "u32"
                    | "u64"
                    | "u128"
                    | "usize"
                    | "i8"
                    | "i16"
                    | "i32"
                    | "i64"
                    | "i128"
                    | "isize"
            );
        }
    }
    false
}

fn is_number_type(ty: &Type) -> bool {
    matches!(ty, Type::Path(type_path) if type_path.path.is_ident("f32") || type_path.path.is_ident("f64"))
}

fn is_serde_json_value(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty
        && let Some(last) = type_path.path.segments.last()
    {
        return last.ident == "Value";
    }
    false
}

fn doc_comment(attrs: &[Attribute]) -> Option<String> {
    let mut parts = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("doc")
            && let Meta::NameValue(meta) = &attr.meta
            && let syn::Expr::Lit(expr_lit) = &meta.value
            && let Lit::Str(lit) = &expr_lit.lit
        {
            let value = lit.value();
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}
