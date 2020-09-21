// This file is part of Substrate.

// Copyright (C) 2020 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::helper;
use quote::ToTokens;
use syn::spanned::Spanned;

/// List of additional token to be used for parsing.
mod keyword {
	syn::custom_keyword!(DispatchResultWithPostInfo);
	syn::custom_keyword!(Call);
	syn::custom_keyword!(weight);
	syn::custom_keyword!(compact);
	syn::custom_keyword!(pallet);
}

/// Definition of dispatchables typically `impl<T: Trait> Module<T> { ... }`
pub struct CallDef {
	/// The where_clause used.
	pub where_clause: Option<syn::WhereClause>,
	/// A set of usage of instance, must be check for consistency with trait.
	pub instances: Vec<helper::InstanceUsage>,
	/// The index of call item in pallet module.
	pub index: usize,
	/// Information on methods (used for expansion).
	pub methods: Vec<CallVariantDef>,
	/// The span of the attribute.
	pub attr_span: proc_macro2::Span,
}

/// Definition of dispatchable typically: `#[weight...] fn foo(origin .., param1: ...) -> ..`
pub struct CallVariantDef {
	/// Function name.
	pub fn_: syn::Ident,
	/// Information on args: `(is_compact, name, type)`
	pub args: Vec<(bool, syn::Ident, Box<syn::Type>)>,
	/// Weight formula.
	pub weight: syn::Expr,
	/// Docs, used for metadata.
	pub docs: Vec<syn::Lit>,
}

/// Attributes for functions in call impl block.
/// Parse for `#[pallet::weight = expr]`
pub struct FunctionAttr {
	weight: syn::Expr,
}

impl syn::parse::Parse for FunctionAttr {
	fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
		input.parse::<syn::Token![#]>()?;
		let content;
		syn::bracketed!(content in input);
		content.parse::<keyword::pallet>()?;
		content.parse::<syn::Token![::]>()?;

		content.parse::<keyword::weight>()?;

		let weight_content;
		syn::parenthesized!(weight_content in content);
		Ok(FunctionAttr {
			weight: weight_content.parse::<syn::Expr>()?,
		})
	}
}

/// Attribute for arguments in function in call impl block.
/// Parse for `#[pallet::compact]|
pub struct ArgAttrIsCompact;

impl syn::parse::Parse for ArgAttrIsCompact {
	fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
		input.parse::<syn::Token![#]>()?;
		let content;
		syn::bracketed!(content in input);
		content.parse::<keyword::pallet>()?;
		content.parse::<syn::Token![::]>()?;

		content.parse::<keyword::compact>()?;
		Ok(ArgAttrIsCompact)
	}
}

impl CallDef {
	pub fn try_from(
		// Span needed for expansion
		attr_span: proc_macro2::Span,
		index: usize,
		item: &mut syn::Item
	) -> syn::Result<Self> {
		let item = if let syn::Item::Impl(item) = item {
			item
		} else {
			return Err(syn::Error::new(item.span(), "Invalid pallet::call, expect item impl"));
		};

		let mut instances = vec![];
		instances.push(helper::check_impl_generics(&item.generics, item.impl_token.span())?);
		instances.push(helper::check_module_usage(&item.self_ty)?);

		if let Some((_, _, for_)) = item.trait_ {
			let msg = "Invalid pallet::call, expect no trait ident as in \
				`impl<..> Module<..> { .. }`";
			return Err(syn::Error::new(for_.span(), msg))
		}

		let mut methods = vec![];
		for impl_item in &mut item.items {
			if let syn::ImplItem::Method(method) = impl_item {
				if method.sig.inputs.len() == 0 {
					let msg = "Invalid pallet::call, must have at least origin arg";
					return Err(syn::Error::new(method.sig.span(), msg));
				}
				helper::check_dispatchable_first_arg(&method.sig.inputs[0])?;

				if let syn::ReturnType::Type(_, type_) = &method.sig.output {
					syn::parse2::<keyword::DispatchResultWithPostInfo>(type_.to_token_stream())?;
				} else {
					let msg = "Invalid pallet::call, require return type \
						DispatchResultWithPostInfo";
					return Err(syn::Error::new(method.sig.span(), msg));
				}

				let mut call_var_attrs: Vec<FunctionAttr> =
					helper::take_item_attrs(&mut method.attrs)?;

				if call_var_attrs.len() != 1 {
					let msg = if call_var_attrs.len() == 0 {
						"Invalid pallet::call, require weight attribute i.e. `#[pallet::weight = $expr]`"
					} else {
						"Invalid pallet::call, to many weight attribute given"
					};
					return Err(syn::Error::new(method.sig.span(), msg));
				}
				let weight = call_var_attrs.pop().unwrap().weight;

				let mut args = vec![];
				for arg in method.sig.inputs.iter_mut().skip(1) {
					let arg = if let syn::FnArg::Typed(arg) = arg {
						arg
					} else {
						unreachable!("Only first argument can be receiver");
					};

					let arg_attrs: Vec<ArgAttrIsCompact> =
						helper::take_item_attrs(&mut arg.attrs)?;

					if arg_attrs.len() > 1 {
						let msg = "Invalid pallet::call, argument has too many attributes";
						return Err(syn::Error::new(arg.span(), msg));
					}

					let arg_ident = if let syn::Pat::Ident(pat) = &*arg.pat {
						pat.ident.clone()
					} else {
						let msg = "Invalid pallet::call, argument must be ident";
						return Err(syn::Error::new(arg.pat.span(), msg));
					};

					args.push((!arg_attrs.is_empty(), arg_ident, arg.ty.clone()));
				}

				let docs = helper::get_doc_literals(&method.attrs);

				methods.push(CallVariantDef {
					fn_: method.sig.ident.clone(),
					weight,
					args,
					docs,
				});
			} else {
				let msg = "Invalid pallet::call, only method accepted";
				return Err(syn::Error::new(impl_item.span(), msg));
			}
		}

		Ok(Self {
			index,
			attr_span,
			instances,
			methods,
			where_clause: item.generics.where_clause.clone(),
		})
	}
}